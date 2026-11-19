//! Byte-level parser for the scrcpy 5.0.1 video socket.
//!
//! The wire layout is normative in `docs/m1-contract.md` §3.2 and was verified
//! against the `v5.0.1` tag of the scrcpy source. It differs from scrcpy 2.x/3.x
//! in two ways that matter here: the config/keyframe flags live in bits 62 and
//! 61, and each encoder (re)start is announced by a standalone 12-byte
//! *session meta* block that carries the encoder dimensions.
//!
//! ```text
//! 0x00                      dummy byte written by the server
//! 64 bytes                  device name, UTF-8, zero padded
//! u32 BE                    codec id (0x68323634 = "h264"; 0 and 1 are errors)
//! 12 bytes                  initial session meta: flags | width | height
//! 12 B header + payload     config packet (bit 62, SPS/PPS in Annex B)
//! 12 B header + payload     frame (bit 61 = keyframe, bits 60-0 = PTS in µs)
//! ```
//!
//! Framing rule after the codec id: every packet boundary starts with a 12-byte
//! block. If bit 7 of its first byte is set the block *is* the session meta
//! (there is no extra header, size field or payload). Otherwise the block is a
//! media header — `u64 BE pts_and_flags` plus `u32 BE payload size` — followed
//! by the payload.
//!
//! The parser emits the normalized stream contract: Annex B payloads are
//! converted to AVCC (`lengthSizeMinusOne = 3`), config packets become
//! [`StreamMessage::Config`] with an `avcC` record and the latest session
//! dimensions, and PTS values are rebased so the first frame is 0.

use lotu_app::ports::StreamMessage;
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::media::h264;

/// Byte the server writes first to prove it is listening behind the forward.
const DUMMY_BYTE: u8 = 0x00;
/// Fixed size of the device name field (`sendDeviceMeta`).
const DEVICE_NAME_LEN: usize = 64;
/// Codec id of H.264 (`"h264"` as a big-endian u32).
const CODEC_ID_H264: u32 = 0x6832_3634;
/// Codec id sent when the device disabled the video stream.
const CODEC_ID_DISABLED: u32 = 0;
/// Codec id sent when the device failed to configure the encoder.
const CODEC_ID_CONFIGURATION_ERROR: u32 = 1;
/// Size of every framing block (session meta or media header).
const BLOCK_LEN: usize = 12;
/// Bit 31 of the session meta flags; also bit 7 of the first wire byte.
const SESSION_FLAG: u32 = 1 << 31;
/// Bit 62 of `pts_and_flags`: decoder configuration packet (SPS/PPS).
const CONFIG_FLAG: u64 = 1 << 62;
/// Bit 61 of `pts_and_flags`: keyframe. Authoritative for the frame flag.
const KEYFRAME_FLAG: u64 = 1 << 61;
/// Bits 60-0 of `pts_and_flags`: presentation timestamp in microseconds.
const PTS_MASK: u64 = (1 << 61) - 1;

