use async_trait::async_trait;

/// One message of the internal video stream.
///
/// The replay adapter produces these values from a fixture; in M1 the scrcpy
/// adapter will produce them from the live stream. The delivery layer is
/// responsible for serializing them into the local WebSocket framing described
/// in `docs/stream-protocol.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamMessage {
    /// Decoder configuration. `avcc` is a complete AVCDecoderConfigurationRecord
    /// (`lengthSizeMinusOne = 3`). The payload is sent before the first frame and
    /// again whenever the video configuration changes.
    Config {
        width: u32,
        height: u32,
        avcc: Vec<u8>,
    },
    /// One H.264 access unit in AVCC (length-prefixed) format.
    Frame {
        pts_us: i64,
        keyframe: bool,
        data: Vec<u8>,
    },
}

/// Source of normalized video messages (fixture replay in M0, scrcpy in M1).
///
/// A source yields exactly one configuration message before its first frame;
/// `None` marks the end of the stream.
#[async_trait]
pub trait FrameSource: Send {
    async fn next_message(&mut self) -> Option<StreamMessage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OneShotSource {
        messages: Vec<StreamMessage>,
    }

    #[async_trait]
    impl FrameSource for OneShotSource {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            if self.messages.is_empty() {
                None
            } else {
                Some(self.messages.remove(0))
            }
        }
    }

    #[tokio::test]
    async fn frame_source_is_dyn_compatible_and_ends_with_none() {
        let mut source: Box<dyn FrameSource> = Box::new(OneShotSource {
            messages: vec![
                StreamMessage::Config {
                    width: 1080,
                    height: 2400,
                    avcc: vec![1, 0x42],
                },
                StreamMessage::Frame {
                    pts_us: 0,
                    keyframe: true,
                    data: vec![0, 0, 0, 1, 0x65],
                },
            ],
        });

        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Config { width: 1080, .. })
        ));
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
        assert!(source.next_message().await.is_none());
    }
}
