//! Port of a live video stream that must be closed explicitly.

use async_trait::async_trait;

use crate::ports::FrameSource;

/// Live stream that owns remote resources (sockets, processes, ADB forwards).
///
/// [`FrameSource`] is left untouched: replay fixtures do not need cleanup. A
/// live source does, so the cleanup is part of the stream contract and the
/// session actor can always run it, including on failure paths.
#[async_trait]
pub trait SessionStream: FrameSource {
    /// Releases every resource of the session.
    ///
    /// The implementation must be deterministic and bounded: best effort per
    /// step with timeouts, failures logged instead of panicking, and no way to
    /// leave a process behind. Calling it consumes the stream, so cleanup can
    /// only run once.
    async fn close(self: Box<Self>);
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::ports::StreamMessage;

    struct FakeSessionStream {
        closes: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl FrameSource for FakeSessionStream {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            None
        }
    }

    #[async_trait]
    impl SessionStream for FakeSessionStream {
        async fn close(self: Box<Self>) {
            *self.closes.lock().expect("closes lock") += 1;
        }
    }

    fn close_count(closes: &Mutex<usize>) -> usize {
        *closes.lock().expect("closes lock")
    }

    #[tokio::test]
    async fn close_is_delivered_exactly_once() {
        let closes = Arc::new(Mutex::new(0));
        let stream: Box<dyn SessionStream> = Box::new(FakeSessionStream {
            closes: Arc::clone(&closes),
        });

        assert_eq!(close_count(&closes), 0);
        stream.close().await;
        assert_eq!(close_count(&closes), 1, "close() must run once");
    }

    #[tokio::test]
    async fn boxed_session_stream_is_still_a_frame_source() {
        let closes = Arc::new(Mutex::new(0));
        let mut stream: Box<dyn SessionStream> = Box::new(FakeSessionStream {
            closes: Arc::clone(&closes),
        });

        assert!(stream.next_message().await.is_none());
        stream.close().await;
        assert_eq!(close_count(&closes), 1, "close() must run once");
    }
}
