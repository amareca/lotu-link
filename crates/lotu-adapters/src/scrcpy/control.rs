//! Pure encoder for the scrcpy 5.0.1 control socket.
//!
//! The wire layout is normative in `docs/m2-contract.md` §2 and was verified
//! against the `v5.0.1` tag of the scrcpy source. Nothing here performs I/O:
//! [`encode`] turns one [`InputMessage`] into the exact bytes the server
//! expects, or fails with [`InputError::Invalid`] when the DTO cannot be encoded
//! safely.
//!
//! Only the M2 message types are emitted — inject keycode (`0`), inject text
//! (`1`), inject touch (`2`), inject scroll (`3`), back-or-screen-on (`4`) and
//! rotate (`11`). An unknown type kills the control session on the server, so
//! the encoder never writes arbitrary bytes.
//!
//! All integers are big-endian, like the video stream.

use lotu_app::dto::{KeyAction, KeyInput, PointerAction, PointerInput, ScrollInput};
use lotu_app::ports::{InputError, InputMessage};

/// Type byte of `InjectKeycode`.
const TYPE_INJECT_KEYCODE: u8 = 0;
/// Type byte of `InjectText`.
const TYPE_INJECT_TEXT: u8 = 1;
/// Type byte of `InjectTouch`.
const TYPE_INJECT_TOUCH: u8 = 2;
/// Type byte of `InjectScroll`.
const TYPE_INJECT_SCROLL: u8 = 3;
/// Type byte of `BackOrScreenOn`.
const TYPE_BACK_OR_SCREEN_ON: u8 = 4;
/// Type byte of `RotateDevice`; the whole message is this byte.
const TYPE_ROTATE_DEVICE: u8 = 11;
/// Type byte of `GetClipboard`.
const TYPE_GET_CLIPBOARD: u8 = 8;
/// Type byte of `SetClipboard`.
const TYPE_SET_CLIPBOARD: u8 = 9;

/// `SC_CONTROL_MSG_CLIPBOARD_TEXT_MAX_LENGTH` (256 KiB minus type/sequence/paste/length).
const MAX_CLIPBOARD_BYTES: usize = (1 << 18) - 14;

/// `MotionEvent.ACTION_DOWN`.
const ACTION_DOWN: u8 = 0;
/// `MotionEvent.ACTION_UP`.
const ACTION_UP: u8 = 1;
/// `MotionEvent.ACTION_MOVE`.
const ACTION_MOVE: u8 = 2;

/// Pointer id of the mouse (`SC_POINTER_ID_MOUSE`).
///
/// With `-1` and only the primary button the server injects a finger
/// (`Controller.injectTouch`), which is the M2 behaviour.
const POINTER_ID_MOUSE: i64 = -1;

/// Android button bitmask: PRIMARY | SECONDARY | TERTIARY | BACK | FORWARD.
const BUTTON_MASK: u32 = 0x1F;

/// Wire size of an inject-touch message.
const TOUCH_MESSAGE_LEN: usize = 32;
/// Wire size of an inject-scroll message.
const SCROLL_MESSAGE_LEN: usize = 21;
/// Wire size of an inject-keycode message.
const KEYCODE_MESSAGE_LEN: usize = 14;
/// Header of an inject-text message: type plus `u32` byte length.
const TEXT_HEADER_LEN: usize = 5;
/// `SC_CONTROL_MSG_INJECT_TEXT_MAX_LENGTH`: UTF-8 bytes, NUL not included.
const MAX_TEXT_BYTES: usize = 300;
/// Wheel notches that saturate one scroll axis (`[−16, 16]`).
const MAX_SCROLL_NOTCHES: f32 = 16.0;

