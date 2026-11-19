//! Port that delivers user input to a live device.
//!
//! The vocabulary is the Android semantics (keycodes, buttons, actions) agreed in
//! `docs/stream-protocol.md` and `docs/m2-contract.md` §3.1. Byte layouts, the
//! mouse pointer id and the fixed-point conversions stay in `lotu-adapters`.

use async_trait::async_trait;

use crate::dto::{KeyAction, KeyInput, PointerInput, ScrollInput};

/// Destination of the input produced by the application.
///
/// Implementations own the transport (the scrcpy control socket in M2). The
/// trait is object safe so [`LiveStream`](crate::ports::LiveStream) can expose an
/// `Arc<dyn InputSink>`.
#[async_trait]
pub trait InputSink: Send + Sync {
    /// Queues the message for serialized writing.
    ///
    /// It never blocks a command beyond the bounded policy of the adapter
    /// (`docs/m2-contract.md` §3.2). `Ok` does **not** mean "injected": the
    /// device never acknowledges input.
    async fn send(&self, message: InputMessage) -> Result<(), InputError>;

    /// Reads the device clipboard, optionally injecting COPY/CUT first.
    ///
    /// The server answers on the same control socket; implementations bound the
    /// wait and fail with `InputError::Failed` on timeout.
    async fn get_clipboard(&self, copy_key: u8) -> Result<String, InputError>;
}

/// One piece of input in Android terms.
#[derive(Debug, Clone, PartialEq)]
pub enum InputMessage {
    Pointer(PointerInput),
    Scroll(ScrollInput),
    Key(KeyInput),
    Text(String),
    BackOrScreenOn(KeyAction),
    Rotate,
    /// Sets the device clipboard; `paste` also injects KEYCODE_PASTE.
    SetClipboard {
        text: String,
        paste: bool,
    },
    /// Requests the device clipboard; `copy_key`: 0 none, 1 copy, 2 cut.
    GetClipboard {
        copy_key: u8,
    },
    /// Asks scrcpy to resend the configuration and an immediate keyframe
    /// (`RESET_VIDEO`, control type 17). Used when starting a recording.
    ResetVideo,
}

/// `GET_CLIPBOARD` copy-key values (`control_msg.h` of scrcpy 5.0.1).
pub const COPY_KEY_NONE: u8 = 0;
pub const COPY_KEY_COPY: u8 = 1;
pub const COPY_KEY_CUT: u8 = 2;

/// Failure modes of [`InputSink::send`].
///
/// The delivery layer maps each variant to a stable frontend error code
/// (`docs/m2-contract.md` §3.4).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// Range or shape validation failed; the message was not sent.
    #[error("invalid input: {0}")]
    Invalid(String),
    /// The channel is closed, the socket broke or the queue stayed full.
    #[error("input delivery failed: {0}")]
    Failed(String),
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::dto::PointerAction;

    /// A sink that records the messages it receives.
    struct RecordingSink {
        sent: Arc<Mutex<Vec<InputMessage>>>,
    }

    #[async_trait]
    impl InputSink for RecordingSink {
        async fn send(&self, message: InputMessage) -> Result<(), InputError> {
            self.sent.lock().expect("sent lock").push(message);
            Ok(())
        }

        async fn get_clipboard(&self, _copy_key: u8) -> Result<String, InputError> {
            Ok(String::new())
        }
    }

    fn pointer() -> PointerInput {
        PointerInput {
            action: PointerAction::Down,
            x: 100,
            y: 200,
            screen_width: 1080,
            screen_height: 2340,
            pressure: 1.0,
            buttons: 1,
            action_button: 1,
        }
    }

    #[tokio::test]
    async fn input_sink_is_dyn_compatible_and_receives_messages_in_order() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let sink: Arc<dyn InputSink> = Arc::new(RecordingSink {
            sent: Arc::clone(&sent),
        });

        sink.send(InputMessage::Pointer(pointer()))
            .await
            .expect("sink accepts pointer");
        sink.send(InputMessage::BackOrScreenOn(KeyAction::Up))
            .await
            .expect("sink accepts back");
        sink.send(InputMessage::Rotate)
            .await
            .expect("sink accepts rotate");
        sink.send(InputMessage::Text("hola".to_owned()))
            .await
            .expect("sink accepts text");

        let sent = sent.lock().expect("sent lock");
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[0], InputMessage::Pointer(pointer()));
        assert_eq!(sent[1], InputMessage::BackOrScreenOn(KeyAction::Up));
        assert_eq!(sent[2], InputMessage::Rotate);
        assert_eq!(sent[3], InputMessage::Text("hola".to_owned()));
    }

    #[test]
    fn input_error_keeps_the_contract_variants() {
        let invalid = InputError::Invalid("x out of range".to_owned());
        let failed = InputError::Failed("channel closed".to_owned());

        assert!(invalid.to_string().contains("x out of range"));
        assert!(failed.to_string().contains("channel closed"));
    }
}
