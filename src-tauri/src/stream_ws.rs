//! Local WebSocket server that replays a [`FrameSource`].
//!
//! Scope of M0b: bind **only** to `127.0.0.1:0`, protect the endpoint with a
//! random per-session token and serve exactly one client at a time. The wire
//! format is the internal protocol of `docs/stream-protocol.md`: a JSON `hello`
//! text frame, then binary frames with a 16-byte little-endian header.
//!
//! The actor only pulls messages from the source while a client is connected, so
//! waiting for a client never consumes the fixture. When the client disconnects
//! the actor goes back to `accept` and keeps the source position; a reconnecting
//! client gets `hello` plus the last configuration so the stream stays
//! decodable.
//!
//! Two sources are supported: a replay fixture ([`ReplayServer::bind`], M0) and
//! a live session ([`ReplayServer::bind_live`], M1). The live variant closes the
//! client socket when the session ends and releases the session exactly once;
//! replay keeps the client connected after the fixture ends, as M0 requires.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use lotu_app::ports::{FrameSource, SessionStream, StreamMessage};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{accept_hdr_async, WebSocketStream};

/// Path of the internal stream endpoint.
pub const STREAM_PATH: &str = "/stream";

/// Number of random bytes in a session token (sent as lowercase hex).
pub const TOKEN_BYTES: usize = 16;

/// Size of the binary message header (type, flags, reserved, PTS, size).
pub const MESSAGE_HEADER_LEN: usize = 16;

const CONFIG_TYPE: u8 = 1;
const FRAME_TYPE: u8 = 2;
const KEYFRAME_FLAG: u8 = 0b0000_0001;
const CONFIG_PAYLOAD_HEADER_LEN: usize = 8;

type WsStream = WebSocketStream<TcpStream>;
type WsSink = SplitSink<WsStream, Message>;
type WsIncoming = SplitStream<WsStream>;
type AcceptFuture = Pin<Box<dyn Future<Output = WsStream> + Send>>;
type SourceFuture = Pin<Box<dyn Future<Output = Option<StreamMessage>> + Send>>;

/// Source served by a [`ReplayServer`].
enum ServedSource {
    /// M0 replay fixture: no cleanup, position preserved across reconnections.
    Replay(Box<dyn FrameSource>),
    /// M1 live session: end of stream closes the client and releases the
    /// session (sockets, process, forward, jar).
    Live(Box<dyn SessionStream>),
}

impl ServedSource {
    async fn next_message(&mut self) -> Option<StreamMessage> {
        match self {
            Self::Replay(source) => source.next_message().await,
            Self::Live(stream) => stream.next_message().await,
        }
    }

    fn is_live(&self) -> bool {
        matches!(self, Self::Live(_))
    }

    /// Runs the deterministic session cleanup; replay has nothing to release.
    async fn close(self) {
        if let Self::Live(stream) = self {
            stream.close().await;
        }
    }
}

/// Errors raised while starting the replay WebSocket server.
#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("cannot bind the replay WebSocket server: {0}")]
    Bind(#[source] std::io::Error),
    #[error("stream payload of {0} bytes exceeds the u32 header limit")]
    MessageTooLarge(usize),
}

/// Bound replay server, ready to run.
///
/// [`ReplayServer::bind`] only opens the loopback listener; the fixture is not
/// consumed until a client connects. Call [`ReplayServer::run`] (or spawn it)
/// to start serving.
pub struct ReplayServer {
    listener: TcpListener,
    address: SocketAddr,
    hello: String,
    token: String,
    source: ServedSource,
}

impl ReplayServer {
    /// Binds an ephemeral loopback port and generates the session token.
    ///
    /// The source is a replay fixture: reaching its end keeps the client
    /// connected and does not release anything (M0 behavior).
    pub async fn bind(
        hello: impl Into<String>,
        source: Box<dyn FrameSource>,
    ) -> Result<Self, StreamError> {
        Self::bind_source(hello, ServedSource::Replay(source)).await
    }

