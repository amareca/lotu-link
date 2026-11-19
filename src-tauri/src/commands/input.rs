//! M2 input commands: thin adapters from typed DTOs to the live session sink.
//!
//! Every command resolves the active live session, validates its payload and
//! delegates to `InputSink`. No scrcpy layout, pointer id or fixed-point
//! conversion belongs here: those stay in `lotu-adapters`
//! (`docs/m2-contract.md` §3.5). Input is fire-and-forget: success means the
//! message was queued, not that the device injected it.

use std::sync::Arc;
use std::time::Duration;

use lotu_app::dto::{KeyAction, KeyInput, PointerInput, ScrollInput};
use lotu_app::ports::{InputMessage, InputSink, COPY_KEY_COPY};
use tauri::State;

use crate::commands::error::{ErrorCode, ErrorDto};
use crate::state::AppState;

const BUTTON_MASK: u32 = 0x1F;
const MAX_NOTCHES: f32 = 16.0;
/// Android `KEYCODE_PASTE` (verified in scrcpy 5.0.1 `android/keycodes.h`).
const KEYCODE_PASTE: i32 = 279;
/// Small delay between setting the clipboard and injecting PASTE.
const PASTE_DELAY: Duration = Duration::from_millis(60);

/// Sends one touch/pointer event to the live session.
#[tauri::command]
pub async fn send_pointer(input: PointerInput, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    validate_pointer(&input)?;
    let sink = input_sink(&state)?;
    sink.send(InputMessage::Pointer(input))
        .await
        .map_err(ErrorDto::from)
}

/// Sends one wheel/scroll event to the live session.
#[tauri::command]
pub async fn send_scroll(input: ScrollInput, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    validate_scroll(&input)?;
    let sink = input_sink(&state)?;
    sink.send(InputMessage::Scroll(input))
        .await
        .map_err(ErrorDto::from)
}

/// Sends one Android keycode event to the live session.
#[tauri::command]
pub async fn send_key(input: KeyInput, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    validate_key(&input)?;
    let sink = input_sink(&state)?;
    sink.send(InputMessage::Key(input))
        .await
        .map_err(ErrorDto::from)
}

/// Sends printable text to the live session (UTF-8, truncated by the encoder).
#[tauri::command]
pub async fn send_text(text: String, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    if text.is_empty() {
        return Err(invalid("text must not be empty"));
    }
    let sink = input_sink(&state)?;
    sink.send(InputMessage::Text(text))
        .await
        .map_err(ErrorDto::from)
}

/// Presses Back, or wakes the screen when it is off (control type 4).
///
/// Distinct from `send_key(BACK)` because type 4 handles a sleeping display,
/// which the keycode path does not (`docs/m2-contract.md` §2.5).
#[tauri::command]
pub async fn send_back(action: KeyAction, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    let sink = input_sink(&state)?;
    sink.send(InputMessage::BackOrScreenOn(action))
        .await
        .map_err(ErrorDto::from)
}

/// Toggles device orientation between portrait and landscape (control type 11).
#[tauri::command]
pub async fn rotate(state: State<'_, AppState>) -> Result<(), ErrorDto> {
    let sink = input_sink(&state)?;
    sink.send(InputMessage::Rotate)
        .await
        .map_err(ErrorDto::from)
}

/// Copies PC text into the device clipboard and injects PASTE.
#[tauri::command]
pub async fn paste_text(text: String, state: State<'_, AppState>) -> Result<(), ErrorDto> {
    if text.is_empty() {
        return Err(invalid("text must not be empty"));
    }
    let sink = input_sink(&state)?;

    // Set the clipboard without asking the server to inject PASTE: some OEM
    // keymaps ignore the server's dedicated paste key. Injecting our own
    // keycode through the already-proven input path is more reliable.
    sink.send(InputMessage::SetClipboard { text, paste: false })
        .await
        .map_err(ErrorDto::from)?;

    tokio::time::sleep(PASTE_DELAY).await;

    let down = KeyInput {
        action: KeyAction::Down,
        keycode: KEYCODE_PASTE,
        repeat: 0,
        meta_state: 0,
    };
    let up = KeyInput {
        action: KeyAction::Up,
        ..down.clone()
    };
    sink.send(InputMessage::Key(down))
        .await
        .map_err(ErrorDto::from)?;
    sink.send(InputMessage::Key(up))
        .await
        .map_err(ErrorDto::from)
}

/// Copies the device clipboard (injecting COPY first) and returns its text.
#[tauri::command]
pub async fn copy_text(state: State<'_, AppState>) -> Result<String, ErrorDto> {
    let sink = input_sink(&state)?;
    sink.get_clipboard(COPY_KEY_COPY)
        .await
        .map_err(ErrorDto::from)
}

fn input_sink(state: &AppState) -> Result<Arc<dyn InputSink>, ErrorDto> {
    state.session_input().ok_or_else(|| {
        ErrorDto::new(
            ErrorCode::SessionInactive,
            "no live session is active (replay does not accept input)",
        )
    })
}

fn validate_pointer(input: &PointerInput) -> Result<(), ErrorDto> {
    validate_point(input.x, input.y, input.screen_width, input.screen_height)?;
    if !input.pressure.is_finite() || !(0.0..=1.0).contains(&input.pressure) {
        return Err(invalid("pressure must be in 0.0..=1.0"));
    }
    validate_buttons(input.buttons, "buttons")?;
    validate_buttons(input.action_button, "actionButton")?;
    Ok(())
}

fn validate_scroll(input: &ScrollInput) -> Result<(), ErrorDto> {
    validate_point(input.x, input.y, input.screen_width, input.screen_height)?;
    if !input.h_scroll.is_finite()
        || !input.v_scroll.is_finite()
        || input.h_scroll.abs() > MAX_NOTCHES
        || input.v_scroll.abs() > MAX_NOTCHES
    {
        return Err(invalid("scroll must be finite and within [-16, 16]"));
    }
    validate_buttons(input.buttons, "buttons")?;
    Ok(())
}

fn validate_key(input: &KeyInput) -> Result<(), ErrorDto> {
    if input.keycode < 0 || input.repeat < 0 || input.meta_state < 0 {
        return Err(invalid(
            "keycode, repeat and metaState must be non-negative",
        ));
    }
    Ok(())
}

fn validate_point(x: i32, y: i32, width: u16, height: u16) -> Result<(), ErrorDto> {
    if width == 0 || height == 0 {
        return Err(invalid("screen dimensions must be positive"));
    }
    if x < 0 || x >= i32::from(width) || y < 0 || y >= i32::from(height) {
        return Err(invalid("point is outside the screen dimensions"));
    }
    Ok(())
}

fn validate_buttons(value: u32, name: &str) -> Result<(), ErrorDto> {
    if value & !BUTTON_MASK != 0 {
        return Err(invalid(format!("{name} contains unknown Android buttons")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ErrorDto {
    ErrorDto::new(ErrorCode::InputInvalid, message)
}
