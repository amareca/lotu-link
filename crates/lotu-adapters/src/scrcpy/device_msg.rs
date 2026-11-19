//! Parser for messages sent by scrcpy-server on the control socket.
//!
//! Layouts verified against `app/src/device_msg.c` of scrcpy v5.0.1. Only the
//! clipboard message is needed by M3; ACK and UHID messages are consumed so the
//! stream stays framed, but their payloads are ignored.

/// A device message could not be framed or decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid device control message")]
pub struct DeviceMessageError;

const TYPE_CLIPBOARD: u8 = 0;
const TYPE_ACK_CLIPBOARD: u8 = 1;
const TYPE_UHID_OUTPUT: u8 = 2;

/// A complete device message and its wire size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceMessage {
    Clipboard(String),
    AckClipboard(u64),
    UhidOutput,
}

/// Parses one message from the front of `buf`.
///
/// Returns `Ok(None)` when more bytes are needed, `Ok(Some(msg))` with the
/// number of consumed bytes, and `Err(())` for an unrecoverable framing error.
pub fn parse(buf: &[u8]) -> Result<Option<(usize, DeviceMessage)>, DeviceMessageError> {
    let Some(&kind) = buf.first() else {
        return Ok(None);
    };

    match kind {
        TYPE_CLIPBOARD => {
            if buf.len() < 5 {
                return Ok(None);
            }
            let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if buf.len() < 5 + len {
                return Ok(None);
            }
            let text = std::str::from_utf8(&buf[5..5 + len]).map_err(|_| DeviceMessageError)?;
            Ok(Some((5 + len, DeviceMessage::Clipboard(text.to_owned()))))
        }
        TYPE_ACK_CLIPBOARD => {
            if buf.len() < 9 {
                return Ok(None);
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&buf[1..9]);
            Ok(Some((
                9,
                DeviceMessage::AckClipboard(u64::from_be_bytes(bytes)),
            )))
        }
        TYPE_UHID_OUTPUT => {
            if buf.len() < 5 {
                return Ok(None);
            }
            let size = u16::from_be_bytes([buf[3], buf[4]]) as usize;
            if buf.len() < 5 + size {
                return Ok(None);
            }
            Ok(Some((5 + size, DeviceMessage::UhidOutput)))
        }
        _ => Err(DeviceMessageError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clipboard_text() {
        let mut buf = vec![TYPE_CLIPBOARD];
        buf.extend_from_slice(&3u32.to_be_bytes());
        buf.extend_from_slice(b"hola");
        assert_eq!(
            parse(&buf).unwrap(),
            Some((8, DeviceMessage::Clipboard("hol".to_owned())))
        );
    }

    #[test]
    fn waits_for_more_bytes() {
        let buf = [TYPE_CLIPBOARD, 0, 0, 0, 5, b'a'];
        assert_eq!(parse(&buf).unwrap(), None);
    }

    #[test]
    fn parses_ack_and_ignores_uhid_payload() {
        let mut ack = vec![TYPE_ACK_CLIPBOARD];
        ack.extend_from_slice(&42u64.to_be_bytes());
        assert_eq!(
            parse(&ack).unwrap(),
            Some((9, DeviceMessage::AckClipboard(42)))
        );

        let mut uhid = vec![TYPE_UHID_OUTPUT, 0, 1, 0, 2, 0xAA, 0xBB];
        assert_eq!(parse(&uhid).unwrap(), Some((7, DeviceMessage::UhidOutput)));
        uhid.pop();
        assert_eq!(parse(&uhid).unwrap(), None);
    }

    #[test]
    fn rejects_unknown_type() {
        assert!(parse(&[99]).is_err());
    }
}