    /// Binds an ephemeral loopback port for a live session.
    ///
    /// Differences from [`bind`](Self::bind): the end of the stream closes the
    /// client socket (so the UI does not keep a frozen canvas) and runs
    /// [`SessionStream::close`] exactly once.
    pub async fn bind_live(
        hello: impl Into<String>,
        stream: Box<dyn SessionStream>,
    ) -> Result<Self, StreamError> {
        Self::bind_source(hello, ServedSource::Live(stream)).await
    }

    async fn bind_source(
        hello: impl Into<String>,
        source: ServedSource,
    ) -> Result<Self, StreamError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(StreamError::Bind)?;
        let address = listener.local_addr().map_err(StreamError::Bind)?;

        Ok(Self {
            listener,
            address,
            hello: hello.into(),
            token: random_token(),
            source,
        })
    }

    /// Endpoint handed to the frontend, token included.
    pub fn ws_url(&self) -> String {
        format!("ws://{}{}?token={}", self.address, STREAM_PATH, self.token)
    }

    /// Token expected in the `token` query parameter.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Serves clients until the task is aborted.
    pub async fn run(self) {
        let ReplayServer {
            listener,
            address: _,
            hello,
            token,
            source,
        } = self;

        let listener = Arc::new(listener);
        let live = source.is_live();
        // The source lives behind a shared mutex so the pending read future can
        // be `'static` and kept across `select!` iterations: a `next_message`
        // future buffers partial reads inside itself, so recreating it after
        // another branch wins would drop video bytes and desynchronize the
        // parser. The mutex is only taken by that single future.
        let source = Arc::new(tokio::sync::Mutex::new(source));
        let mut client: Option<Client> = None;
        let mut pending_accept: Option<AcceptFuture> = None;
        let mut pending_message: Option<SourceFuture> = None;
        let mut last_config: Option<Vec<u8>> = None;
        let mut awaiting_keyframe = false;
        let mut source_done = false;

        loop {
            let event = match client.as_mut() {
                Some(client) => {
                    let accept = accept_future(&mut pending_accept, &listener, &token);
                    if pending_message.is_none() {
                        let source = Arc::clone(&source);
                        pending_message = Some(Box::pin(async move {
                            let mut source = source.lock().await;
                            source.next_message().await
                        }));
                    }
                    let message = pending_message
                        .as_mut()
                        .expect("the source future was just installed");
                    tokio::select! {
                        biased;
                        accepted = accept => Event::Accepted(Box::new(accepted)),
                        message = message, if !source_done => {
                            Event::Source(message)
                        }
                        incoming = client.incoming.next() => Event::Incoming(incoming),
                    }
                }
                None => {
                    let accept = accept_future(&mut pending_accept, &listener, &token);
                    Event::Accepted(Box::new(accept.await))
                }
            };

            if matches!(event, Event::Accepted(_)) {
                pending_accept = None;
            }
            if matches!(event, Event::Source(_)) {
                pending_message = None;
            }

            match event {
                Event::Accepted(stream) => {
                    // A new authorized connection replaces the previous client.
                    let (mut sink, incoming) = (*stream).split();
                    if sink.send(Message::text(hello.clone())).await.is_err() {
                        continue;
                    }
                    // Reconnecting mid-stream needs the last configuration to
                    // rebuild the decoder; the source position is preserved.
                    if let Some(config) = &last_config {
                        if sink.send(Message::binary(config.clone())).await.is_err() {
                            continue;
                        }
                    }
                    // Live reconnect: the decoder has no reference frames, so
                    // delivery resumes at the next keyframe (`docs/m1-contract.md`
                    // 4.1, D6). The first connection always starts at the config.
                    awaiting_keyframe = live && last_config.is_some();
                    client = Some(Client { sink, incoming });
                }
                Event::Incoming(None | Some(Err(_))) => client = None,
                Event::Incoming(Some(Ok(Message::Close(_)))) => client = None,
                Event::Incoming(Some(Ok(_))) => {}
                Event::Source(None) => {
                    if live {
                        // The device disconnected or the server died: the
                        // client must observe the close, not a frozen canvas.
                        break;
                    }
                    source_done = true;
                }
                Event::Source(Some(message)) => {
                    if awaiting_keyframe {
                        match &message {
                            // A new encoder configuration arrives before its
                            // first IDR; keep waiting for that keyframe.
                            StreamMessage::Config { .. } => {}
                            StreamMessage::Frame { keyframe: true, .. } => {
                                awaiting_keyframe = false
                            }
                            StreamMessage::Frame {
                                keyframe: false, ..
                            } => continue,
                        }
                    }
                    let is_config = matches!(&message, StreamMessage::Config { .. });
                    let Ok(bytes) = encode_message(&message) else {
                        // A message that cannot be framed can never be
                        // delivered; the stream is unusable from here on.
                        source_done = true;
                        continue;
                    };
                    let Some(active) = client.as_mut() else {
                        continue;
                    };
                    if active
                        .sink
                        .send(Message::binary(bytes.clone()))
                        .await
                        .is_err()
                    {
                        client = None;
                        continue;
                    }
                    if is_config {
                        last_config = Some(bytes);
                    }
                }
            }
        }

        // Live end of stream: close the client socket before releasing the
        // session, so the UI sees the stream end and the resources are freed
        // exactly once (`docs/m1-contract.md` 4.1 and 4.5).
        if let Some(client) = client.take() {
            let mut sink = client.sink;
            let _ = sink.send(Message::Close(None)).await;
            let _ = sink.close().await;
        }
        // Release the pending read future so the source is uniquely owned, then
        // run the deterministic cleanup (a no-op for replay).
        drop(pending_message);
        match Arc::try_unwrap(source) {
            Ok(source) => source.into_inner().close().await,
            Err(_) => eprintln!("a pending stream read survived the actor; cleanup skipped"),
        }
    }
}

