//! Replay fixture container (`LOTURP01`) and sibling metadata.
//!
//! A fixture is a pre-normalized stream: every record after the magic is exactly
//! one message of the internal WebSocket protocol described in
//! `docs/stream-protocol.md` (16-byte little-endian header plus payload, except
//! for the initial JSON `hello` text).
//!
//! The runtime only reads fixtures through [`ReplayFrameSource`](super::ReplayFrameSource);
//! the writer helpers at the bottom exist for the offline `make_replay_fixture`
//! example and for tests.

use std::path::{Path, PathBuf};

use lotu_app::dto::DeviceDto;
use lotu_domain::{Device, DeviceState, Resolution, Serial};
use serde::Deserialize;

/// ASCII magic that starts every replay container.
pub const MAGIC: &[u8; 8] = b"LOTURP01";

/// Exact `hello` text expected as the first record.
pub const HELLO_JSON: &str = r#"{"v":1,"type":"hello","codec":"h264"}"#;

/// Size of the internal stream message header (type, flags, reserved, PTS, size).
pub const MESSAGE_HEADER_LEN: usize = 16;

const RECORD_LENGTH_LEN: usize = 4;
const CONFIG_MESSAGE: u8 = 1;
const FRAME_MESSAGE: u8 = 2;
const KEYFRAME_FLAG: u8 = 0b0000_0001;
const CONFIG_PAYLOAD_HEADER_LEN: usize = 8;

/// Errors raised while reading a fixture or its metadata.
///
/// The Tauri delivery layer maps every variant to the stable `fixture_invalid`
/// error code.
#[derive(Debug, thiserror::Error)]
pub enum FixtureError {
    #[error("cannot read fixture file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid fixture magic; expected \"LOTURP01\"")]
    InvalidMagic,
    #[error("fixture is truncated: {0}")]
    Truncated(&'static str),
    #[error("record {index} is malformed: {reason}")]
    MalformedRecord { index: usize, reason: String },
    #[error("invalid hello record: {0}")]
    InvalidHello(String),
    #[error("invalid config record: {0}")]
    InvalidConfig(String),
    #[error("invalid frame record {index}: {reason}")]
    InvalidFrame { index: usize, reason: String },
    #[error("invalid record order: {0}")]
    InvalidOrder(String),
    #[error("invalid frame timestamps: {0}")]
    InvalidPts(String),
    #[error("invalid fixture metadata: {0}")]
    InvalidMeta(String),
    #[error("fixture record is larger than the u32 container limit")]
    RecordTooLarge,
}

/// One frame stored in a fixture (already in AVCC format).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureFrame {
    pub pts_us: i64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

/// Parsed contents of a replay fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayFixture {
    pub hello: String,
    pub width: u32,
    pub height: u32,
    /// Complete AVCDecoderConfigurationRecord (`lengthSizeMinusOne = 3`).
    pub avcc: Vec<u8>,
    pub frames: Vec<FixtureFrame>,
}

impl ReplayFixture {
    /// Parses and validates a complete fixture container.
    ///
    /// Rules enforced: magic, record framing, `hello` → config → frames order,
    /// exactly one config message, a non-empty frame list, normalized first PTS
    /// (`0`) and non-decreasing PTS values.
    pub fn parse(bytes: &[u8]) -> Result<Self, FixtureError> {
        let records = split_records(bytes)?;

        let hello_payload = records
            .first()
            .ok_or_else(|| FixtureError::InvalidOrder("missing hello record".to_owned()))?;
        let hello = parse_hello(hello_payload)?;

        let config_payload = records
            .get(1)
            .ok_or_else(|| FixtureError::InvalidOrder("missing config record".to_owned()))?;
        let (width, height, avcc) = parse_config(config_payload, 1)?;

        let mut frames = Vec::with_capacity(records.len().saturating_sub(2));
        let mut previous_pts: Option<i64> = None;
        for (index, payload) in records.iter().enumerate().skip(2) {
            let frame = parse_frame(payload, index, previous_pts)?;
            previous_pts = Some(frame.pts_us);
            frames.push(frame);
        }

        if frames.is_empty() {
            return Err(FixtureError::InvalidOrder(
                "fixture contains no frames".to_owned(),
            ));
        }

        Ok(Self {
            hello,
            width,
            height,
            avcc,
            frames,
        })
    }
}

