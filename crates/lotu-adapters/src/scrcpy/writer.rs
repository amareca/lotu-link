//! Serialized writer of the scrcpy control socket.
//!
//! [`spawn_writer`] owns the write half of the control socket and drains a
//! bounded queue with a single `recv → write_all` loop, so concurrently issued
//! commands can never interleave partial messages on the wire.
//!
//! [`ScrcpyInputSink`] is the [`InputSink`] facade: it encodes the message with
//! the pure [`control::encode`] and queues the resulting bytes. The queue policy
//! is normative in `docs/m2-contract.md` §3.2:
//!
//! - `try_send` first; a full queue only costs the command a bounded wait.
//! - A full queue drops pointer **moves** (`Ok`): the frontend already batches
//!   them per animation frame, so the newest position arrives shortly.
//! - Any other message waits at most [`SEND_TIMEOUT`] and then fails.
//! - A closed channel (writer stopped, socket broken, session torn down) fails
//!   immediately.
//!
//! No `Ok` means "injected": the device never acknowledges input.

use std::time::Duration;

use async_trait::async_trait;
use lotu_app::dto::PointerAction;
use lotu_app::ports::{InputError, InputMessage, InputSink};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, error::SendTimeoutError, error::TrySendError};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use super::control;
use super::device_msg::{self, DeviceMessage};

/// Capacity of the writer queue (`docs/m2-contract.md` §3.2 and D14).
pub const WRITER_QUEUE_CAPACITY: usize = 256;

/// Upper bound a non-move command waits for room in a full queue.
pub const SEND_TIMEOUT: Duration = Duration::from_millis(50);

/// Upper bound waiting for the server answer to `GET_CLIPBOARD`.
pub const CLIPBOARD_TIMEOUT: Duration = Duration::from_secs(3);

/// [`InputSink`] backed by the session's control-socket writer.
///
/// Holds only the sender: encoding happens on the command path (pure, no
/// locks), writing happens in the single writer task.
#[derive(Debug, Clone)]
pub struct ScrcpyInputSink {
    tx: mpsc::Sender<Vec<u8>>,
    clipboard_tx: broadcast::Sender<String>,
}

impl ScrcpyInputSink {
    /// Builds the sink around an existing queue sender.
    ///
    /// Used by [`spawn_writer`]; callers outside this module should go through
    /// that function so the writer task always exists.
    fn new(tx: mpsc::Sender<Vec<u8>>, clipboard_tx: broadcast::Sender<String>) -> Self {
        Self { tx, clipboard_tx }
    }

    /// Subscribes to clipboard texts received from the device.
    pub fn clipboard_subscribe(&self) -> broadcast::Receiver<String> {
        self.clipboard_tx.subscribe()
    }

    /// Sender used by the control-socket reader to publish clipboard texts.
    pub fn clipboard_sender(&self) -> broadcast::Sender<String> {
        self.clipboard_tx.clone()
    }
}

#[async_trait]
impl InputSink for ScrcpyInputSink {
    async fn send(&self, message: InputMessage) -> Result<(), InputError> {
        let is_move = is_pointer_move(&message);
        let bytes = control::encode(&message)?;

        // Empty text encodes to nothing: there is nothing to inject, so there
        // is nothing to write (and an empty `write_all` would be a no-op).
        if bytes.is_empty() {
            return Ok(());
        }

        match self.tx.try_send(bytes) {
            Ok(()) => Ok(()),
            Err(TrySendError::Closed(_)) => Err(writer_closed()),
            Err(TrySendError::Full(bytes)) => {
                if is_move {
                    // Dropped on purpose: a newer move is already queued or
                    // about to be, and blocking here would stall the command.
                    return Ok(());
                }

                match self.tx.send_timeout(bytes, SEND_TIMEOUT).await {
                    Ok(()) => Ok(()),
                    Err(SendTimeoutError::Closed(_)) => Err(writer_closed()),
                    Err(SendTimeoutError::Timeout(_)) => Err(InputError::Failed(format!(
                        "control writer queue stayed full for {} ms",
                        SEND_TIMEOUT.as_millis()
                    ))),
                }
            }
        }
    }

    async fn get_clipboard(&self, copy_key: u8) -> Result<String, InputError> {
        // Subscribe before sending: the answer may arrive immediately.
        let mut clipboard = self.clipboard_subscribe();
        self.send(InputMessage::GetClipboard { copy_key }).await?;

        match timeout(CLIPBOARD_TIMEOUT, clipboard.recv()).await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(broadcast::error::RecvError::Closed)) => Err(InputError::Failed(
                "device clipboard channel is closed".to_owned(),
            )),
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => Err(InputError::Failed(
                "device clipboard response was missed".to_owned(),
            )),
            Err(_) => Err(InputError::Failed(format!(
                "device clipboard did not answer within {} ms",
                CLIPBOARD_TIMEOUT.as_millis()
            ))),
        }
    }
}