struct Client {
    sink: WsSink,
    incoming: WsIncoming,
}

enum Event {
    /// Boxed because the TLS-agnostic WebSocket stream is much larger than the
    /// other variants and the event lives across `select!` points.
    Accepted(Box<WsStream>),
    Incoming(Option<Result<Message, WsError>>),
    Source(Option<StreamMessage>),
}

fn accept_future<'a>(
    slot: &'a mut Option<AcceptFuture>,
    listener: &Arc<TcpListener>,
    token: &str,
) -> &'a mut AcceptFuture {
    slot.get_or_insert_with(|| {
        let listener = Arc::clone(listener);
        let token = token.to_owned();
        Box::pin(async move { accept_authorized(&listener, &token).await })
    })
}

/// Accepts connections until one passes path and token validation.
///
/// Invalid handshakes are rejected with `401 Unauthorized` and the loop keeps
/// waiting; a rejected connection never disturbs the current client.
async fn accept_authorized(listener: &TcpListener, token: &str) -> WsStream {
    loop {
        let tcp = match listener.accept().await {
            Ok((tcp, _peer)) => tcp,
            Err(_) => {
                // The listener is alive for the whole actor; back off briefly
                // instead of spinning on a transient accept error.
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                continue;
            }
        };

        if let Some(stream) = handshake(tcp, token).await {
            return stream;
        }
    }
}

async fn handshake(tcp: TcpStream, token: &str) -> Option<WsStream> {
    let expected = token.to_owned();
    // The callback signature is fixed by `accept_hdr_async`; the rejection type
    // (`ErrorResponse`) cannot be boxed.
    #[allow(clippy::result_large_err)]
    let callback = move |request: &Request, response: Response| {
        if is_authorized(request, &expected) {
            Ok(response)
        } else {
            let mut rejection: ErrorResponse = ErrorResponse::new(None);
            *rejection.status_mut() = StatusCode::UNAUTHORIZED;
            Err(rejection)
        }
    };

    accept_hdr_async(tcp, callback).await.ok()
}

fn is_authorized(request: &Request, token: &str) -> bool {
    let uri = request.uri();
    if uri.path() != STREAM_PATH {
        return false;
    }

    let Some(query) = uri.query() else {
        return false;
    };
    query
        .split('&')
        .any(|pair| pair.split_once('=') == Some(("token", token)))
}

