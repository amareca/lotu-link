//! Live scrcpy session: wire primitives and lifecycle.
//!
//! The pure builders below render the exact values of `docs/m1-contract.md`
//! 3.1: the 31-bit session id, the abstract socket name, the `adb forward`
//! arguments and the `app_process` invocation of the pinned server. They stay
//! pure so the wire contract is testable without a device.
//!
//! On top of them, [`ScrcpyVideoSource`] runs the session establishment
//! (push, forward, server process, video/control sockets, handshake) and
//! [`ScrcpySessionStream`] owns the resulting resources until
//! `SessionStream::close` releases them. Every cleanup step is best effort and
//! bounded, and a dropped session never leaves the server process running.

use std::net::{Ipv4Addr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lotu_app::ports::{
    FrameSource, InputSink, LiveStream, SessionError, SessionStream, StreamMessage, VideoSource,
};
use lotu_domain::{Serial, VideoConfig};
use rand::Rng;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

use crate::adb::SystemAdb;
use crate::platform::require_scrcpy_server;
use crate::scrcpy::parser::ScrcpyVideoParser;
use crate::scrcpy::writer::{receive_device_messages, spawn_writer};

/// Exact server version. The server aborts when it does not match
/// `BuildConfig.VERSION_NAME`.
pub const SERVER_VERSION: &str = "5.0.1";
/// Main class executed through `app_process`.
pub const SERVER_CLASS: &str = "com.genymobile.scrcpy.Server";
/// Remote copy of the vendored jar. The name is ours so it never collides with
/// a user-installed scrcpy server; pushing overwrites leftovers of past
/// sessions (idempotent cleanup).
pub const SERVER_REMOTE_JAR: &str = "/data/local/tmp/lotu-link-server.jar";

/// Highest valid scid: scrcpy uses a 31-bit random id.
const SCID_MASK: u32 = 0x7FFF_FFFF;

/// Random 31-bit scid, the same range the scrcpy client generates.
pub fn random_scid() -> u32 {
    rand::rng().random::<u32>() & SCID_MASK
}

/// Formats the scid as the server expects it: eight lowercase hex digits
/// (`%08x`).
pub fn scid_hex(scid: u32) -> String {
    debug_assert!(scid <= SCID_MASK, "scid must fit in 31 bits");
    format!("{scid:08x}")
}

/// Abstract socket name used by the server (`DesktopConnection.getSocketName`).
pub fn socket_name(scid: u32) -> String {
    format!("scrcpy_{}", scid_hex(scid))
}

/// `tcp:<port>` spec of the local side of the forward.
pub fn forward_local_spec(port: u16) -> String {
    format!("tcp:{port}")
}

/// `localabstract:<socket>` spec of the remote side of the forward.
pub fn forward_remote_spec(scid: u32) -> String {
    format!("localabstract:{}", socket_name(scid))
}

/// Arguments of `adb forward tcp:<port> localabstract:scrcpy_<scid>`.
pub fn forward_args(port: u16, scid: u32) -> [String; 3] {
    [
        String::from("forward"),
        forward_local_spec(port),
        forward_remote_spec(scid),
    ]
}

/// Arguments of `adb forward --remove tcp:<port>`.
pub fn remove_forward_args(port: u16) -> [String; 3] {
    [
        String::from("forward"),
        String::from("--remove"),
        forward_local_spec(port),
    ]
}

/// Rendered forward command (diagnostics and contract tests).
pub fn forward_command_line(port: u16, scid: u32) -> String {
    format!("adb {}", forward_args(port, scid).join(" "))
}

/// Options of `com.genymobile.scrcpy.Server`, exactly as fixed by the contract.
///
/// `audio=false` and `tunnel_forward=true` are explicit because the server
/// defaults differ (`audio=true`, reverse tunnel). `clipboard_autosync=false`
/// keeps the control socket silent in M1. Values that the server defaults to
/// the contract's choice (`send_dummy_byte`, `send_device_meta`,
/// `send_stream_meta`, `send_frame_meta`, `cleanup`, `power_on`,
/// `downsize_on_error`, orientation unlocked) are not passed.
pub fn server_options(scid: u32) -> Vec<String> {
    vec![
        format!("scid={}", scid_hex(scid)),
        String::from("tunnel_forward=true"),
        String::from("video=true"),
        String::from("audio=false"),
        String::from("control=true"),
        String::from("video_codec=h264"),
        String::from("log_level=warn"),
        String::from("max_size=0"),
        String::from("max_fps=60"),
        String::from("video_bit_rate=8000000"),
        String::from("clipboard_autosync=false"),
    ]
}

/// Arguments that follow `adb shell` to launch the server:
/// `CLASSPATH=<jar> app_process / <class> <version> <options...>`.
pub fn app_process_args(scid: u32) -> Vec<String> {
    let mut args = vec![
        format!("CLASSPATH={SERVER_REMOTE_JAR}"),
        String::from("app_process"),
        String::from("/"),
        String::from(SERVER_CLASS),
        String::from(SERVER_VERSION),
    ];
    args.extend(server_options(scid));
    args
}

/// Total budget of `ScrcpyVideoSource::open` (`docs/m1-contract.md` 3.1).
pub const SESSION_OPEN_BUDGET: Duration = Duration::from_secs(15);
/// `adb forward` attempts, each with a fresh ephemeral port.
pub const FORWARD_ATTEMPTS: u32 = 3;
/// Video socket attempts while the server starts listening.
pub const VIDEO_CONNECT_ATTEMPTS: u32 = 50;
/// Delay between video socket attempts.
pub const VIDEO_RETRY_DELAY: Duration = Duration::from_millis(100);
/// Time the server gets to exit on its own after both sockets are closed.
pub const SERVER_EXIT_TIMEOUT: Duration = Duration::from_secs(1);
/// Time the server gets to die after the kill signal.
pub const SERVER_KILL_TIMEOUT: Duration = Duration::from_secs(1);
/// Budget of `adb forward --remove` during cleanup.
pub const FORWARD_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
/// Budget of `adb shell rm -f` during cleanup.
pub const SHELL_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Opener of live scrcpy sessions for one device.
///
/// Holds the ADB client and the vendored server jar. Everything else (scid,
/// forward port, sockets, process) is created and released internally.
#[derive(Debug)]
pub struct ScrcpyVideoSource {
    adb: Arc<SystemAdb>,
    server_jar: PathBuf,
}

impl ScrcpyVideoSource {
    pub fn new(adb: Arc<SystemAdb>, server_jar: PathBuf) -> Self {
        Self { adb, server_jar }
    }

    /// Full establishment sequence of `docs/m1-contract.md` 3.1.
    ///
    /// The 15 s budget is applied by [`VideoSource::open`], which owns the
    /// timeout; the guard built here releases every resource it has already
    /// acquired when this future is dropped or returns an error.
    async fn open_session(
        &self,
        serial: &Serial,
        _config: &VideoConfig,
    ) -> Result<LiveStream, SessionError> {
        // M1 pins the server options to the contract values (max_size=0,
        // max_fps=60, 8 Mbps), which match `VideoConfig::default()`; the port
        // already hands the configuration over for M2.
        let jar = require_scrcpy_server(&self.server_jar)
            .map_err(|error| SessionError::VendorMissing(error.to_string()))?;

        // 1. Push. Overwrites leftovers of previous sessions.
        self.adb.push(serial, jar, SERVER_REMOTE_JAR).await?;

        // 2. scid.
        let scid = random_scid();

        // 3. Forward, up to three attempts with a fresh port each time.
        let mut last_forward_error = None;
        let mut port = None;
        for _ in 0..FORWARD_ATTEMPTS {
            let candidate = ephemeral_port()?;
            match self
                .adb
                .forward(
                    serial,
                    &forward_local_spec(candidate),
                    &forward_remote_spec(scid),
                )
                .await
            {
                Ok(()) => {
                    port = Some(candidate);
                    break;
                }
                Err(error) => last_forward_error = Some(error),
            }
        }
        let Some(port) = port else {
            return Err(match last_forward_error {
                Some(error) => SessionError::Adb(error),
                None => SessionError::Failed("could not allocate a forward port".to_owned()),
            });
        };

        let mut resources =
            SessionResources::new(Arc::clone(&self.adb), serial.clone(), Some(port));

        // 4. Server process.
        let child = match self.adb.spawn_server(serial, &app_process_args(scid)) {
            Ok(child) => child,
            Err(error) => {
                resources.cleanup().await;
                return Err(SessionError::Adb(error));
            }
        };
        resources.child = Some(child);

        let address = (Ipv4Addr::LOCALHOST, port);

        // 5. Video socket plus dummy byte.
        let video = match connect_video(address).await {
            Ok(video) => video,
            Err(error) => {
                resources.cleanup().await;
                return Err(error);
            }
        };

        // 6. Control socket: read half is drained, write half feeds the input
        // sink through one serialized writer task (M2).
        let control = match connect_control(address).await {
            Ok(control) => control,
            Err(error) => {
                // Close the video socket first so the server can notice and
                // exit on its own once the forward is gone.
                drop(video);
                resources.cleanup().await;
                return Err(error);
            }
        };
        let (read_half, write_half) = control.into_split();
        let (input, writer) = spawn_writer(write_half);
        resources.writer = Some(writer);
        resources.drain = Some(tokio::spawn(receive_device_messages(
            read_half,
            input.clipboard_sender(),
        )));

        // 7. Remaining handshake (device name, codec id, initial session meta).
        let parser = match ScrcpyVideoParser::connect_after_dummy(video).await {
            Ok(parser) => parser,
            Err(error) => {
                resources.cleanup().await;
                return Err(SessionError::Failed(error.to_string()));
            }
        };

        // 8. The tunnel is no longer needed once both sockets are connected.
        // A failure here is not fatal: the session works and cleanup retries.
        let _ = timeout(
            FORWARD_CLEANUP_TIMEOUT,
            self.adb.remove_forward(serial, &forward_local_spec(port)),
        )
        .await;
        resources.port = None;

        let device_name = parser.device_name().to_owned();
        Ok(LiveStream {
            device_name,
            stream: Box::new(ScrcpySessionStream {
                parser: Some(parser),
                resources,
            }),
            input: Arc::new(input) as Arc<dyn InputSink>,
        })
    }
}

#[async_trait]
impl VideoSource for ScrcpyVideoSource {
    async fn open(
        &self,
        serial: &Serial,
        config: &VideoConfig,
    ) -> Result<LiveStream, SessionError> {
        match timeout(SESSION_OPEN_BUDGET, self.open_session(serial, config)).await {
            Ok(result) => result,
            // Dropping the future runs `SessionResources::drop`, which kills
            // the server and schedules the best-effort remote cleanup.
            Err(_) => Err(SessionError::Timeout),
        }
    }
}

/// Live stream backed by the scrcpy video socket.
///
/// `close` is the only path that guarantees an awaited cleanup; dropping the
/// stream still aborts the drain task and kills the server process, and the
/// guard schedules the remote cleanup on a best-effort basis.
pub struct ScrcpySessionStream {
    parser: Option<ScrcpyVideoParser<TcpStream>>,
    resources: SessionResources,
}

#[async_trait]
impl FrameSource for ScrcpySessionStream {
    async fn next_message(&mut self) -> Option<StreamMessage> {
        let message = match self.parser.as_mut() {
            Some(parser) => parser.next_message().await,
            None => return None,
        };

        match message {
            Ok(message) => message,
            Err(_) => {
                // Protocol or I/O failure: the stream cannot recover, so close
                // the video socket and let `close` release the rest.
                self.parser = None;
                None
            }
        }
    }
}

#[async_trait]
impl SessionStream for ScrcpySessionStream {
    async fn close(self: Box<Self>) {
        let mut session = *self;
        session.parser = None;
        session.resources.cleanup().await;
    }
}

/// Resources acquired while establishing a session, released either by
/// [`cleanup`](SessionResources::cleanup) or, on cancellation (timeout), by
/// [`Drop`].
struct SessionResources {
    adb: Arc<SystemAdb>,
    serial: Serial,
    port: Option<u16>,
    child: Option<Child>,
    drain: Option<JoinHandle<()>>,
    writer: Option<JoinHandle<()>>,
}

impl SessionResources {
    fn new(adb: Arc<SystemAdb>, serial: Serial, port: Option<u16>) -> Self {
        Self {
            adb,
            serial,
            port,
            child: None,
            drain: None,
            writer: None,
        }
    }

    /// Bounded, best-effort release of every remote and local resource.
    async fn cleanup(&mut self) {
        // Close the local sockets first: the server notices the broken pipes
        // and exits on its own, which is cheaper than a kill.
        if let Some(handle) = self.writer.take() {
            handle.abort();
            let _ = handle.await;
        }

        if let Some(handle) = self.drain.take() {
            handle.abort();
            let _ = handle.await;
        }

        if let Some(child) = self.child.take() {
            self.wait_or_kill(child).await;
        }

        if let Some(port) = self.port.take() {
            let _ = timeout(
                FORWARD_CLEANUP_TIMEOUT,
                self.adb
                    .remove_forward(&self.serial, &forward_local_spec(port)),
            )
            .await;
        }

        let _ = timeout(
            SHELL_CLEANUP_TIMEOUT,
            self.adb
                .shell(&self.serial, &["rm", "-f", SERVER_REMOTE_JAR]),
        )
        .await;
    }

    /// Waits up to [`SERVER_EXIT_TIMEOUT`] for a graceful exit, then kills.
    async fn wait_or_kill(&self, mut child: Child) {
        if matches!(timeout(SERVER_EXIT_TIMEOUT, child.wait()).await, Ok(Ok(_))) {
            return;
        }

        let _ = timeout(SERVER_KILL_TIMEOUT, child.kill()).await;
    }
}

impl Drop for SessionResources {
    fn drop(&mut self) {
        if let Some(handle) = self.writer.take() {
            handle.abort();
        }
        if let Some(handle) = self.drain.take() {
            handle.abort();
        }

        let had_child = self.child.is_some();
        if let Some(mut child) = self.child.take() {
            // `kill_on_drop` already covers the process; the explicit signal
            // makes the child die even if the runtime is gone.
            let _ = child.start_kill();
        }

        let port = self.port.take();
        if port.is_none() && !had_child {
            return;
        }

        // Cancellation (for example the 15 s budget) drops this guard outside
        // an async context we can await in. Schedule the remote steps instead;
        // they are best effort, as required by 4.5.
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let adb = Arc::clone(&self.adb);
        let serial = self.serial.clone();
        runtime.spawn(async move {
            if let Some(port) = port {
                let _ = timeout(
                    FORWARD_CLEANUP_TIMEOUT,
                    adb.remove_forward(&serial, &forward_local_spec(port)),
                )
                .await;
            }
            let _ = timeout(
                SHELL_CLEANUP_TIMEOUT,
                adb.shell(&serial, &["rm", "-f", SERVER_REMOTE_JAR]),
            )
            .await;
        });
    }
}

/// Picks a free local TCP port by binding `127.0.0.1:0` and releasing it.
///
/// The race between releasing the port and ADB binding it is accepted by the
/// contract: a failure retries with a new port.
fn ephemeral_port() -> Result<u16, SessionError> {
    let listener = StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| {
        SessionError::Failed(format!("could not bind an ephemeral port: {error}"))
    })?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| SessionError::Failed(format!("could not read the local port: {error}")))
}