/// Starts the writer task and returns the sink that feeds it.
///
/// The task writes queued messages in FIFO order until the channel closes or a
/// socket write fails. In both cases it drops the receiver, which closes the
/// channel and makes every later [`InputSink::send`] fail; the write half is
/// dropped too, so the server sees the broken pipe and exits, as in M1.
///
/// Must be called inside a Tokio runtime.
pub fn spawn_writer(write_half: OwnedWriteHalf) -> (ScrcpyInputSink, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(WRITER_QUEUE_CAPACITY);
    let (clipboard_tx, _) = broadcast::channel(8);
    let handle = tokio::spawn(write_loop(write_half, rx));
    (ScrcpyInputSink::new(tx, clipboard_tx), handle)
}

/// Reads device messages from the control socket and publishes clipboard texts.
///
/// Non-clipboard messages are consumed for framing and ignored. An invalid
/// message ends the receiver; the session stream will notice the socket close.
pub async fn receive_device_messages(
    mut reader: OwnedReadHalf,
    clipboard_tx: broadcast::Sender<String>,
) {
    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0u8; 8192];

    loop {
        let read = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(size) => size,
        };
        buffer.extend_from_slice(&chunk[..read]);

        loop {
            match device_msg::parse(&buffer) {
                Ok(Some((consumed, message))) => {
                    if let DeviceMessage::Clipboard(text) = message {
                        let _ = clipboard_tx.send(text);
                    }
                    buffer.drain(..consumed);
                }
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
}

/// Serialized drain loop: one message at a time, no interleaving.
async fn write_loop<W>(mut writer: W, mut rx: mpsc::Receiver<Vec<u8>>)
where
    W: AsyncWrite + Unpin,
{
    while let Some(bytes) = rx.recv().await {
        if writer.write_all(&bytes).await.is_err() {
            // Dropping `rx` closes the channel: the sink reports `Failed`.
            break;
        }
    }
}

fn is_pointer_move(message: &InputMessage) -> bool {
    matches!(message, InputMessage::Pointer(input) if input.action == PointerAction::Move)
}

fn writer_closed() -> InputError {
    InputError::Failed("control writer channel is closed".to_owned())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::net::Ipv4Addr;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use lotu_app::dto::{KeyInput, PointerInput, ScrollInput};
    use tokio::io::AsyncReadExt;
    use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::time::{timeout, Instant};

    use super::*;
    use crate::scrcpy::control;

    const READ_TIMEOUT: Duration = Duration::from_secs(5);

    fn test_sink(tx: mpsc::Sender<Vec<u8>>) -> ScrcpyInputSink {
        let (clipboard_tx, _) = broadcast::channel(8);
        ScrcpyInputSink::new(tx, clipboard_tx)
    }

    fn pointer(action: PointerAction) -> InputMessage {
        InputMessage::Pointer(PointerInput {
            action,
            x: 100,
            y: 200,
            screen_width: 1080,
            screen_height: 2340,
            pressure: if action == PointerAction::Up {
                0.0
            } else {
                1.0
            },
            buttons: if action == PointerAction::Up { 0 } else { 1 },
            action_button: if action == PointerAction::Move { 0 } else { 1 },
        })
    }

    /// A loopback pair: the client halves and the server end of the socket.
    async fn connected_pair() -> (OwnedReadHalf, OwnedWriteHalf, TcpStream) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("local address");

        let accept = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.expect("accept");
            socket
        });

        let client = TcpStream::connect(address).await.expect("connect");
        let server = accept.await.expect("accept task");
        let (read_half, write_half) = client.into_split();

        (read_half, write_half, server)
    }

    #[test]
    fn queue_capacity_and_bounded_wait_follow_the_contract() {
        assert_eq!(WRITER_QUEUE_CAPACITY, 256);
        assert_eq!(SEND_TIMEOUT, Duration::from_millis(50));
    }

    #[tokio::test]
    async fn messages_reach_the_socket_in_fifo_order_with_exact_bytes() {
        let (_read_half, write_half, mut server) = connected_pair().await;
        let (sink, writer) = spawn_writer(write_half);

        let messages = [
            pointer(PointerAction::Down),
            InputMessage::Rotate,
            InputMessage::Text("hola".to_owned()),
            InputMessage::Scroll(ScrollInput {
                x: 540,
                y: 1200,
                screen_width: 1080,
                screen_height: 2340,
                h_scroll: 0.0,
                v_scroll: -1.0,
                buttons: 0,
            }),
            InputMessage::Key(KeyInput {
                action: lotu_app::dto::KeyAction::Down,
                keycode: 66,
                repeat: 0,
                meta_state: 0,
            }),
            pointer(PointerAction::Up),
        ];

        let mut expected = Vec::new();
        for message in &messages {
            expected.extend(control::encode(message).expect("message encodes"));
            sink.send(message.clone()).await.expect("sink accepts");
        }

        let mut received = vec![0u8; expected.len()];
        timeout(READ_TIMEOUT, server.read_exact(&mut received))
            .await
            .expect("read before the timeout")
            .expect("read the whole payload");
        assert_eq!(received, expected);

        // The server side saw the stream; now close the sink and let the
        // writer task finish so no task is left behind.
        drop(sink);
        timeout(READ_TIMEOUT, writer)
            .await
            .expect("writer joins")
            .expect("writer task does not panic");
    }

    #[tokio::test]
    async fn pointer_move_is_dropped_when_the_queue_is_full() {
        let (tx, mut rx) = mpsc::channel(WRITER_QUEUE_CAPACITY);
        let sink = test_sink(tx);

        for index in 0..WRITER_QUEUE_CAPACITY {
            sink.tx.try_send(vec![index as u8]).expect("queue has room");
        }

        let started = Instant::now();
        sink.send(pointer(PointerAction::Move))
            .await
            .expect("a dropped move is not an error");
        let elapsed = started.elapsed();

        assert!(
            elapsed < SEND_TIMEOUT,
            "a move must not wait on a full queue (waited {elapsed:?})"
        );

        // Nothing was enqueued: the queue still holds exactly its capacity.
        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        assert_eq!(drained, WRITER_QUEUE_CAPACITY);
    }

    #[tokio::test]
    async fn other_messages_wait_a_bounded_time_and_then_fail_on_a_full_queue() {
        let (tx, mut rx) = mpsc::channel(WRITER_QUEUE_CAPACITY);
        let sink = test_sink(tx);

        for index in 0..WRITER_QUEUE_CAPACITY {
            sink.tx.try_send(vec![index as u8]).expect("queue has room");
        }

        let started = Instant::now();
        let error = sink
            .send(InputMessage::Rotate)
            .await
            .expect_err("a full queue must not block a command forever");
        let elapsed = started.elapsed();

        assert!(matches!(error, InputError::Failed(_)), "{error:?}");
        assert!(
            elapsed >= SEND_TIMEOUT,
            "the wait must be the full budget before failing (waited {elapsed:?})"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "the wait must stay bounded (waited {elapsed:?})"
        );

        let mut drained = 0;
        while rx.try_recv().is_ok() {
            drained += 1;
        }
        assert_eq!(drained, WRITER_QUEUE_CAPACITY);
    }

    #[tokio::test]
    async fn a_closed_channel_fails_the_send() {
        let (tx, rx) = mpsc::channel(WRITER_QUEUE_CAPACITY);
        drop(rx);
        let sink = test_sink(tx);

        let error = sink
            .send(InputMessage::Rotate)
            .await
            .expect_err("a closed channel must fail");

        assert!(matches!(error, InputError::Failed(_)), "{error:?}");
    }

    /// Writer that fails on the first write, like a socket whose peer is gone.
    struct BrokenWriter;

    impl AsyncWrite for BrokenWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "peer is gone",
            )))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn a_failed_write_ends_the_writer_and_closes_the_channel() {
        let (tx, rx) = mpsc::channel(4);
        let sink = test_sink(tx);
        let writer = tokio::spawn(write_loop(BrokenWriter, rx));

        // Queue one message so the writer reaches the failing write.
        sink.send(InputMessage::Rotate)
            .await
            .expect("the queue accepts one message");

        timeout(READ_TIMEOUT, writer)
            .await
            .expect("writer stops after the write error")
            .expect("writer task does not panic");

        let error = sink
            .send(InputMessage::Rotate)
            .await
            .expect_err("a stopped writer must fail later sends");
        assert!(matches!(error, InputError::Failed(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_closed_socket_eventually_fails_the_send() {
        let (_read_half, write_half, server) = connected_pair().await;
        let (sink, writer) = spawn_writer(write_half);

        // The peer disappears without reading: the socket resets after the
        // first write, so the writer must stop and the channel must close.
        drop(server);

        let mut failed = None;
        for _ in 0..64 {
            match sink.send(InputMessage::Text("x".to_owned())).await {
                Ok(()) => tokio::task::yield_now().await,
                Err(error) => {
                    failed = Some(error);
                    break;
                }
            }
        }

        let error = failed.expect("the writer must report the broken socket");
        assert!(matches!(error, InputError::Failed(_)), "{error:?}");

        // The writer already stopped; joining must be immediate.
        timeout(READ_TIMEOUT, writer)
            .await
            .expect("writer joins")
            .expect("writer task does not panic");
        drop(_read_half);
    }

    #[tokio::test]
    async fn empty_text_writes_nothing() {
        let (_read_half, write_half, mut server) = connected_pair().await;
        let (sink, writer) = spawn_writer(write_half);

        sink.send(InputMessage::Text(String::new()))
            .await
            .expect("empty text is a no-op");
        sink.send(InputMessage::Rotate)
            .await
            .expect("rotate is queued");

        // Only the rotate byte arrives: the empty text produced no message.
        let mut received = [0u8; 1];
        timeout(READ_TIMEOUT, server.read_exact(&mut received))
            .await
            .expect("read before the timeout")
            .expect("read the rotate byte");
        assert_eq!(received, [0x0Bu8]);

        drop(sink);
        timeout(READ_TIMEOUT, writer)
            .await
            .expect("writer joins")
            .expect("writer task does not panic");
    }
}