/// Encodes one input message into control-socket bytes.
///
/// An empty text message encodes to an empty vector: there is nothing to write
/// and the caller must skip the socket write (the device has no text to inject).
///
/// # Errors
///
/// Returns [`InputError::Invalid`] when the message cannot be represented on the
/// wire (zero dimensions, a point outside the screen, non-finite pressure or
/// scroll, out-of-mask buttons, negative keycode/repeat/metaState).
pub fn encode(message: &InputMessage) -> Result<Vec<u8>, InputError> {
    match message {
        InputMessage::Pointer(input) => encode_pointer(input),
        InputMessage::Scroll(input) => encode_scroll(input),
        InputMessage::Key(input) => encode_key(input),
        InputMessage::Text(text) => Ok(encode_text(text)),
        InputMessage::BackOrScreenOn(action) => {
            Ok(vec![TYPE_BACK_OR_SCREEN_ON, action_byte(*action)])
        }
        InputMessage::Rotate => Ok(vec![TYPE_ROTATE_DEVICE]),
        InputMessage::SetClipboard { text, paste } => encode_set_clipboard(text, *paste),
        InputMessage::GetClipboard { copy_key } => encode_get_clipboard(*copy_key),
    }
}

/// Encodes `SET_CLIPBOARD`: type, sequence (0 = no ack), paste flag, length, text.
fn encode_set_clipboard(text: &str, paste: bool) -> Result<Vec<u8>, InputError> {
    let text = truncate_utf8(text, MAX_CLIPBOARD_BYTES);
    if text.is_empty() {
        return Err(InputError::Invalid(
            "clipboard text must not be empty".to_owned(),
        ));
    }

    let mut bytes = Vec::with_capacity(10 + 4 + text.len());
    bytes.push(TYPE_SET_CLIPBOARD);
    bytes.extend_from_slice(&0u64.to_be_bytes()); // sequence: no acknowledgement
    bytes.push(u8::from(paste));
    bytes.extend_from_slice(&(text.len() as u32).to_be_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(bytes)
}

/// Encodes `GET_CLIPBOARD`: type and copy-key (0 none, 1 copy, 2 cut).
fn encode_get_clipboard(copy_key: u8) -> Result<Vec<u8>, InputError> {
    if copy_key > 2 {
        return Err(InputError::Invalid(format!(
            "invalid clipboard copy key {copy_key}"
        )));
    }
    Ok(vec![TYPE_GET_CLIPBOARD, copy_key])
}

fn encode_pointer(input: &PointerInput) -> Result<Vec<u8>, InputError> {
    validate_screen(input.screen_width, input.screen_height)?;
    validate_point(input.x, input.y, input.screen_width, input.screen_height)?;
    if !input.pressure.is_finite() || !(0.0..=1.0).contains(&input.pressure) {
        return Err(InputError::Invalid(format!(
            "pressure {} is outside [0, 1]",
            input.pressure
        )));
    }
    validate_button_mask(input.buttons)?;
    validate_button_mask(input.action_button)?;

    let mut bytes = Vec::with_capacity(TOUCH_MESSAGE_LEN);
    bytes.push(TYPE_INJECT_TOUCH);
    bytes.push(match input.action {
        PointerAction::Down => ACTION_DOWN,
        PointerAction::Move => ACTION_MOVE,
        PointerAction::Up => ACTION_UP,
    });
    bytes.extend_from_slice(&POINTER_ID_MOUSE.to_be_bytes());
    bytes.extend_from_slice(&input.x.to_be_bytes());
    bytes.extend_from_slice(&input.y.to_be_bytes());
    bytes.extend_from_slice(&input.screen_width.to_be_bytes());
    bytes.extend_from_slice(&input.screen_height.to_be_bytes());
    bytes.extend_from_slice(&pressure_fixed(input.pressure).to_be_bytes());
    bytes.extend_from_slice(&input.action_button.to_be_bytes());
    bytes.extend_from_slice(&input.buttons.to_be_bytes());
    debug_assert_eq!(bytes.len(), TOUCH_MESSAGE_LEN);
    Ok(bytes)
}

fn encode_scroll(input: &ScrollInput) -> Result<Vec<u8>, InputError> {
    validate_screen(input.screen_width, input.screen_height)?;
    validate_point(input.x, input.y, input.screen_width, input.screen_height)?;
    if !input.h_scroll.is_finite() || !input.v_scroll.is_finite() {
        return Err(InputError::Invalid(
            "scroll notches must be finite numbers".to_owned(),
        ));
    }
    validate_button_mask(input.buttons)?;

    let mut bytes = Vec::with_capacity(SCROLL_MESSAGE_LEN);
    bytes.push(TYPE_INJECT_SCROLL);
    bytes.extend_from_slice(&input.x.to_be_bytes());
    bytes.extend_from_slice(&input.y.to_be_bytes());
    bytes.extend_from_slice(&input.screen_width.to_be_bytes());
    bytes.extend_from_slice(&input.screen_height.to_be_bytes());
    bytes.extend_from_slice(&scroll_fixed(input.h_scroll).to_be_bytes());
    bytes.extend_from_slice(&scroll_fixed(input.v_scroll).to_be_bytes());
    bytes.extend_from_slice(&input.buttons.to_be_bytes());
    debug_assert_eq!(bytes.len(), SCROLL_MESSAGE_LEN);
    Ok(bytes)
}

fn encode_key(input: &KeyInput) -> Result<Vec<u8>, InputError> {
    if input.keycode < 0 {
        return Err(InputError::Invalid(format!(
            "keycode {} must not be negative",
            input.keycode
        )));
    }
    if input.repeat < 0 {
        return Err(InputError::Invalid(format!(
            "repeat {} must not be negative",
            input.repeat
        )));
    }
    if input.meta_state < 0 {
        return Err(InputError::Invalid(format!(
            "metaState {} must not be negative",
            input.meta_state
        )));
    }

    let mut bytes = Vec::with_capacity(KEYCODE_MESSAGE_LEN);
    bytes.push(TYPE_INJECT_KEYCODE);
    bytes.push(action_byte(input.action));
    bytes.extend_from_slice(&input.keycode.to_be_bytes());
    bytes.extend_from_slice(&input.repeat.to_be_bytes());
    bytes.extend_from_slice(&input.meta_state.to_be_bytes());
    debug_assert_eq!(bytes.len(), KEYCODE_MESSAGE_LEN);
    Ok(bytes)
}

fn encode_text(text: &str) -> Vec<u8> {
    let truncated = truncate_utf8(text, MAX_TEXT_BYTES);
    if truncated.is_empty() {
        return Vec::new();
    }

    let mut bytes = Vec::with_capacity(TEXT_HEADER_LEN + truncated.len());
    bytes.push(TYPE_INJECT_TEXT);
    // `truncated.len() <= MAX_TEXT_BYTES`, so the cast cannot lose data.
    bytes.extend_from_slice(&(truncated.len() as u32).to_be_bytes());
    bytes.extend_from_slice(truncated.as_bytes());
    bytes
}

/// Truncates `text` to at most `max_bytes` UTF-8 bytes on a character boundary.
///
/// Mirrors scrcpy's `sc_str_utf8_truncation_index`: a character is kept only if
/// it fits completely, so multi-byte characters are never split.
fn truncate_utf8(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }

    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Fixed-point pressure as decoded by `Binary.u16FixedPointToFloat`.
///
/// `1.0` saturates to `0xFFFF` (which decodes back to exactly 1.0), `0.0` is
/// `0x0000`.
fn pressure_fixed(pressure: f32) -> u16 {
    let scaled = pressure.clamp(0.0, 1.0) * 65_536.0;
    scaled.trunc().min(65_535.0) as u16
}

/// Signed fixed-point scroll: `trunc(clamp(notches / 16, -1, 1) × 32768)`,
/// saturated to the `i16` range. One notch is `2048` (`0x0800`).
fn scroll_fixed(notches: f32) -> i16 {
    let normalized = (notches / MAX_SCROLL_NOTCHES).clamp(-1.0, 1.0);
    (normalized * 32_768.0).trunc().clamp(-32_768.0, 32_767.0) as i16
}

fn action_byte(action: KeyAction) -> u8 {
    match action {
        KeyAction::Down => ACTION_DOWN,
        KeyAction::Up => ACTION_UP,
    }
}

fn validate_screen(width: u16, height: u16) -> Result<(), InputError> {
    if width == 0 || height == 0 {
        return Err(InputError::Invalid(format!(
            "screen dimensions {width}x{height} must be non-zero"
        )));
    }
    Ok(())
}

fn validate_point(x: i32, y: i32, width: u16, height: u16) -> Result<(), InputError> {
    if x < 0 || y < 0 || x >= i32::from(width) || y >= i32::from(height) {
        return Err(InputError::Invalid(format!(
            "point ({x}, {y}) is outside the {width}x{height} screen"
        )));
    }
    Ok(())
}

fn validate_button_mask(buttons: u32) -> Result<(), InputError> {
    if buttons & !BUTTON_MASK != 0 {
        return Err(InputError::Invalid(format!(
            "buttons 0x{buttons:X} use bits outside 0x{BUTTON_MASK:X}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use lotu_app::dto::PointerAction;

    use super::*;

    const WIDTH: u16 = 1080;
    const HEIGHT: u16 = 2340;

    /// `-1` as a big-endian `i64`: the mouse pointer id.
    const POINTER_ID_BYTES: [u8; 8] = [0xFF; 8];

    fn pointer(
        action: PointerAction,
        pressure: f32,
        buttons: u32,
        action_button: u32,
    ) -> InputMessage {
        InputMessage::Pointer(PointerInput {
            action,
            x: 100,
            y: 200,
            screen_width: WIDTH,
            screen_height: HEIGHT,
            pressure,
            buttons,
            action_button,
        })
    }

    fn scroll(h_scroll: f32, v_scroll: f32) -> InputMessage {
        InputMessage::Scroll(ScrollInput {
            x: 540,
            y: 1200,
            screen_width: WIDTH,
            screen_height: HEIGHT,
            h_scroll,
            v_scroll,
            buttons: 0,
        })
    }

    fn key(action: KeyAction, keycode: i32) -> InputMessage {
        InputMessage::Key(KeyInput {
            action,
            keycode,
            repeat: 0,
            meta_state: 0,
        })
    }

    fn encode_ok(message: &InputMessage) -> Vec<u8> {
        encode(message).expect("message should encode")
    }

    #[test]
    fn touch_down_has_the_exact_wire_layout() {
        let expected: [u8; 32] = [
            0x02, // inject touch
            0x00, // ACTION_DOWN
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // pointer id -1
            0x00, 0x00, 0x00, 0x64, // x = 100
            0x00, 0x00, 0x00, 0xC8, // y = 200
            0x04, 0x38, // screen width 1080
            0x09, 0x24, // screen height 2340
            0xFF, 0xFF, // pressure 1.0 (saturated)
            0x00, 0x00, 0x00, 0x01, // action button PRIMARY
            0x00, 0x00, 0x00, 0x01, // buttons PRIMARY
        ];

        assert_eq!(
            encode_ok(&pointer(PointerAction::Down, 1.0, 1, 1)),
            expected
        );
    }

    #[test]
    fn touch_move_clears_the_action_button() {
        let expected: [u8; 32] = [
            0x02, // inject touch
            0x02, // ACTION_MOVE
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // pointer id -1
            0x00, 0x00, 0x00, 0x64, // x = 100
            0x00, 0x00, 0x00, 0xC8, // y = 200
            0x04, 0x38, // screen width
            0x09, 0x24, // screen height
            0xFF, 0xFF, // pressure 1.0
            0x00, 0x00, 0x00, 0x00, // action button: none
            0x00, 0x00, 0x00, 0x01, // buttons PRIMARY
        ];

        assert_eq!(
            encode_ok(&pointer(PointerAction::Move, 1.0, 1, 0)),
            expected
        );
    }

    #[test]
    fn touch_up_releases_pressure_and_buttons() {
        let expected: [u8; 32] = [
            0x02, // inject touch
            0x01, // ACTION_UP
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // pointer id -1
            0x00, 0x00, 0x00, 0x64, // x = 100
            0x00, 0x00, 0x00, 0xC8, // y = 200
            0x04, 0x38, // screen width
            0x09, 0x24, // screen height
            0x00, 0x00, // pressure 0.0
            0x00, 0x00, 0x00, 0x01, // action button PRIMARY
            0x00, 0x00, 0x00, 0x00, // buttons released
        ];

        assert_eq!(encode_ok(&pointer(PointerAction::Up, 0.0, 0, 1)), expected);
    }

    #[test]
    fn touch_pressure_is_fixed_point_and_saturates() {
        let encoded = encode_ok(&pointer(PointerAction::Down, 0.5, 1, 1));
        assert_eq!(encoded.len(), TOUCH_MESSAGE_LEN);
        assert_eq!(&encoded[22..24], &[0x80, 0x00]);

        let saturated = encode_ok(&pointer(PointerAction::Down, 1.0, 1, 1));
        assert_eq!(&saturated[22..24], &[0xFF, 0xFF]);
    }

    #[test]
    fn pointer_id_is_always_the_mouse_id() {
        let encoded = encode_ok(&pointer(PointerAction::Move, 1.0, 1, 0));
        assert_eq!(&encoded[2..10], &POINTER_ID_BYTES);
    }

    #[test]
    fn scroll_one_notch_up_is_0x0800() {
        let expected: [u8; 21] = [
            0x03, // inject scroll
            0x00, 0x00, 0x02, 0x1C, // x = 540
            0x00, 0x00, 0x04, 0xB0, // y = 1200
            0x04, 0x38, // screen width
            0x09, 0x24, // screen height
            0x00, 0x00, // hScroll 0
            0x08, 0x00, // vScroll +1 notch
            0x00, 0x00, 0x00, 0x00, // buttons
        ];

        assert_eq!(encode_ok(&scroll(0.0, 1.0)), expected);
    }

    #[test]
    fn scroll_one_notch_down_is_negative_2048() {
        let encoded = encode_ok(&scroll(0.0, -1.0));
        assert_eq!(encoded.len(), SCROLL_MESSAGE_LEN);
        assert_eq!(&encoded[15..17], &[0xF8, 0x00]);
        assert_eq!(&encoded[13..15], &[0x00, 0x00]);
    }

    #[test]
    fn scroll_right_is_positive_horizontal() {
        let encoded = encode_ok(&scroll(1.0, 0.0));
        assert_eq!(&encoded[13..15], &[0x08, 0x00]);
        assert_eq!(&encoded[15..17], &[0x00, 0x00]);
    }

    #[test]
    fn scroll_saturates_at_plus_minus_16_notches() {
        let up = encode_ok(&scroll(0.0, 16.0));
        assert_eq!(&up[15..17], &[0x7F, 0xFF]);

        let down = encode_ok(&scroll(0.0, -16.0));
        assert_eq!(&down[15..17], &[0x80, 0x00]);

        // Out-of-range notches are clamped, not rejected: the command layer
        // validates the DTO and the encoder stays defensive.
        let beyond_up = encode_ok(&scroll(0.0, 40.0));
        assert_eq!(&beyond_up[15..17], &[0x7F, 0xFF]);

        let beyond_down = encode_ok(&scroll(0.0, -40.0));
        assert_eq!(&beyond_down[15..17], &[0x80, 0x00]);
    }

    #[test]
    fn keycode_back_down_has_the_exact_wire_layout() {
        let expected: [u8; 14] = [
            0x00, // inject keycode
            0x00, // ACTION_DOWN
            0x00, 0x00, 0x00, 0x04, // KEYCODE_BACK
            0x00, 0x00, 0x00, 0x00, // repeat
            0x00, 0x00, 0x00, 0x00, // metaState
        ];

        assert_eq!(encode_ok(&key(KeyAction::Down, 4)), expected);
    }

    #[test]
    fn keycode_home_up_has_the_exact_wire_layout() {
        let expected: [u8; 14] = [
            0x00, // inject keycode
            0x01, // ACTION_UP
            0x00, 0x00, 0x00, 0x03, // KEYCODE_HOME
            0x00, 0x00, 0x00, 0x00, // repeat
            0x00, 0x00, 0x00, 0x00, // metaState
        ];

        assert_eq!(encode_ok(&key(KeyAction::Up, 3)), expected);
    }

    #[test]
    fn keycode_encodes_meta_state_big_endian() {
        let message = InputMessage::Key(KeyInput {
            action: KeyAction::Down,
            keycode: 66,
            repeat: 0,
            meta_state: 0x1001,
        });

        let encoded = encode_ok(&message);
        assert_eq!(encoded.len(), KEYCODE_MESSAGE_LEN);
        assert_eq!(&encoded[10..14], &[0x00, 0x00, 0x10, 0x01]);
    }

    #[test]
    fn text_hola_is_header_plus_utf8() {
        let expected = vec![0x01, 0x00, 0x00, 0x00, 0x04, b'h', b'o', b'l', b'a'];

        assert_eq!(encode_ok(&InputMessage::Text("hola".to_owned())), expected);
    }

    #[test]
    fn text_is_truncated_to_300_bytes_on_a_character_boundary() {
        // 151 × "é" = 302 bytes; the last character does not fit, so 150
        // characters (300 bytes) are kept.
        let text = "é".repeat(151);
        let encoded = encode_ok(&InputMessage::Text(text));

        assert_eq!(encoded.len(), TEXT_HEADER_LEN + MAX_TEXT_BYTES);
        assert_eq!(&encoded[..TEXT_HEADER_LEN], &[0x01, 0x00, 0x00, 0x01, 0x2C]);
        let payload = std::str::from_utf8(&encoded[TEXT_HEADER_LEN..])
            .expect("truncated payload should be valid UTF-8");
        assert_eq!(payload.chars().count(), 150);
        assert!(payload.chars().all(|c| c == 'é'));
    }

    #[test]
    fn text_truncation_steps_back_when_the_limit_splits_a_character() {
        // "a" + 150 × "é" = 301 bytes. Byte 300 is inside the last character, so
        // the encoder keeps 299 bytes (1 + 149 characters).
        let text = format!("a{}", "é".repeat(150));
        let encoded = encode_ok(&InputMessage::Text(text));

        assert_eq!(encoded.len(), TEXT_HEADER_LEN + 299);
        assert_eq!(&encoded[1..5], &[0x00, 0x00, 0x01, 0x2B]);
        let payload = std::str::from_utf8(&encoded[TEXT_HEADER_LEN..])
            .expect("truncated payload should be valid UTF-8");
        assert_eq!(payload.chars().count(), 150);
        assert!(payload.starts_with('a'));
    }

    #[test]
    fn text_shorter_than_the_limit_is_not_touched() {
        let text = "á".repeat(150); // exactly 300 bytes
        let encoded = encode_ok(&InputMessage::Text(text.clone()));

        assert_eq!(encoded.len(), TEXT_HEADER_LEN + 300);
        assert_eq!(&encoded[TEXT_HEADER_LEN..], text.as_bytes());
    }

    #[test]
    fn empty_text_produces_no_message() {
        assert!(encode_ok(&InputMessage::Text(String::new())).is_empty());
    }

    #[test]
    fn back_or_screen_on_has_two_bytes() {
        assert_eq!(
            encode_ok(&InputMessage::BackOrScreenOn(KeyAction::Down)),
            vec![0x04, 0x00]
        );
        assert_eq!(
            encode_ok(&InputMessage::BackOrScreenOn(KeyAction::Up)),
            vec![0x04, 0x01]
        );
    }

    #[test]
    fn rotate_is_a_single_byte() {
        assert_eq!(encode_ok(&InputMessage::Rotate), vec![0x0B]);
    }

    #[test]
    fn zero_screen_dimensions_are_invalid() {
        for (width, height) in [(0, HEIGHT), (WIDTH, 0)] {
            let message = InputMessage::Pointer(PointerInput {
                action: PointerAction::Down,
                x: 0,
                y: 0,
                screen_width: width,
                screen_height: height,
                pressure: 1.0,
                buttons: 1,
                action_button: 1,
            });
            assert!(matches!(encode(&message), Err(InputError::Invalid(_))));
        }
    }

    #[test]
    fn points_outside_the_screen_are_invalid() {
        let cases = [(1080, 0), (0, 2340), (-1, 0), (0, -1)];
        for (x, y) in cases {
            let message = InputMessage::Pointer(PointerInput {
                action: PointerAction::Down,
                x,
                y,
                screen_width: WIDTH,
                screen_height: HEIGHT,
                pressure: 1.0,
                buttons: 1,
                action_button: 1,
            });
            assert!(
                matches!(encode(&message), Err(InputError::Invalid(_))),
                "point ({x}, {y}) must be rejected"
            );
        }
    }

    #[test]
    fn pressure_outside_the_unit_range_is_invalid() {
        for pressure in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert!(
                matches!(
                    encode(&pointer(PointerAction::Down, pressure, 1, 1)),
                    Err(InputError::Invalid(_))
                ),
                "pressure {pressure} must be rejected"
            );
        }
    }

    #[test]
    fn buttons_outside_the_android_mask_are_invalid() {
        assert!(matches!(
            encode(&pointer(PointerAction::Down, 1.0, 0x20, 1)),
            Err(InputError::Invalid(_))
        ));
        assert!(matches!(
            encode(&pointer(PointerAction::Down, 1.0, 1, 0x20)),
            Err(InputError::Invalid(_))
        ));

        let message = InputMessage::Scroll(ScrollInput {
            x: 0,
            y: 0,
            screen_width: WIDTH,
            screen_height: HEIGHT,
            h_scroll: 0.0,
            v_scroll: 1.0,
            buttons: 0x40,
        });
        assert!(matches!(encode(&message), Err(InputError::Invalid(_))));
    }

    #[test]
    fn non_finite_scroll_is_invalid() {
        assert!(matches!(
            encode(&scroll(f32::NAN, 0.0)),
            Err(InputError::Invalid(_))
        ));
        assert!(matches!(
            encode(&scroll(0.0, f32::INFINITY)),
            Err(InputError::Invalid(_))
        ));
    }

    #[test]
    fn negative_key_fields_are_invalid() {
        let negative_keycode = InputMessage::Key(KeyInput {
            action: KeyAction::Down,
            keycode: -1,
            repeat: 0,
            meta_state: 0,
        });
        let negative_repeat = InputMessage::Key(KeyInput {
            action: KeyAction::Down,
            keycode: 4,
            repeat: -1,
            meta_state: 0,
        });
        let negative_meta = InputMessage::Key(KeyInput {
            action: KeyAction::Down,
            keycode: 4,
            repeat: 0,
            meta_state: -1,
        });

        for message in [negative_keycode, negative_repeat, negative_meta] {
            assert!(matches!(encode(&message), Err(InputError::Invalid(_))));
        }
    }

    #[test]
    fn truncate_utf8_never_splits_a_character() {
        assert_eq!(truncate_utf8("hola", 300), "hola");
        assert_eq!(truncate_utf8("é", 1), "");
        assert_eq!(truncate_utf8("é", 2), "é");
        assert_eq!(truncate_utf8("aé", 2), "a");
        assert_eq!(truncate_utf8("aé", 0), "");
    }

    #[test]
    fn set_clipboard_has_the_exact_wire_layout() {
        let bytes = encode_ok(&InputMessage::SetClipboard {
            text: "hola".to_owned(),
            paste: true,
        });
        assert_eq!(bytes[0], TYPE_SET_CLIPBOARD);
        assert_eq!(&bytes[1..9], &[0u8; 8]); // sequence 0: no acknowledgement
        assert_eq!(bytes[9], 1);
        assert_eq!(&bytes[10..14], &4u32.to_be_bytes());
        assert_eq!(&bytes[14..], b"hola");
    }

    #[test]
    fn get_clipboard_encodes_copy_key_and_rejects_invalid_values() {
        assert_eq!(
            encode_ok(&InputMessage::GetClipboard { copy_key: 1 }),
            vec![TYPE_GET_CLIPBOARD, 1]
        );
        assert!(matches!(
            encode(&InputMessage::GetClipboard { copy_key: 3 }),
            Err(InputError::Invalid(_))
        ));
    }
}
