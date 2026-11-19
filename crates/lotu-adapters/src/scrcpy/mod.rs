//! scrcpy 5.0.1 adapter.
//!
//! Everything scrcpy-specific (ADB handshake, sockets, wire layout) stays inside
//! this module tree, as required by `AGENTS.md` and `docs/architecture.md`.
//! Callers only see [`lotu_app::ports::StreamMessage`] values.
//!
//! [`session`] holds the pure wire primitives and the live session lifecycle
//! ([`ScrcpyVideoSource`], [`ScrcpySessionStream`]); [`parser`] owns the video
//! stream layout; [`control`] is the pure encoder of control-socket messages.

pub mod control;
pub mod device_msg;
pub mod parser;
pub mod session;
pub mod writer;

pub use parser::{ScrcpyError, ScrcpyVideoParser};
pub use session::{ScrcpySessionStream, ScrcpyVideoSource};
pub use writer::{ScrcpyInputSink, SEND_TIMEOUT, WRITER_QUEUE_CAPACITY};