fn random_token() -> String {
    let bytes: [u8; TOKEN_BYTES] = rand::random();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 0x0f) as usize] as char);
    }
    token
}

/// Serializes one [`StreamMessage`] into the internal WebSocket wire format.
///
/// Header (little-endian): type, flags, reserved (`u16`), PTS (`i64`), payload
/// size (`u32`). Configuration messages carry `u32 width`, `u32 height` and the
/// complete `avcC` as payload; frames carry the AVCC access unit.
pub fn encode_message(message: &StreamMessage) -> Result<Vec<u8>, StreamError> {
    match message {
        StreamMessage::Config {
            width,
            height,
            avcc,
        } => {
            let payload_len = CONFIG_PAYLOAD_HEADER_LEN
                .checked_add(avcc.len())
                .ok_or(StreamError::MessageTooLarge(usize::MAX))?;
            let mut out = Vec::with_capacity(MESSAGE_HEADER_LEN + payload_len);
            push_header(&mut out, CONFIG_TYPE, 0, 0, payload_len)?;
            out.extend_from_slice(&width.to_le_bytes());
            out.extend_from_slice(&height.to_le_bytes());
            out.extend_from_slice(avcc);
            Ok(out)
        }
        StreamMessage::Frame {
            pts_us,
            keyframe,
            data,
        } => {
            let flags = if *keyframe { KEYFRAME_FLAG } else { 0 };
            let mut out = Vec::with_capacity(MESSAGE_HEADER_LEN + data.len());
            push_header(&mut out, FRAME_TYPE, flags, *pts_us, data.len())?;
            out.extend_from_slice(data);
            Ok(out)
        }
    }
}