/// Connects the video socket and consumes the dummy byte, retrying while the
/// server starts listening (50 × 100 ms).
async fn connect_video(address: (Ipv4Addr, u16)) -> Result<TcpStream, SessionError> {
    let mut last_error = String::from("the server did not answer");

    for attempt in 0..VIDEO_CONNECT_ATTEMPTS {
        if attempt > 0 {
            sleep(VIDEO_RETRY_DELAY).await;
        }

        match TcpStream::connect(address).await {
            Ok(mut stream) => match read_dummy_byte(&mut stream).await {
                Ok(()) => return Ok(stream),
                Err(error) => last_error = error,
            },
            Err(error) => last_error = error.to_string(),
        }
    }

    Err(SessionError::Failed(format!(
        "could not connect the video socket: {last_error}"
    )))
}

/// Reads the single `0x00` byte the server writes on the first socket.
async fn read_dummy_byte(stream: &mut TcpStream) -> Result<(), String> {
    let mut dummy = [0u8; 1];
    match stream.read_exact(&mut dummy).await {
        Ok(_) if dummy[0] == 0x00 => Ok(()),
        Ok(_) => Err(format!("unexpected dummy byte 0x{:02X}", dummy[0])),
        Err(error) => Err(error.to_string()),
    }
}

/// Connects the control socket and enables `TCP_NODELAY`.
///
/// The server sends nothing on this socket in M1 (`clipboard_autosync=false`),
/// but it requires the connection; the caller drains it.
async fn connect_control(address: (Ipv4Addr, u16)) -> Result<TcpStream, SessionError> {
    let stream = TcpStream::connect(address).await.map_err(|error| {
        SessionError::Failed(format!("could not connect the control socket: {error}"))
    })?;
    stream
        .set_nodelay(true)
        .map_err(|error| SessionError::Failed(format!("could not set TCP_NODELAY: {error}")))?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCID: u32 = 0x00ab_cdef;

    #[test]
    fn scid_is_31_bits_and_formats_as_eight_hex_digits() {
        for _ in 0..64 {
            let scid = random_scid();
            assert!(scid <= SCID_MASK, "scid {scid:#010x} exceeds 31 bits");

            let hex = scid_hex(scid);
            assert_eq!(hex.len(), 8, "{hex}");
            assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(hex, hex.to_lowercase());
        }

        assert_eq!(scid_hex(SCID), "00abcdef");
        assert_eq!(scid_hex(0), "00000000");
    }

    #[test]
    fn socket_name_uses_the_formatted_scid() {
        assert_eq!(socket_name(SCID), "scrcpy_00abcdef");
    }

    #[test]
    fn forward_command_matches_the_contract() {
        assert_eq!(
            forward_command_line(27183, SCID),
            "adb forward tcp:27183 localabstract:scrcpy_00abcdef"
        );
        assert_eq!(
            forward_args(27183, SCID),
            ["forward", "tcp:27183", "localabstract:scrcpy_00abcdef"]
        );
        assert_eq!(
            remove_forward_args(27183),
            ["forward", "--remove", "tcp:27183"]
        );
    }

    #[test]
    fn server_options_match_the_contract() {
        assert_eq!(
            server_options(SCID),
            [
                "scid=00abcdef",
                "tunnel_forward=true",
                "video=true",
                "audio=false",
                "control=true",
                "video_codec=h264",
                "log_level=warn",
                "max_size=0",
                "max_fps=60",
                "video_bit_rate=8000000",
                "clipboard_autosync=false",
            ]
        );
    }

    #[test]
    fn app_process_args_match_the_contract() {
        assert_eq!(
            app_process_args(SCID),
            [
                "CLASSPATH=/data/local/tmp/lotu-link-server.jar",
                "app_process",
                "/",
                "com.genymobile.scrcpy.Server",
                "5.0.1",
                "scid=00abcdef",
                "tunnel_forward=true",
                "video=true",
                "audio=false",
                "control=true",
                "video_codec=h264",
                "log_level=warn",
                "max_size=0",
                "max_fps=60",
                "video_bit_rate=8000000",
                "clipboard_autosync=false",
            ]
        );
    }

    #[test]
    fn lifecycle_budgets_follow_the_contract() {
        assert_eq!(SESSION_OPEN_BUDGET, Duration::from_secs(15));
        assert_eq!(FORWARD_ATTEMPTS, 3);
        assert_eq!(VIDEO_CONNECT_ATTEMPTS, 50);
        assert_eq!(VIDEO_RETRY_DELAY, Duration::from_millis(100));
        assert_eq!(SERVER_EXIT_TIMEOUT, Duration::from_secs(1));
        assert_eq!(SERVER_KILL_TIMEOUT, Duration::from_secs(1));
        assert_eq!(FORWARD_CLEANUP_TIMEOUT, Duration::from_secs(2));
        assert_eq!(SHELL_CLEANUP_TIMEOUT, Duration::from_secs(2));
    }

    #[test]
    fn ephemeral_port_is_selected() {
        let port = ephemeral_port().expect("an ephemeral port is available");
        assert_ne!(port, 0);
    }

    #[tokio::test]
    async fn missing_server_jar_fails_before_touching_adb() {
        let adb = Arc::new(SystemAdb::new(
            PathBuf::from("lotu-adb-does-not-exist-zzz"),
            5038,
        ));
        let jar = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("missing-scrcpy-server.jar");
        let source = ScrcpyVideoSource::new(adb, jar);
        let serial = Serial::new("R58M12345").expect("valid serial");

        let error = match source.open(&serial, &VideoConfig::default()).await {
            Ok(_) => panic!("the session must not start without the jar"),
            Err(error) => error,
        };

        assert!(matches!(error, SessionError::VendorMissing(_)), "{error:?}");
    }

    const VIDEO_CODEC_H264: u32 = 0x6832_3634;

    /// Device name, codec id and the initial session meta, as the server
    /// writes them after the dummy byte.
    fn handshake_bytes() -> Vec<u8> {
        let mut bytes = vec![0u8; 64];
        bytes[..6].copy_from_slice(b"Device");
        bytes.extend_from_slice(&VIDEO_CODEC_H264.to_be_bytes());
        bytes.extend_from_slice(&(1u32 << 31).to_be_bytes());
        bytes.extend_from_slice(&1080u32.to_be_bytes());
        bytes.extend_from_slice(&2400u32.to_be_bytes());
        bytes
    }

    /// Runs a local TCP server that writes `payload` and closes, then performs
    /// the post-dummy handshake against it.
    async fn parser_over_socket(payload: Vec<u8>) -> ScrcpyVideoParser<TcpStream> {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("listener binds");
        let address = listener.local_addr().expect("local address");

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            socket.write_all(&payload).await.expect("write payload");
            socket.shutdown().await.expect("shutdown");
        });

        let client = TcpStream::connect(address).await.expect("connect");
        ScrcpyVideoParser::connect_after_dummy(client)
            .await
            .expect("handshake")
    }

    fn session_stream(parser: ScrcpyVideoParser<TcpStream>) -> ScrcpySessionStream {
        let adb = Arc::new(SystemAdb::new(
            PathBuf::from("lotu-adb-does-not-exist-zzz"),
            5038,
        ));
        ScrcpySessionStream {
            parser: Some(parser),
            resources: SessionResources::new(
                adb,
                Serial::new("R58M12345").expect("valid serial"),
                None,
            ),
        }
    }

    #[tokio::test]
    async fn next_message_ends_with_none_at_a_clean_eof() {
        let parser = parser_over_socket(handshake_bytes()).await;
        let mut session = session_stream(parser);

        assert!(session.next_message().await.is_none());
        assert!(session.next_message().await.is_none());

        Box::new(session).close().await;
    }

    #[tokio::test]
    async fn next_message_turns_a_truncated_packet_into_none() {
        let mut bytes = handshake_bytes();
        bytes.extend_from_slice(&[0u8; 5]);

        let parser = parser_over_socket(bytes).await;
        let mut session = session_stream(parser);

        assert!(session.next_message().await.is_none());
        assert!(session.next_message().await.is_none());
    }
}