/// Errors raised while reading the scrcpy video stream.
///
/// The session layer maps these to `SessionError::Failed`: they always mean a
/// protocol or I/O failure, never a condition that a silent retry may fix.
#[derive(Debug, thiserror::Error)]
pub enum ScrcpyError {
    #[error("expected dummy byte 0x00, got 0x{0:02X}")]
    InvalidDummyByte(u8),
    #[error(
        "unexpected end of stream while reading {context}: expected {expected} bytes, got {got}"
    )]
    UnexpectedEof {
        context: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("truncated packet header: expected {expected} bytes, got {got}")]
    TruncatedHeader { expected: usize, got: usize },
    #[error("truncated packet payload: expected {expected} bytes, got {got}")]
    TruncatedPayload { expected: usize, got: usize },
    #[error("invalid device name: {0}")]
    InvalidDeviceName(String),
    #[error("the device disabled the video stream (codec id 0)")]
    StreamDisabled,
    #[error("the device failed to configure the video encoder (codec id 1)")]
    CodecConfigurationError,
    #[error("unsupported video codec id 0x{0:08X}")]
    UnsupportedCodec(u32),
    #[error("expected a session meta block after the codec id, got a media packet")]
    ExpectedSessionMeta,
    #[error("invalid session video size: {width}x{height}")]
    InvalidSessionSize { width: u32, height: u32 },
    #[error("media packet has a zero-sized payload")]
    ZeroSizedPacket,
    #[error("invalid H.264 data: {0}")]
    H264(#[from] h264::H264Error),
    #[error("scrcpy stream I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Parser that turns the raw scrcpy video socket into normalized messages.
///
/// [`ScrcpyVideoParser::connect`] validates the session handshake (dummy byte,
/// device name, codec id and the initial session meta) and then
/// [`next_message`](ScrcpyVideoParser::next_message) yields one
/// [`StreamMessage`] per config or frame packet. A clean end of stream at a
/// packet boundary returns `Ok(None)`.
pub struct ScrcpyVideoParser<R> {
    reader: R,
    device_name: String,
    width: u32,
    height: u32,
    /// Raw PTS of the first frame, subtracted from every later frame.
    first_pts: Option<i64>,
}

impl<R> ScrcpyVideoParser<R>
where
    R: AsyncRead + Unpin + Send,
{
    /// Reads and validates the fixed handshake of the video socket.
    ///
    /// The caller is responsible for opening the TCP connection and for the
    /// retry policy around it; this function consumes the dummy byte, the
    /// device name, the codec id and the initial session meta. The session
    /// layer that needs to open the control socket between the dummy byte and
    /// the rest of the handshake uses
    /// [`connect_after_dummy`](Self::connect_after_dummy) instead.
    pub async fn connect(mut reader: R) -> Result<Self, ScrcpyError> {
        let mut dummy = [0u8; 1];
        read_exact(&mut reader, &mut dummy, "dummy byte").await?;
        if dummy[0] != DUMMY_BYTE {
            return Err(ScrcpyError::InvalidDummyByte(dummy[0]));
        }

        Self::connect_after_dummy(reader).await
    }

    /// Handshake for a caller that already consumed the dummy byte.
    ///
    /// The server writes the dummy byte on the video socket as soon as the
    /// tunnel works, but it blocks until the control socket is accepted before
    /// it sends the device name (`DesktopConnection.open`). The session layer
    /// therefore reads the dummy byte right after connecting, opens the control
    /// socket and only then calls this function to read the device name, the
    /// codec id and the initial session meta.
    pub async fn connect_after_dummy(mut reader: R) -> Result<Self, ScrcpyError> {
        let mut name = [0u8; DEVICE_NAME_LEN];
        read_exact(&mut reader, &mut name, "device name").await?;
        let device_name = parse_device_name(&name)?;

        let mut codec = [0u8; 4];
        read_exact(&mut reader, &mut codec, "codec id").await?;
        match u32::from_be_bytes(codec) {
            CODEC_ID_DISABLED => return Err(ScrcpyError::StreamDisabled),
            CODEC_ID_CONFIGURATION_ERROR => return Err(ScrcpyError::CodecConfigurationError),
            CODEC_ID_H264 => {}
            other => return Err(ScrcpyError::UnsupportedCodec(other)),
        }

        let mut block = [0u8; BLOCK_LEN];
        read_exact(&mut reader, &mut block, "initial session meta").await?;
        if block[0] & 0x80 == 0 {
            return Err(ScrcpyError::ExpectedSessionMeta);
        }
        let (width, height) = parse_session_meta(&block)?;

        Ok(Self {
            reader,
            device_name,
            width,
            height,
            first_pts: None,
        })
    }

    /// Device name reported by the server (diagnostic only).
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Latest encoder dimensions from the session meta blocks.
    pub fn resolution(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Next normalized message, or `None` when the stream ends cleanly.
    ///
    /// Session meta blocks are consumed internally (they update
    /// [`resolution`](Self::resolution)); they never surface as messages.
    pub async fn next_message(&mut self) -> Result<Option<StreamMessage>, ScrcpyError> {
        loop {
            let Some(block) = self.read_block().await? else {
                return Ok(None);
            };

            // Bit 7 of the first byte is bit 63 of the u64 view: session meta.
            if block[0] & 0x80 != 0 {
                let (width, height) = parse_session_meta(&block)?;
                self.width = width;
                self.height = height;
                continue;
            }

            let pts_and_flags = u64::from_be_bytes([
                block[0], block[1], block[2], block[3], block[4], block[5], block[6], block[7],
            ]);
            let size = u32::from_be_bytes([block[8], block[9], block[10], block[11]]);
            if size == 0 {
                return Err(ScrcpyError::ZeroSizedPacket);
            }
            let payload = self.read_payload(size).await?;

            if pts_and_flags & CONFIG_FLAG != 0 {
                let avcc = h264::avcc_from_annex_b(&payload)?;
                return Ok(Some(StreamMessage::Config {
                    width: self.width,
                    height: self.height,
                    avcc,
                }));
            }

            // The protocol keyframe flag is authoritative. The IDR detection
            // performed by `normalize_sample` is kept as a cross-check in the
            // normalizer tests, not as the source of this flag.
            let keyframe = pts_and_flags & KEYFRAME_FLAG != 0;
            let raw_pts = (pts_and_flags & PTS_MASK) as i64;
            let first_pts = *self.first_pts.get_or_insert(raw_pts);
            let pts_us = raw_pts - first_pts;

            let sample = h264::normalize_sample(&payload)?;
            return Ok(Some(StreamMessage::Frame {
                pts_us,
                keyframe,
                data: sample.data,
            }));
        }
    }

    /// Reads one 12-byte framing block; `None` on a clean EOF at its start.
    async fn read_block(&mut self) -> Result<Option<[u8; BLOCK_LEN]>, ScrcpyError> {
        let mut block = [0u8; BLOCK_LEN];
        let mut filled = 0;
        while filled < BLOCK_LEN {
            let read = self
                .reader
                .read(&mut block[filled..])
                .await
                .map_err(ScrcpyError::Io)?;
            if read == 0 {
                if filled == 0 {
                    return Ok(None);
                }
                return Err(ScrcpyError::TruncatedHeader {
                    expected: BLOCK_LEN,
                    got: filled,
                });
            }
            filled += read;
        }
        Ok(Some(block))
    }

    /// Reads exactly `size` payload bytes.
    async fn read_payload(&mut self, size: u32) -> Result<Vec<u8>, ScrcpyError> {
        let mut payload = vec![0u8; size as usize];
        let mut filled = 0;
        while filled < payload.len() {
            let read = self
                .reader
                .read(&mut payload[filled..])
                .await
                .map_err(ScrcpyError::Io)?;
            if read == 0 {
                return Err(ScrcpyError::TruncatedPayload {
                    expected: payload.len(),
                    got: filled,
                });
            }
            filled += read;
        }
        Ok(payload)
    }
}

/// Parses the 64-byte device name field: UTF-8 truncated to 63 bytes plus zero
/// padding (`sendDeviceMeta`).
fn parse_device_name(bytes: &[u8; DEVICE_NAME_LEN]) -> Result<String, ScrcpyError> {
    let name_end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(DEVICE_NAME_LEN);
    let (name, padding) = bytes.split_at(name_end);
    if padding.iter().any(|&byte| byte != 0) {
        return Err(ScrcpyError::InvalidDeviceName(
            "field is not zero padded".to_owned(),
        ));
    }

    let name = std::str::from_utf8(name)
        .map_err(|error| ScrcpyError::InvalidDeviceName(error.to_string()))?;
    Ok(name.to_owned())
}

/// Parses a session meta block: `u32 BE flags | u32 BE width | u32 BE height`.
///
/// The session flag (bit 31) is the framing discriminator and is already
/// checked by the caller; `client_resized` (bit 0) is irrelevant in M1. Both
/// dimensions must be non-zero, at the start and in the middle of the stream.
fn parse_session_meta(block: &[u8; BLOCK_LEN]) -> Result<(u32, u32), ScrcpyError> {
    let flags = u32::from_be_bytes([block[0], block[1], block[2], block[3]]);
    if flags & SESSION_FLAG == 0 {
        return Err(ScrcpyError::ExpectedSessionMeta);
    }

    let width = u32::from_be_bytes([block[4], block[5], block[6], block[7]]);
    let height = u32::from_be_bytes([block[8], block[9], block[10], block[11]]);
    if width == 0 || height == 0 {
        return Err(ScrcpyError::InvalidSessionSize { width, height });
    }

    Ok((width, height))
}

/// Reads exactly `buffer.len()` bytes, reporting a clean EOF as a typed error.
async fn read_exact<R>(
    reader: &mut R,
    buffer: &mut [u8],
    context: &'static str,
) -> Result<(), ScrcpyError>
where
    R: AsyncRead + Unpin,
{
    let mut filled = 0;
    while filled < buffer.len() {
        let read = reader
            .read(&mut buffer[filled..])
            .await
            .map_err(ScrcpyError::Io)?;
        if read == 0 {
            return Err(ScrcpyError::UnexpectedEof {
                context,
                expected: buffer.len(),
                got: filled,
            });
        }
        filled += read;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use tokio::io::ReadBuf;

    use super::*;
    use ScrcpyError as E;

    const SPS: [u8; 4] = [0x67, 0x42, 0xC0, 0x1F];
    const PPS: [u8; 4] = [0x68, 0xCE, 0x3C, 0x80];
    const IDR: [u8; 4] = [0x65, 0x88, 0x84, 0x21];
    const P_FRAME: [u8; 4] = [0x41, 0x9A, 0x22, 0x33];

    /// Annex B buffer with 4-byte start codes per NAL unit.
    fn annex_b(nals: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for nal in nals {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(nal);
        }
        out
    }

    /// Dummy byte + 64-byte device name + codec id, as the server writes them.
    fn handshake(name: &str, codec: u32) -> Vec<u8> {
        let mut out = vec![0x00];
        out.extend_from_slice(&name_field(name));
        out.extend_from_slice(&codec.to_be_bytes());
        out
    }

    fn name_field(name: &str) -> [u8; DEVICE_NAME_LEN] {
        let mut field = [0u8; DEVICE_NAME_LEN];
        let bytes = name.as_bytes();
        let len = bytes.len().min(DEVICE_NAME_LEN);
        field[..len].copy_from_slice(&bytes[..len]);
        field
    }

    /// Session meta block: flags with bit 31, width, height.
    fn session_block(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity(BLOCK_LEN);
        out.extend_from_slice(&SESSION_FLAG.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out
    }

    /// Media header (`u64 BE pts_and_flags` + `u32 BE size`).
    fn media_header(pts_and_flags: u64, size: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(BLOCK_LEN);
        out.extend_from_slice(&pts_and_flags.to_be_bytes());
        out.extend_from_slice(&(size as u32).to_be_bytes());
        out
    }

    /// Config packet: bit 62, no PTS, Annex B SPS/PPS payload.
    fn config_packet(payload: &[u8]) -> Vec<u8> {
        let mut out = media_header(CONFIG_FLAG, payload.len());
        out.extend_from_slice(payload);
        out
    }

    /// Frame packet: bit 61 when `keyframe`, PTS in µs.
    fn frame_packet(raw_pts: u64, keyframe: bool, payload: &[u8]) -> Vec<u8> {
        let mut pts_and_flags = raw_pts & PTS_MASK;
        if keyframe {
            pts_and_flags |= KEYFRAME_FLAG;
        }
        let mut out = media_header(pts_and_flags, payload.len());
        out.extend_from_slice(payload);

        // Guards the fixture itself: the PTS and flag fields are disjoint.
        assert_eq!(raw_pts & !PTS_MASK, 0);
        out
    }

    fn expected_avcc() -> Vec<u8> {
        h264::build_avcc(&SPS, &PPS).expect("test parameter sets are valid")
    }

    #[tokio::test]
    async fn parses_dummy_name_codec_session_config_idr_and_p() {
        let mut bytes = handshake("Pixel 8", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(10_000, true, &annex_b(&[&IDR])));
        bytes.extend_from_slice(&frame_packet(26_000, false, &annex_b(&[&P_FRAME])));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");

        assert_eq!(parser.device_name(), "Pixel 8");
        assert_eq!(parser.resolution(), (1080, 2400));

        match parser.next_message().await.expect("config") {
            Some(StreamMessage::Config {
                width,
                height,
                avcc,
            }) => {
                assert_eq!((width, height), (1080, 2400));
                assert_eq!(avcc, expected_avcc());
            }
            other => panic!("expected config, got {other:?}"),
        }

        // First frame is rebased to PTS 0; the AVCC sample drops start codes.
        match parser.next_message().await.expect("idr") {
            Some(StreamMessage::Frame {
                pts_us,
                keyframe,
                data,
            }) => {
                assert_eq!(pts_us, 0);
                assert!(keyframe);
                assert_eq!(data, [0u8, 0, 0, 4, 0x65, 0x88, 0x84, 0x21]);
            }
            other => panic!("expected IDR frame, got {other:?}"),
        }

        match parser.next_message().await.expect("p frame") {
            Some(StreamMessage::Frame {
                pts_us,
                keyframe,
                data,
            }) => {
                assert_eq!(pts_us, 16_000);
                assert!(!keyframe);
                assert_eq!(data, [0u8, 0, 0, 4, 0x41, 0x9A, 0x22, 0x33]);
            }
            other => panic!("expected P frame, got {other:?}"),
        }

        assert!(parser.next_message().await.expect("clean EOF").is_none());
        assert!(parser.next_message().await.expect("clean EOF").is_none());
    }

    #[tokio::test]
    async fn connect_after_dummy_reads_name_codec_session_and_messages() {
        // Everything the server writes after the dummy byte, as the session
        // layer sees it once it has probed the tunnel.
        let mut bytes = name_field("SM-S911B").to_vec();
        bytes.extend_from_slice(&CODEC_ID_H264.to_be_bytes());
        bytes.extend_from_slice(&session_block(1080, 2340));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(7_000, true, &annex_b(&[&IDR])));

        let mut parser = ScrcpyVideoParser::connect_after_dummy(&bytes[..])
            .await
            .expect("handshake after dummy");

        assert_eq!(parser.device_name(), "SM-S911B");
        assert_eq!(parser.resolution(), (1080, 2340));

        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config {
                width: 1080,
                height: 2340,
                ..
            })
        ));
        assert!(matches!(
            parser.next_message().await.expect("idr"),
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
        assert!(parser.next_message().await.expect("EOF").is_none());
    }

    #[tokio::test]
    async fn connect_after_dummy_does_not_accept_a_pending_dummy_byte() {
        // A stream that still starts with the dummy byte is invalid here: the
        // dummy would be read as the first byte of the device name field,
        // breaking the zero-padding contract.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));

        assert!(matches!(
            ScrcpyVideoParser::connect_after_dummy(&bytes[..]).await,
            Err(E::InvalidDeviceName(_))
        ));
    }

    #[tokio::test]
    async fn both_connect_paths_produce_the_same_messages() {
        let mut with_dummy = handshake("Pixel 8", CODEC_ID_H264);
        with_dummy.extend_from_slice(&session_block(1080, 2400));
        with_dummy.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        with_dummy.extend_from_slice(&frame_packet(2_000, true, &annex_b(&[&IDR])));

        let without_dummy = with_dummy[1..].to_vec();

        let mut complete = ScrcpyVideoParser::connect(&with_dummy[..])
            .await
            .expect("full handshake");
        let mut split = ScrcpyVideoParser::connect_after_dummy(&without_dummy[..])
            .await
            .expect("split handshake");

        assert_eq!(complete.device_name(), split.device_name());
        assert_eq!(complete.resolution(), split.resolution());
        assert_eq!(
            complete.next_message().await.expect("config"),
            split.next_message().await.expect("config")
        );
        assert_eq!(
            complete.next_message().await.expect("frame"),
            split.next_message().await.expect("frame")
        );
        assert!(complete.next_message().await.expect("EOF").is_none());
        assert!(split.next_message().await.expect("EOF").is_none());

        // `without_dummy` only differs by the consumed byte.
        assert_eq!(without_dummy.len(), with_dummy.len() - 1);
    }

    #[tokio::test]
    async fn midstream_session_meta_updates_dimensions_for_next_config() {
        let mut bytes = handshake("SM-S911B", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2340));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(1_000, true, &annex_b(&[&IDR])));
        bytes.extend_from_slice(&frame_packet(2_000, false, &annex_b(&[&P_FRAME])));

        // Rotation: a standalone 12-byte session meta, then the new encoder
        // configuration and IDR, all in packet-boundary positions.
        bytes.extend_from_slice(&session_block(2340, 1080));
        let rotated_sps = [0x67, 0x4D, 0x00, 0x28];
        let rotated_pps = [0x68, 0xEE, 0x3C, 0x80];
        bytes.extend_from_slice(&config_packet(&annex_b(&[&rotated_sps, &rotated_pps])));
        bytes.extend_from_slice(&frame_packet(3_000, true, &annex_b(&[&IDR])));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert_eq!(parser.resolution(), (1080, 2340));

        assert!(matches!(
            parser.next_message().await.expect("initial config"),
            Some(StreamMessage::Config {
                width: 1080,
                height: 2340,
                ..
            })
        ));
        assert!(matches!(
            parser.next_message().await.expect("idr"),
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
        assert!(matches!(
            parser.next_message().await.expect("p frame"),
            Some(StreamMessage::Frame {
                pts_us: 1_000,
                keyframe: false,
                ..
            })
        ));

        // The session meta block is consumed internally and dimensions follow it.
        match parser.next_message().await.expect("rotated config") {
            Some(StreamMessage::Config {
                width,
                height,
                avcc,
            }) => {
                assert_eq!((width, height), (2340, 1080));
                assert_eq!(
                    avcc,
                    h264::build_avcc(&rotated_sps, &rotated_pps).expect("valid sets")
                );
            }
            other => panic!("expected rotated config, got {other:?}"),
        }
        assert_eq!(parser.resolution(), (2340, 1080));

        // PTS normalization is global: the first frame of the stream is 0.
        assert!(matches!(
            parser.next_message().await.expect("rotated idr"),
            Some(StreamMessage::Frame {
                pts_us: 2_000,
                keyframe: true,
                ..
            })
        ));
        assert!(parser.next_message().await.expect("EOF").is_none());
    }

    #[tokio::test]
    async fn config_packet_does_not_consume_the_pts_origin() {
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(720, 1280));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(50_000, true, &annex_b(&[&IDR])));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("idr"),
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn keyframe_flag_is_authoritative_over_idr_detection() {
        // Flag set on a P slice, and unset on an IDR slice: the parser trusts
        // the protocol flag, as the contract requires.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(800, 600));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(0, true, &annex_b(&[&P_FRAME])));
        bytes.extend_from_slice(&frame_packet(1_000, false, &annex_b(&[&IDR])));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("flagged P slice"),
            Some(StreamMessage::Frame { keyframe: true, .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("unflagged IDR"),
            Some(StreamMessage::Frame {
                keyframe: false,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn pts_mask_strips_the_keyframe_bit() {
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));

        // Large PTS values stay inside the 61-bit field. Bits above the mask
        // are flags and must not leak into the timestamp.
        let raw_pts = PTS_MASK - 100_000;
        bytes.extend_from_slice(&frame_packet(raw_pts, true, &annex_b(&[&IDR])));
        bytes.extend_from_slice(&frame_packet(raw_pts + 33_333, true, &annex_b(&[&IDR])));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("first frame"),
            Some(StreamMessage::Frame { pts_us: 0, .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("second frame"),
            Some(StreamMessage::Frame { pts_us: 33_333, .. })
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_dummy_byte() {
        let mut bytes = vec![0x7F];
        bytes.extend_from_slice(&name_field("Device"));
        bytes.extend_from_slice(&CODEC_ID_H264.to_be_bytes());
        bytes.extend_from_slice(&session_block(1080, 2400));

        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::InvalidDummyByte(0x7F))
        ));
    }

    #[tokio::test]
    async fn rejects_disabled_and_failed_codecs() {
        let mut disabled = handshake("Device", CODEC_ID_DISABLED);
        disabled.extend_from_slice(&session_block(1080, 2400));
        assert!(matches!(
            ScrcpyVideoParser::connect(&disabled[..]).await,
            Err(E::StreamDisabled)
        ));

        let mut failed = handshake("Device", CODEC_ID_CONFIGURATION_ERROR);
        failed.extend_from_slice(&session_block(1080, 2400));
        assert!(matches!(
            ScrcpyVideoParser::connect(&failed[..]).await,
            Err(E::CodecConfigurationError)
        ));
    }

    #[tokio::test]
    async fn rejects_unknown_codec_id() {
        let bytes = handshake("Device", 0x6832_3635);
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::UnsupportedCodec(0x6832_3635))
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_device_name() {
        // Invalid UTF-8 inside the name field.
        let mut bytes = vec![0x00];
        bytes.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0x00]);
        bytes.extend_from_slice(&[0u8; DEVICE_NAME_LEN - 4]);
        bytes.extend_from_slice(&CODEC_ID_H264.to_be_bytes());
        bytes.extend_from_slice(&session_block(1080, 2400));
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::InvalidDeviceName(_))
        ));

        // Non-zero bytes after the terminator break the zero padding contract.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        let last = DEVICE_NAME_LEN; // index of the final name byte in `bytes`
        bytes[last] = b'X';
        bytes.extend_from_slice(&session_block(1080, 2400));
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::InvalidDeviceName(_))
        ));
    }

    #[tokio::test]
    async fn rejects_first_block_that_is_not_a_session_meta() {
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));

        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::ExpectedSessionMeta)
        ));
    }

    #[tokio::test]
    async fn rejects_zero_dimensions_in_any_session_meta() {
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(0, 2400));
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::InvalidSessionSize {
                width: 0,
                height: 2400
            })
        ));

        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 0));
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::InvalidSessionSize {
                width: 1080,
                height: 0
            })
        ));

        // Mid-stream session meta: same validation.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(0, true, &annex_b(&[&IDR])));
        bytes.extend_from_slice(&session_block(1080, 0));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await.expect("idr"),
            Some(StreamMessage::Frame { .. })
        ));
        assert!(matches!(
            parser.next_message().await,
            Err(E::InvalidSessionSize {
                width: 1080,
                height: 0
            })
        ));
    }

    #[tokio::test]
    async fn rejects_truncated_handshake() {
        // Missing the dummy byte entirely.
        assert!(matches!(
            ScrcpyVideoParser::connect(&[][..]).await,
            Err(E::UnexpectedEof {
                context: "dummy byte",
                expected: 1,
                got: 0
            })
        ));

        // Device name cut short.
        let mut bytes = vec![0x00];
        bytes.extend_from_slice(b"Pixel");
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::UnexpectedEof {
                context: "device name",
                ..
            })
        ));

        // Codec id cut short.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.truncate(bytes.len() - 4);
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::UnexpectedEof {
                context: "codec id",
                ..
            })
        ));

        // Initial session meta cut short.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400)[..6]);
        assert!(matches!(
            ScrcpyVideoParser::connect(&bytes[..]).await,
            Err(E::UnexpectedEof {
                context: "initial session meta",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn rejects_truncated_header_and_payload() {
        // Header cut in the middle of a media packet.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        let payload = annex_b(&[&IDR]);
        bytes.extend_from_slice(&media_header(KEYFRAME_FLAG, payload.len())[..7]);

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await,
            Err(E::TruncatedHeader {
                expected: BLOCK_LEN,
                got: 7
            })
        ));

        // Payload cut short.
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        let payload = annex_b(&[&IDR]);
        let header = media_header(KEYFRAME_FLAG, payload.len());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&payload[..3]);

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            parser.next_message().await,
            Err(E::TruncatedPayload {
                expected,
                got: 3
            }) if expected == payload.len()
        ));
    }

    #[tokio::test]
    async fn rejects_zero_sized_media_packet() {
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&media_header(KEYFRAME_FLAG, 0));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await,
            Err(E::ZeroSizedPacket)
        ));
    }

    #[tokio::test]
    async fn maps_h264_errors_from_invalid_config_payload() {
        let mut bytes = handshake("Device", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        // Config packet without SPS/PPS.
        bytes.extend_from_slice(&config_packet(&annex_b(&[&IDR])));

        let mut parser = ScrcpyVideoParser::connect(&bytes[..])
            .await
            .expect("handshake");
        assert!(matches!(
            parser.next_message().await,
            Err(E::H264(h264::H264Error::MissingSps))
        ));
    }

    #[tokio::test]
    async fn parses_stream_delivered_in_small_chunks() {
        let mut bytes = handshake("Chunked", CODEC_ID_H264);
        bytes.extend_from_slice(&session_block(1080, 2400));
        bytes.extend_from_slice(&config_packet(&annex_b(&[&SPS, &PPS])));
        bytes.extend_from_slice(&frame_packet(5_000, true, &annex_b(&[&IDR])));

        let reader = TrickleReader::new(&bytes, 3);
        let mut parser = ScrcpyVideoParser::connect(reader)
            .await
            .expect("handshake across reads");

        assert_eq!(parser.device_name(), "Chunked");
        assert!(matches!(
            parser.next_message().await.expect("config"),
            Some(StreamMessage::Config {
                width: 1080,
                height: 2400,
                ..
            })
        ));
        assert!(matches!(
            parser.next_message().await.expect("idr"),
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
        assert!(parser.next_message().await.expect("EOF").is_none());
    }

    /// `AsyncRead` that yields at most `chunk` bytes per call.
    struct TrickleReader<'a> {
        data: &'a [u8],
        position: usize,
        chunk: usize,
    }

    impl<'a> TrickleReader<'a> {
        fn new(data: &'a [u8], chunk: usize) -> Self {
            Self {
                data,
                position: 0,
                chunk,
            }
        }
    }

    impl AsyncRead for TrickleReader<'_> {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            let start = this.position;
            let end = (start + this.chunk).min(this.data.len());
            let end = end.min(start + buf.remaining());
            if end <= start {
                return Poll::Ready(Ok(()));
            }

            buf.put_slice(&this.data[start..end]);
            this.position = end;
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn session_meta_parser_rejects_missing_session_flag() {
        let mut block = [0u8; BLOCK_LEN];
        block[4..8].copy_from_slice(&1080u32.to_be_bytes());
        block[8..12].copy_from_slice(&2400u32.to_be_bytes());

        assert!(matches!(
            parse_session_meta(&block),
            Err(E::ExpectedSessionMeta)
        ));
    }
}