/// Sibling `<name>.json` metadata of a replay fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureMeta {
    pub v: u32,
    pub name: String,
    /// Mirror of the `loop` JSON key (reserved word in Rust).
    pub looping: bool,
    pub device: DeviceDto,
}

impl FixtureMeta {
    /// Reads and validates the metadata next to a fixture file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FixtureError> {
        let path = path.as_ref();
        let text = read_to_string(path)?;
        Self::parse(&text)
    }

    /// Parses and validates fixture metadata from JSON text.
    pub fn parse(text: &str) -> Result<Self, FixtureError> {
        let file: MetaFile = serde_json::from_str(text)
            .map_err(|error| FixtureError::InvalidMeta(error.to_string()))?;

        if file.v != 1 {
            return Err(FixtureError::InvalidMeta(format!(
                "unsupported metadata version {}",
                file.v
            )));
        }
        if file.name.trim().is_empty() {
            return Err(FixtureError::InvalidMeta(
                "fixture name must not be empty".to_owned(),
            ));
        }

        let device = file.device.into_device()?;

        Ok(Self {
            v: file.v,
            name: file.name,
            looping: file.looping,
            device,
        })
    }
}

/// Encodes a fixture container from an already normalized config and frames.
///
/// Used by the offline fixture tool and by tests. The runtime never writes
/// fixtures.
pub fn encode_fixture(
    hello: &str,
    width: u32,
    height: u32,
    avcc: &[u8],
    frames: &[FixtureFrame],
) -> Result<Vec<u8>, FixtureError> {
    if width == 0 || height == 0 {
        return Err(FixtureError::InvalidConfig(
            "width and height must be greater than zero".to_owned(),
        ));
    }
    validate_avcc(avcc).map_err(FixtureError::InvalidConfig)?;

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    write_record(&mut out, hello.as_bytes())?;

    let mut config_payload = Vec::with_capacity(CONFIG_PAYLOAD_HEADER_LEN + avcc.len());
    config_payload.extend_from_slice(&width.to_le_bytes());
    config_payload.extend_from_slice(&height.to_le_bytes());
    config_payload.extend_from_slice(avcc);

    let mut config_message = Vec::with_capacity(MESSAGE_HEADER_LEN + config_payload.len());
    push_message_header(
        &mut config_message,
        CONFIG_MESSAGE,
        0,
        0,
        config_payload.len(),
    )?;
    config_message.extend_from_slice(&config_payload);
    write_record(&mut out, &config_message)?;

    for frame in frames {
        let flags = if frame.keyframe { KEYFRAME_FLAG } else { 0 };
        let mut message = Vec::with_capacity(MESSAGE_HEADER_LEN + frame.data.len());
        push_message_header(
            &mut message,
            FRAME_MESSAGE,
            flags,
            frame.pts_us,
            frame.data.len(),
        )?;
        message.extend_from_slice(&frame.data);
        write_record(&mut out, &message)?;
    }

    Ok(out)
}

pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, FixtureError> {
    std::fs::read(path).map_err(|source| FixtureError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_to_string(path: &Path) -> Result<String, FixtureError> {
    std::fs::read_to_string(path).map_err(|source| FixtureError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn split_records(bytes: &[u8]) -> Result<Vec<&[u8]>, FixtureError> {
    if bytes.len() < MAGIC.len() {
        return Err(FixtureError::Truncated("magic"));
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(FixtureError::InvalidMagic);
    }

    let mut records = Vec::new();
    let mut offset = MAGIC.len();
    while offset < bytes.len() {
        let length_end = offset + RECORD_LENGTH_LEN;
        if length_end > bytes.len() {
            return Err(FixtureError::Truncated("record length"));
        }

        let mut length_bytes = [0u8; RECORD_LENGTH_LEN];
        length_bytes.copy_from_slice(&bytes[offset..length_end]);
        let length = u32::from_le_bytes(length_bytes) as usize;
        offset = length_end;

        let payload_end = offset
            .checked_add(length)
            .ok_or(FixtureError::Truncated("record payload"))?;
        if payload_end > bytes.len() {
            return Err(FixtureError::Truncated("record payload"));
        }
        if length == 0 {
            return Err(FixtureError::MalformedRecord {
                index: records.len(),
                reason: "empty record payload".to_owned(),
            });
        }

        records.push(&bytes[offset..payload_end]);
        offset = payload_end;
    }

    Ok(records)
}

fn parse_hello(payload: &[u8]) -> Result<String, FixtureError> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| FixtureError::InvalidHello("hello must be UTF-8 text".to_owned()))?;
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| FixtureError::InvalidHello(format!("hello is not valid JSON: {error}")))?;

    let version = value.get("v").and_then(serde_json::Value::as_u64);
    let kind = value.get("type").and_then(serde_json::Value::as_str);
    let codec = value.get("codec").and_then(serde_json::Value::as_str);
    if version != Some(1) || kind != Some("hello") || codec != Some("h264") {
        return Err(FixtureError::InvalidHello(
            r#"expected {"v":1,"type":"hello","codec":"h264"}"#.to_owned(),
        ));
    }

    Ok(text.to_owned())
}

struct MessageHeader {
    kind: u8,
    flags: u8,
    pts_us: i64,
}

fn parse_message_header(payload: &[u8], index: usize) -> Result<MessageHeader, FixtureError> {
    if payload.len() < MESSAGE_HEADER_LEN {
        return Err(FixtureError::MalformedRecord {
            index,
            reason: format!(
                "binary message needs at least {MESSAGE_HEADER_LEN} bytes, got {}",
                payload.len()
            ),
        });
    }

    let reserved = u16::from_le_bytes([payload[2], payload[3]]);
    if reserved != 0 {
        return Err(FixtureError::MalformedRecord {
            index,
            reason: "reserved header bytes must be zero".to_owned(),
        });
    }

    let mut pts_bytes = [0u8; 8];
    pts_bytes.copy_from_slice(&payload[4..12]);
    let mut size_bytes = [0u8; 4];
    size_bytes.copy_from_slice(&payload[12..16]);

    let declared = u32::from_le_bytes(size_bytes) as usize;
    let actual = payload.len() - MESSAGE_HEADER_LEN;
    if declared != actual {
        return Err(FixtureError::MalformedRecord {
            index,
            reason: format!("declared payload length {declared} does not match {actual} bytes"),
        });
    }

    Ok(MessageHeader {
        kind: payload[0],
        flags: payload[1],
        pts_us: i64::from_le_bytes(pts_bytes),
    })
}

fn parse_config(payload: &[u8], index: usize) -> Result<(u32, u32, Vec<u8>), FixtureError> {
    let header = parse_message_header(payload, index)?;
    if header.kind != CONFIG_MESSAGE {
        return Err(FixtureError::InvalidOrder(
            "the second record must be the config message".to_owned(),
        ));
    }
    if header.flags != 0 {
        return Err(FixtureError::InvalidConfig(
            "config message flags must be zero".to_owned(),
        ));
    }
    if header.pts_us != 0 {
        return Err(FixtureError::InvalidConfig(
            "config message PTS must be zero".to_owned(),
        ));
    }

    let body = &payload[MESSAGE_HEADER_LEN..];
    if body.len() < CONFIG_PAYLOAD_HEADER_LEN {
        return Err(FixtureError::InvalidConfig(
            "config payload must contain width and height".to_owned(),
        ));
    }

    let width = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
    let height = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    if width == 0 || height == 0 {
        return Err(FixtureError::InvalidConfig(
            "width and height must be greater than zero".to_owned(),
        ));
    }

    let avcc = body[CONFIG_PAYLOAD_HEADER_LEN..].to_vec();
    validate_avcc(&avcc).map_err(FixtureError::InvalidConfig)?;

    Ok((width, height, avcc))
}

fn parse_frame(
    payload: &[u8],
    index: usize,
    previous_pts: Option<i64>,
) -> Result<FixtureFrame, FixtureError> {
    let header = parse_message_header(payload, index)?;
    if header.kind == CONFIG_MESSAGE {
        return Err(FixtureError::InvalidOrder(
            "config message must be the second record and appear only once".to_owned(),
        ));
    }
    if header.kind != FRAME_MESSAGE {
        return Err(FixtureError::MalformedRecord {
            index,
            reason: format!("unsupported message type {}", header.kind),
        });
    }
    if header.flags & !KEYFRAME_FLAG != 0 {
        return Err(FixtureError::InvalidFrame {
            index,
            reason: "unsupported frame flags".to_owned(),
        });
    }
    if payload.len() == MESSAGE_HEADER_LEN {
        return Err(FixtureError::InvalidFrame {
            index,
            reason: "frame payload is empty".to_owned(),
        });
    }

    match previous_pts {
        None if header.pts_us != 0 => {
            return Err(FixtureError::InvalidPts(format!(
                "first frame PTS must be 0, got {}",
                header.pts_us
            )));
        }
        Some(previous) if header.pts_us < previous => {
            return Err(FixtureError::InvalidPts(format!(
                "frame {index} PTS {} is lower than previous PTS {previous}",
                header.pts_us
            )));
        }
        _ => {}
    }

    Ok(FixtureFrame {
        pts_us: header.pts_us,
        keyframe: header.flags & KEYFRAME_FLAG != 0,
        data: payload[MESSAGE_HEADER_LEN..].to_vec(),
    })
}

fn validate_avcc(avcc: &[u8]) -> Result<(), String> {
    if avcc.len() < 7 {
        return Err("avcC is too short".to_owned());
    }
    if avcc[0] != 1 {
        return Err(format!("unsupported avcC configurationVersion {}", avcc[0]));
    }
    if avcc[4] & 0x03 != 3 {
        return Err("avcC must use 4-byte NAL length prefixes".to_owned());
    }

    let num_sps = avcc[5] & 0x1F;
    if num_sps != 1 {
        return Err(format!("avcC must declare exactly one SPS, got {num_sps}"));
    }

    let mut offset = 6;
    let sps_length = read_u16(avcc, offset).ok_or("avcC is missing the SPS length")? as usize;
    offset += 2;
    if sps_length < 4 {
        return Err("SPS must be at least 4 bytes".to_owned());
    }
    offset = checked_skip(avcc, offset, sps_length).ok_or("avcC SPS length exceeds its payload")?;

    let num_pps = *avcc.get(offset).ok_or("avcC is missing the PPS count")? as usize;
    offset += 1;
    if num_pps != 1 {
        return Err(format!("avcC must declare exactly one PPS, got {num_pps}"));
    }

    let pps_length = read_u16(avcc, offset).ok_or("avcC is missing the PPS length")? as usize;
    offset += 2;
    if pps_length == 0 {
        return Err("PPS must not be empty".to_owned());
    }
    checked_skip(avcc, offset, pps_length).ok_or("avcC PPS length exceeds its payload")?;

    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn checked_skip(bytes: &[u8], offset: usize, length: usize) -> Option<usize> {
    let end = offset.checked_add(length)?;
    if end > bytes.len() {
        return None;
    }
    Some(end)
}

fn push_message_header(
    out: &mut Vec<u8>,
    kind: u8,
    flags: u8,
    pts_us: i64,
    payload_len: usize,
) -> Result<(), FixtureError> {
    let payload_len = u32::try_from(payload_len).map_err(|_| FixtureError::RecordTooLarge)?;
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&pts_us.to_le_bytes());
    out.extend_from_slice(&payload_len.to_le_bytes());
    Ok(())
}

fn write_record(out: &mut Vec<u8>, payload: &[u8]) -> Result<(), FixtureError> {
    let length = u32::try_from(payload.len()).map_err(|_| FixtureError::RecordTooLarge)?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

#[derive(Debug, Deserialize)]
struct MetaFile {
    v: u32,
    name: String,
    #[serde(rename = "loop")]
    looping: bool,
    device: MetaDevice,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetaDevice {
    serial: String,
    name: String,
    state: String,
    android_version: Option<String>,
    resolution: Option<MetaResolution>,
    battery_percent: Option<u8>,
    transport: String,
}

#[derive(Debug, Deserialize)]
struct MetaResolution {
    width: u32,
    height: u32,
}

impl MetaDevice {
    fn into_device(self) -> Result<DeviceDto, FixtureError> {
        if self.transport != "usb" {
            return Err(FixtureError::InvalidMeta(format!(
                "unsupported transport {:?}; expected \"usb\"",
                self.transport
            )));
        }

        let serial = Serial::new(self.serial)
            .map_err(|error| FixtureError::InvalidMeta(error.to_string()))?;
        let state = parse_state(&self.state)?;
        let resolution = match self.resolution {
            Some(resolution) => Some(
                Resolution::new(resolution.width, resolution.height)
                    .map_err(|error| FixtureError::InvalidMeta(error.to_string()))?,
            ),
            None => None,
        };

        let device = Device::new(
            serial,
            self.name,
            state,
            self.android_version,
            resolution,
            self.battery_percent,
        )
        .map_err(|error| FixtureError::InvalidMeta(error.to_string()))?;

        Ok(DeviceDto::from(device))
    }
}

fn parse_state(raw: &str) -> Result<DeviceState, FixtureError> {
    match raw {
        "connected" => Ok(DeviceState::Connected),
        "unauthorized" => Ok(DeviceState::Unauthorized),
        "offline" => Ok(DeviceState::Offline),
        other => Err(FixtureError::InvalidMeta(format!(
            "unsupported device state {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_avcc() -> Vec<u8> {
        let sps = [0x67, 0x42, 0xC0, 0x1F];
        let pps = [0x68, 0xCE, 0x3C, 0x80];
        let mut avcc = vec![1, sps[1], sps[2], sps[3], 0xFF, 0xE1];
        avcc.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(&sps);
        avcc.push(1);
        avcc.extend_from_slice(&(pps.len() as u16).to_be_bytes());
        avcc.extend_from_slice(&pps);
        avcc
    }

    fn message(kind: u8, flags: u8, pts_us: i64, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        push_message_header(&mut out, kind, flags, pts_us, payload.len())
            .expect("test payload fits u32");
        out.extend_from_slice(payload);
        out
    }

    fn config_message(width: u32, height: u32, avcc: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&width.to_le_bytes());
        payload.extend_from_slice(&height.to_le_bytes());
        payload.extend_from_slice(avcc);
        message(CONFIG_MESSAGE, 0, 0, &payload)
    }

    fn frame_message(pts_us: i64, keyframe: bool, data: &[u8]) -> Vec<u8> {
        message(
            FRAME_MESSAGE,
            if keyframe { KEYFRAME_FLAG } else { 0 },
            pts_us,
            data,
        )
    }

    fn container(records: &[Vec<u8>]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for record in records {
            write_record(&mut out, record).expect("test record fits u32");
        }
        out
    }

    fn valid_container() -> Vec<u8> {
        container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
            frame_message(0, true, &[0, 0, 0, 2, 0x65, 0x88]),
            frame_message(33_333, false, &[0, 0, 0, 2, 0x41, 0x9A]),
        ])
    }

    #[test]
    fn parses_valid_fixture() {
        let fixture = ReplayFixture::parse(&valid_container()).expect("fixture should parse");

        assert_eq!(fixture.hello, HELLO_JSON);
        assert_eq!(fixture.width, 1080);
        assert_eq!(fixture.height, 2400);
        assert_eq!(fixture.avcc, synthetic_avcc());
        assert_eq!(fixture.frames.len(), 2);
        assert_eq!(fixture.frames[0].pts_us, 0);
        assert!(fixture.frames[0].keyframe);
        assert_eq!(fixture.frames[1].pts_us, 33_333);
        assert!(!fixture.frames[1].keyframe);
    }

    #[test]
    fn accepts_non_decreasing_pts() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
            frame_message(0, false, &[0, 0, 0, 1, 0x41]),
        ]);

        let fixture = ReplayFixture::parse(&bytes).expect("equal PTS should be accepted");
        assert_eq!(fixture.frames.len(), 2);
    }

    #[test]
    fn rejects_invalid_magic() {
        let mut bytes = valid_container();
        bytes[..MAGIC.len()].copy_from_slice(b"LOTURP02");

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidMagic)
        ));
    }

    #[test]
    fn rejects_truncated_magic() {
        assert!(matches!(
            ReplayFixture::parse(b"LOTU"),
            Err(FixtureError::Truncated("magic"))
        ));
    }

    #[test]
    fn rejects_truncated_record_length() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&[0x01, 0x00]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::Truncated("record length"))
        ));
    }

    #[test]
    fn rejects_truncated_record_payload() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(b"short");

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::Truncated("record payload"))
        ));
    }

    #[test]
    fn rejects_missing_hello_record() {
        assert!(matches!(
            ReplayFixture::parse(&container(&[])),
            Err(FixtureError::InvalidOrder(message)) if message.contains("hello")
        ));
    }

    #[test]
    fn rejects_config_as_first_record() {
        let bytes = container(&[config_message(1080, 2400, &synthetic_avcc())]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidHello(_))
        ));
    }

    #[test]
    fn rejects_missing_config_record() {
        let bytes = container(&[HELLO_JSON.as_bytes().to_vec()]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidOrder(message)) if message.contains("config")
        ));
    }

    #[test]
    fn rejects_frame_before_config() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidOrder(message)) if message.contains("config")
        ));
    }

    #[test]
    fn rejects_config_after_frames() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
            config_message(1080, 2400, &synthetic_avcc()),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidOrder(message)) if message.contains("only once")
        ));
    }

    #[test]
    fn rejects_first_pts_other_than_zero() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
            frame_message(33_333, true, &[0, 0, 0, 1, 0x65]),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidPts(message)) if message.contains("first frame")
        ));
    }

    #[test]
    fn rejects_decreasing_pts() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
            frame_message(10_000, false, &[0, 0, 0, 1, 0x41]),
            frame_message(5_000, false, &[0, 0, 0, 1, 0x41]),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidPts(message)) if message.contains("lower than")
        ));
    }

    #[test]
    fn rejects_fixture_without_frames() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidOrder(message)) if message.contains("no frames")
        ));
    }

    #[test]
    fn rejects_invalid_hello() {
        let bytes = container(&[
            br#"{"v":1,"type":"bye","codec":"h264"}"#.to_vec(),
            config_message(1080, 2400, &synthetic_avcc()),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidHello(_))
        ));
    }

    #[test]
    fn rejects_avcc_without_four_byte_lengths() {
        let mut avcc = synthetic_avcc();
        avcc[4] = 0xFC;

        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(1080, 2400, &avcc),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidConfig(message)) if message.contains("4-byte")
        ));
    }

    #[test]
    fn rejects_zero_resolution() {
        let bytes = container(&[
            HELLO_JSON.as_bytes().to_vec(),
            config_message(0, 2400, &synthetic_avcc()),
            frame_message(0, true, &[0, 0, 0, 1, 0x65]),
        ]);

        assert!(matches!(
            ReplayFixture::parse(&bytes),
            Err(FixtureError::InvalidConfig(message)) if message.contains("greater than zero")
        ));
    }

    #[test]
    fn encode_and_parse_round_trip() {
        let frames = vec![
            FixtureFrame {
                pts_us: 0,
                keyframe: true,
                data: vec![0, 0, 0, 2, 0x65, 0x88],
            },
            FixtureFrame {
                pts_us: 33_333,
                keyframe: false,
                data: vec![0, 0, 0, 2, 0x41, 0x9A],
            },
        ];

        let bytes = encode_fixture(HELLO_JSON, 1080, 2400, &synthetic_avcc(), &frames)
            .expect("fixture should encode");
        let fixture = ReplayFixture::parse(&bytes).expect("encoded fixture should parse");

        assert_eq!(fixture.frames, frames);
        assert_eq!(fixture.avcc, synthetic_avcc());
    }

    #[test]
    fn encoded_container_uses_internal_stream_framing() {
        let frames = vec![FixtureFrame {
            pts_us: 0,
            keyframe: true,
            data: vec![0, 0, 0, 2, 0x65, 0x88],
        }];
        let bytes = encode_fixture(HELLO_JSON, 1080, 2400, &synthetic_avcc(), &frames)
            .expect("fixture should encode");
        let records = split_records(&bytes).expect("encoded records should split");

        assert_eq!(records.len(), 3);
        assert_eq!(
            std::str::from_utf8(records[0]).expect("hello is UTF-8"),
            HELLO_JSON
        );

        let config = records[1];
        assert_eq!(config[0], CONFIG_MESSAGE);
        assert_eq!(config[1], 0);
        assert_eq!(&config[2..4], &[0u8, 0u8][..]);
        assert_eq!(&config[4..12], &[0u8; 8][..]);
        assert_eq!(
            u32::from_le_bytes([config[12], config[13], config[14], config[15]]) as usize,
            8 + synthetic_avcc().len()
        );

        let frame = records[2];
        assert_eq!(frame[0], FRAME_MESSAGE);
        assert_eq!(frame[1] & KEYFRAME_FLAG, KEYFRAME_FLAG);
        assert_eq!(&frame[4..12], &[0u8; 8][..]);
        assert_eq!(
            u32::from_le_bytes([frame[12], frame[13], frame[14], frame[15]]) as usize,
            frames[0].data.len()
        );
    }

    #[test]
    fn parses_metadata() {
        let meta = FixtureMeta::parse(META_JSON).expect("metadata should parse");

        assert_eq!(meta.v, 1);
        assert_eq!(meta.name, "pixel-portrait");
        assert!(meta.looping);
        assert_eq!(meta.device.serial, "REPLAY0001");
        assert_eq!(meta.device.name, "Pixel 8 Pro (replay)");
        assert_eq!(meta.device.state, DeviceState::Connected);
        assert_eq!(meta.device.android_version.as_deref(), Some("15"));
        assert_eq!(
            meta.device.resolution.map(|r| (r.width, r.height)),
            Some((1080, 2400))
        );
        assert_eq!(meta.device.battery_percent, Some(85));
        assert_eq!(meta.device.transport, "usb");
    }

    #[test]
    fn rejects_metadata_with_unknown_state() {
        let json = META_JSON.replace("\"connected\"", "\"authorized\"");

        assert!(matches!(
            FixtureMeta::parse(&json),
            Err(FixtureError::InvalidMeta(message)) if message.contains("state")
        ));
    }

    #[test]
    fn rejects_metadata_with_non_usb_transport() {
        let json = META_JSON.replace("\"usb\"", "\"wifi\"");

        assert!(matches!(
            FixtureMeta::parse(&json),
            Err(FixtureError::InvalidMeta(message)) if message.contains("transport")
        ));
    }

    #[test]
    fn rejects_metadata_with_unsupported_version() {
        let json = META_JSON.replace("\"v\": 1", "\"v\": 2");

        assert!(matches!(
            FixtureMeta::parse(&json),
            Err(FixtureError::InvalidMeta(message)) if message.contains("version")
        ));
    }

    #[test]
    fn rejects_metadata_with_out_of_range_battery() {
        let json = META_JSON.replace("\"batteryPercent\": 85", "\"batteryPercent\": 185");

        assert!(matches!(
            FixtureMeta::parse(&json),
            Err(FixtureError::InvalidMeta(message)) if message.contains("battery")
        ));
    }

    const META_JSON: &str = r#"{
  "v": 1,
  "name": "pixel-portrait",
  "loop": true,
  "device": {
    "serial": "REPLAY0001",
    "name": "Pixel 8 Pro (replay)",
    "state": "connected",
    "androidVersion": "15",
    "resolution": { "width": 1080, "height": 2400 },
    "batteryPercent": 85,
    "transport": "usb"
  }
}"#;
}