fn push_header(
    out: &mut Vec<u8>,
    kind: u8,
    flags: u8,
    pts_us: i64,
    payload_len: usize,
) -> Result<(), StreamError> {
    let payload_len =
        u32::try_from(payload_len).map_err(|_| StreamError::MessageTooLarge(payload_len))?;
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&pts_us.to_le_bytes());
    out.extend_from_slice(&payload_len.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use futures_util::StreamExt;
    use tokio::sync::Semaphore;
    use tokio_tungstenite::{connect_async, MaybeTlsStream};

    use super::*;

    const HELLO: &str = r#"{"v":1,"type":"hello","codec":"h264"}"#;
    const READ_TIMEOUT: Duration = Duration::from_secs(2);

    fn avcc() -> Vec<u8> {
        vec![
            1, 0x42, 0xC0, 0x1F, 0xFF, 0xE1, 0, 4, 0x67, 0x42, 0xC0, 0x1F, 1, 0, 2, 0x68, 0xCE,
        ]
    }

    fn config() -> StreamMessage {
        StreamMessage::Config {
            width: 1080,
            height: 2400,
            avcc: avcc(),
        }
    }

    fn key_frame(pts_us: i64) -> StreamMessage {
        StreamMessage::Frame {
            pts_us,
            keyframe: true,
            data: vec![0, 0, 0, 2, 0x65, 0x88],
        }
    }

    fn inter_frame(pts_us: i64) -> StreamMessage {
        StreamMessage::Frame {
            pts_us,
            keyframe: false,
            data: vec![0, 0, 0, 2, 0x41, 0x9A],
        }
    }

    /// Test source gated by permits: one permit is needed per message.
    struct GatedSource {
        messages: VecDeque<StreamMessage>,
        permits: Arc<Semaphore>,
        pulled: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl FrameSource for GatedSource {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            // The permit is consumed, not recycled: a released permit would let
            // the actor pull every message on a single budget unit and the
            // "position is preserved" assertions would race with delivery.
            let permit = self.permits.acquire().await.ok()?;
            permit.forget();
            self.pulled.fetch_add(1, Ordering::SeqCst);
            self.messages.pop_front()
        }
    }

    async fn test_server(
        messages: Vec<StreamMessage>,
        initial_permits: usize,
    ) -> (ReplayServer, Arc<Semaphore>, Arc<AtomicUsize>) {
        let permits = Arc::new(Semaphore::new(initial_permits));
        let pulled = Arc::new(AtomicUsize::new(0));
        let source = GatedSource {
            messages: messages.into(),
            permits: Arc::clone(&permits),
            pulled: Arc::clone(&pulled),
        };

        let server = ReplayServer::bind(HELLO, Box::new(source))
            .await
            .expect("the replay server should bind");
        (server, permits, pulled)
    }

    async fn received(ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>) -> Message {
        tokio::time::timeout(READ_TIMEOUT, ws.next())
            .await
            .expect("a message should arrive before the timeout")
            .expect("the socket should stay open")
            .expect("the message should be valid")
    }

    fn assert_header(bytes: &[u8], kind: u8, flags: u8, pts_us: i64, payload_len: usize) {
        assert!(bytes.len() >= MESSAGE_HEADER_LEN);
        assert_eq!(bytes[0], kind);
        assert_eq!(bytes[1], flags);
        assert_eq!(&bytes[2..4], &[0, 0]);
        assert_eq!(i64::from_le_bytes(bytes[4..12].try_into().unwrap()), pts_us);
        assert_eq!(
            u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize,
            payload_len
        );
        assert_eq!(bytes.len(), MESSAGE_HEADER_LEN + payload_len);
    }

    #[test]
    fn encodes_config_message() {
        let bytes = encode_message(&config()).expect("config should encode");
        assert_header(
            &bytes,
            CONFIG_TYPE,
            0,
            0,
            CONFIG_PAYLOAD_HEADER_LEN + avcc().len(),
        );
        assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1080);
        assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 2400);
        assert_eq!(&bytes[24..], avcc().as_slice());
    }

    #[test]
    fn encodes_frame_message_type_flags_and_pts() {
        let bytes = encode_message(&key_frame(33_333)).expect("frame should encode");
        assert_header(&bytes, FRAME_TYPE, KEYFRAME_FLAG, 33_333, 6);
        assert_eq!(&bytes[MESSAGE_HEADER_LEN..], &[0, 0, 0, 2, 0x65, 0x88]);

        let bytes = encode_message(&inter_frame(66_666)).expect("frame should encode");
        assert_header(&bytes, FRAME_TYPE, 0, 66_666, 6);
        assert_eq!(&bytes[MESSAGE_HEADER_LEN..], &[0, 0, 0, 2, 0x41, 0x9A]);
    }

    #[test]
    fn token_is_lowercase_hex_of_the_expected_length() {
        let token = random_token();
        assert_eq!(token.len(), TOKEN_BYTES * 2);
        assert!(token
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[tokio::test]
    async fn ws_url_uses_loopback_and_stream_path() {
        let (server, _permits, _pulled) = test_server(Vec::new(), 0).await;
        let url = server.ws_url();
        assert!(url.starts_with("ws://127.0.0.1:"), "{url}");
        assert!(url.contains("/stream?token="), "{url}");
        assert!(url.ends_with(server.token()), "{url}");
    }

    #[tokio::test]
    async fn rejects_wrong_token_and_wrong_path() {
        let (server, _permits, _pulled) = test_server(vec![config()], 16).await;
        let address = server.address;
        let token = server.token().to_owned();
        let task = tokio::spawn(server.run());

        let rejected = [
            format!("ws://{address}/stream?token=0000"),
            format!("ws://{address}/stream"),
            format!("ws://{address}/other?token={token}"),
        ];
        for url in rejected {
            assert!(
                connect_async(url.as_str()).await.is_err(),
                "{url} must be rejected"
            );
        }

        let valid = format!("ws://{address}/stream?token={token}");
        let (mut ws, _response) = connect_async(valid.as_str())
            .await
            .expect("a valid path and token should be accepted");
        assert!(matches!(received(&mut ws).await, Message::Text(text) if text == HELLO));

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn streams_hello_config_and_frames_with_exact_headers() {
        let (server, _permits, _pulled) =
            test_server(vec![config(), key_frame(0), inter_frame(33_333)], 16).await;
        let url = server.ws_url();
        let task = tokio::spawn(server.run());

        let (mut ws, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");

        match received(&mut ws).await {
            Message::Text(text) => assert_eq!(text, HELLO),
            other => panic!("expected the hello text, got {other:?}"),
        }

        match received(&mut ws).await {
            Message::Binary(bytes) => {
                assert_header(
                    &bytes,
                    CONFIG_TYPE,
                    0,
                    0,
                    CONFIG_PAYLOAD_HEADER_LEN + avcc().len(),
                );
                assert_eq!(u32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1080);
                assert_eq!(u32::from_le_bytes(bytes[20..24].try_into().unwrap()), 2400);
                assert_eq!(&bytes[24..], avcc().as_slice());
            }
            other => panic!("expected the config message, got {other:?}"),
        }

        match received(&mut ws).await {
            Message::Binary(bytes) => {
                assert_header(&bytes, FRAME_TYPE, KEYFRAME_FLAG, 0, 6);
                assert_eq!(&bytes[MESSAGE_HEADER_LEN..], &[0, 0, 0, 2, 0x65, 0x88]);
            }
            other => panic!("expected the first frame, got {other:?}"),
        }

        match received(&mut ws).await {
            Message::Binary(bytes) => assert_header(&bytes, FRAME_TYPE, 0, 33_333, 6),
            other => panic!("expected the second frame, got {other:?}"),
        }

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn waits_for_a_client_without_consuming_the_source() {
        let (server, _permits, pulled) = test_server(vec![config(), key_frame(0)], 16).await;
        let url = server.ws_url();
        let task = tokio::spawn(server.run());

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            pulled.load(Ordering::SeqCst),
            0,
            "the source must not be pulled while no client is connected"
        );

        let (mut ws, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut ws).await, Message::Text(_)));
        assert!(matches!(received(&mut ws).await, Message::Binary(_)));
        assert!(pulled.load(Ordering::SeqCst) >= 1);

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn reconnect_resumes_position_and_resends_config() {
        let (server, permits, pulled) =
            test_server(vec![config(), key_frame(0), inter_frame(1_000)], 0).await;
        let url = server.ws_url();
        let task = tokio::spawn(server.run());

        let (mut first, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut first).await, Message::Text(_)));

        permits.add_permits(1);
        assert!(matches!(received(&mut first).await, Message::Binary(_)));
        permits.add_permits(1);
        assert!(matches!(received(&mut first).await, Message::Binary(_)));
        assert_eq!(pulled.load(Ordering::SeqCst), 2);

        // Disconnecting must not consume the remaining frame.
        drop(first);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(pulled.load(Ordering::SeqCst), 2);

        let (mut second, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut second).await, Message::Text(_)));

        // The reconnecting client gets the last configuration again.
        match received(&mut second).await {
            Message::Binary(bytes) => assert_header(
                &bytes,
                CONFIG_TYPE,
                0,
                0,
                CONFIG_PAYLOAD_HEADER_LEN + avcc().len(),
            ),
            other => panic!("a reconnecting client must get the config, got {other:?}"),
        }

        // The source position is preserved: the next frame is the second one.
        permits.add_permits(1);
        match received(&mut second).await {
            Message::Binary(bytes) => assert_header(&bytes, FRAME_TYPE, 0, 1_000, 6),
            other => panic!("the second frame should follow, got {other:?}"),
        }
        assert_eq!(pulled.load(Ordering::SeqCst), 3);

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn a_new_connection_invalidates_the_previous_client() {
        let (server, _permits, _pulled) = test_server(vec![config()], 16).await;
        let url = server.ws_url();
        let task = tokio::spawn(server.run());

        let (mut first, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut first).await, Message::Text(_)));

        let (mut second, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut second).await, Message::Text(_)));

        let closed = tokio::time::timeout(READ_TIMEOUT, async {
            loop {
                match first.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                }
            }
        })
        .await;
        assert!(closed.is_ok(), "the previous client should be disconnected");

        task.abort();
        let _ = task.await;
    }

    /// Live source double gated by permits: one permit per message, so the test
    /// controls when the actor sees the next value.
    struct GatedLiveSource {
        messages: VecDeque<StreamMessage>,
        permits: Arc<Semaphore>,
        closes: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl FrameSource for GatedLiveSource {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            let permit = self.permits.acquire().await.ok()?;
            permit.forget();
            self.messages.pop_front()
        }
    }

    #[async_trait]
    impl SessionStream for GatedLiveSource {
        async fn close(self: Box<Self>) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Live source double that always ends immediately (no permit gate).
    struct EndingLiveSource {
        message: Option<StreamMessage>,
        closes: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl FrameSource for EndingLiveSource {
        async fn next_message(&mut self) -> Option<StreamMessage> {
            self.message.take()
        }
    }

    #[async_trait]
    impl SessionStream for EndingLiveSource {
        async fn close(self: Box<Self>) {
            self.closes.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn live_end_of_stream_closes_the_client_and_releases_the_session() {
        let closes = Arc::new(AtomicUsize::new(0));
        let source = EndingLiveSource {
            message: Some(config()),
            closes: Arc::clone(&closes),
        };
        let server = ReplayServer::bind_live(HELLO, Box::new(source))
            .await
            .expect("the live server should bind");
        let url = server.ws_url();
        let task = tokio::spawn(server.run());

        let (mut ws, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut ws).await, Message::Text(text) if text == HELLO));

        // The config is delivered, then the source ends: the client must
        // observe the close instead of a frozen canvas.
        assert!(matches!(received(&mut ws).await, Message::Binary(_)));
        let closed = tokio::time::timeout(READ_TIMEOUT, async {
            loop {
                match ws.next().await {
                    None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => continue,
                }
            }
        })
        .await;
        assert!(closed.is_ok(), "the client must observe the close");

        tokio::time::timeout(READ_TIMEOUT, task)
            .await
            .expect("the actor must finish after the end of stream")
            .expect("the actor should not panic");
        assert_eq!(
            closes.load(Ordering::SeqCst),
            1,
            "close() must run exactly once"
        );
    }

    #[tokio::test]
    async fn live_reconnect_resumes_at_the_next_keyframe() {
        let closes = Arc::new(AtomicUsize::new(0));
        let permits = Arc::new(Semaphore::new(0));
        let source = GatedLiveSource {
            messages: VecDeque::from(vec![
                config(),
                key_frame(0),
                inter_frame(1_000),
                key_frame(2_000),
            ]),
            permits: Arc::clone(&permits),
            closes,
        };
        let server = ReplayServer::bind_live(HELLO, Box::new(source))
            .await
            .expect("the live server should bind");
        let url = server.ws_url();
        let task = tokio::spawn(server.run());

        let (mut first, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut first).await, Message::Text(_)));

        permits.add_permits(1);
        assert!(matches!(received(&mut first).await, Message::Binary(_)));
        permits.add_permits(1);
        match received(&mut first).await {
            Message::Binary(bytes) => assert_header(&bytes, FRAME_TYPE, KEYFRAME_FLAG, 0, 6),
            other => panic!("expected the keyframe, got {other:?}"),
        }

        // A new client replaces the old one and gets the cached configuration;
        // the P frame that follows must be dropped, so the next binary is the
        // keyframe at 2 ms.
        let (mut second, _response) = connect_async(url.as_str())
            .await
            .expect("the handshake should succeed");
        assert!(matches!(received(&mut second).await, Message::Text(_)));
        assert!(matches!(received(&mut second).await, Message::Binary(_)));

        permits.add_permits(1);
        permits.add_permits(1);
        match received(&mut second).await {
            Message::Binary(bytes) => assert_header(&bytes, FRAME_TYPE, KEYFRAME_FLAG, 2_000, 6),
            other => panic!("expected the next keyframe, got {other:?}"),
        }

        task.abort();
        let _ = task.await;
    }
}
