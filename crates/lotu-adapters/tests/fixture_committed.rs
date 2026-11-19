//! Regression tests for the committed M0b replay fixture.
//!
//! `fixtures/replay/pixel-portrait.lotureplay` and its sibling `.json` are
//! repository artifacts consumed by the M0b replay mode. These tests pin the
//! byte-level contract so accidental regeneration, corruption or a metadata
//! edit is caught before the fixture reaches the UI.
//!
//! The fixture is located relative to the crate manifest
//! (`CARGO_MANIFEST_DIR` → `crates/lotu-adapters` → workspace root), so the
//! tests work from any checkout path and on any platform.

use std::path::{Path, PathBuf};

use lotu_adapters::replay::fixture::{
    FixtureMeta, ReplayFixture, HELLO_JSON, MAGIC, MESSAGE_HEADER_LEN,
};
use lotu_adapters::replay::ReplayFrameSource;

const FIXTURE_NAME: &str = "pixel-portrait";
const RECORD_LENGTH_LEN: usize = 4;
const EXPECTED_RECORDS: usize = 182; // hello + config + 180 frames
const EXPECTED_FRAMES: usize = 180;
const EXPECTED_KEYFRAMES: usize = 6;
const EXPECTED_WIDTH: u32 = 1080;
const EXPECTED_HEIGHT: u32 = 2400;

fn fixture_path() -> PathBuf {
    fixtures_dir().join(format!("{FIXTURE_NAME}.lotureplay"))
}

fn metadata_path() -> PathBuf {
    fixtures_dir().join(format!("{FIXTURE_NAME}.json"))
}

/// Resolves `fixtures/replay` from the workspace root, not from the process cwd.
fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("replay")
}

fn read_fixture_bytes() -> Vec<u8> {
    let path = fixture_path();
    std::fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Splits the container into record payloads using the public framing constants.
fn split_records(bytes: &[u8]) -> Vec<&[u8]> {
    assert!(
        bytes.len() >= MAGIC.len(),
        "fixture is shorter than the {}-byte magic",
        MAGIC.len()
    );
    assert_eq!(&bytes[..MAGIC.len()], MAGIC, "unexpected fixture magic");

    let mut records = Vec::new();
    let mut offset = MAGIC.len();
    while offset < bytes.len() {
        let length_end = offset + RECORD_LENGTH_LEN;
        let length_bytes: [u8; RECORD_LENGTH_LEN] = bytes
            .get(offset..length_end)
            .unwrap_or_else(|| panic!("truncated record length at offset {offset}"))
            .try_into()
            .expect("record length slice has the expected size");
        offset = length_end;

        let length = u32::from_le_bytes(length_bytes) as usize;
        let payload_end = offset + length;
        let payload = bytes
            .get(offset..payload_end)
            .unwrap_or_else(|| panic!("truncated record payload at offset {offset}"));
        records.push(payload);
        offset = payload_end;
    }

    assert_eq!(offset, bytes.len(), "records must cover the whole fixture");
    records
}

/// Minimal AVCDecoderConfigurationRecord view; panics on anything malformed.
struct AvcC<'a> {
    sps: &'a [u8],
    pps: &'a [u8],
}

fn parse_avcc(avcc: &[u8]) -> AvcC<'_> {
    assert!(avcc.len() >= 8, "avcC is too short: {} bytes", avcc.len());
    assert_eq!(avcc[0], 1, "configurationVersion must be 1");
    assert_eq!(
        avcc[4] & 0x03,
        3,
        "lengthSizeMinusOne must be 3 (4-byte NAL lengths)"
    );
    assert_eq!(avcc[5] & 0x1F, 1, "numOfSPS must be 1");

    let sps_len = u16::from_be_bytes([avcc[6], avcc[7]]) as usize;
    let sps_start = 8;
    let sps_end = sps_start + sps_len;
    assert!(sps_len >= 4, "SPS must be at least 4 bytes");
    let sps = avcc
        .get(sps_start..sps_end)
        .unwrap_or_else(|| panic!("SPS length {sps_len} exceeds the avcC payload"));
    assert_eq!(sps[0] & 0x1F, 7, "first SPS NAL must have type 7");
    assert_eq!(
        &avcc[1..4],
        &sps[1..4],
        "avcC profile bytes must mirror the SPS"
    );

    let pps_count = *avcc
        .get(sps_end)
        .unwrap_or_else(|| panic!("avcC is missing the PPS count at offset {sps_end}"));
    assert_eq!(pps_count, 1, "numOfPPS must be 1");

    let pps_len_offset = sps_end + 1;
    let pps_len_bytes = avcc
        .get(pps_len_offset..pps_len_offset + 2)
        .expect("avcC is missing the PPS length");
    let pps_len = u16::from_be_bytes([pps_len_bytes[0], pps_len_bytes[1]]) as usize;
    assert!(pps_len > 0, "PPS must not be empty");

    let pps_start = pps_len_offset + 2;
    let pps_end = pps_start + pps_len;
    let pps = avcc
        .get(pps_start..pps_end)
        .unwrap_or_else(|| panic!("PPS length {pps_len} exceeds the avcC payload"));
    assert_eq!(pps[0] & 0x1F, 8, "first PPS NAL must have type 8");
    assert_eq!(
        pps_end,
        avcc.len(),
        "avcC must contain exactly one SPS and one PPS"
    );

    AvcC { sps, pps }
}

#[test]
fn committed_fixture_files_exist() {
    for path in [fixture_path(), metadata_path()] {
        assert!(
            path.is_file(),
            "missing committed fixture file: {}",
            path.display()
        );
    }
}

#[test]
fn container_has_magic_and_expected_records() {
    let bytes = read_fixture_bytes();
    let records = split_records(&bytes);

    assert_eq!(
        records.len(),
        EXPECTED_RECORDS,
        "expected hello + config + {EXPECTED_FRAMES} frames"
    );

    let hello = std::str::from_utf8(records[0]).expect("hello record must be UTF-8 text");
    assert_eq!(hello, HELLO_JSON, "unexpected hello record");

    let config = records[1];
    assert!(
        config.len() >= MESSAGE_HEADER_LEN + 8,
        "config record is shorter than its header plus width/height"
    );
    assert_eq!(config[0], 1, "second record must be a config message");
    assert_eq!(config[1], 0, "config message flags must be zero");
    assert_eq!(&config[2..4], &[0, 0], "reserved header bytes must be zero");
    assert_eq!(
        i64::from_le_bytes(config[4..12].try_into().expect("config PTS")),
        0,
        "config message PTS must be zero"
    );
    let declared = u32::from_le_bytes(config[12..16].try_into().expect("config size")) as usize;
    assert_eq!(
        declared,
        config.len() - MESSAGE_HEADER_LEN,
        "config payload length must match the header"
    );

    let width = u32::from_le_bytes(config[16..20].try_into().expect("config width"));
    let height = u32::from_le_bytes(config[20..24].try_into().expect("config height"));
    assert_eq!((width, height), (EXPECTED_WIDTH, EXPECTED_HEIGHT));

    for (index, frame) in records[2..].iter().enumerate() {
        assert_eq!(
            frame[0],
            2,
            "record {} must be a frame message, not a config",
            index + 2
        );
        assert_eq!(
            frame[1] & !0x01,
            0,
            "frame {} has unsupported flags",
            index + 2
        );
        assert!(
            frame.len() > MESSAGE_HEADER_LEN,
            "frame {} has an empty payload",
            index + 2
        );
        let declared = u32::from_le_bytes(frame[12..16].try_into().expect("frame size")) as usize;
        assert_eq!(
            declared,
            frame.len() - MESSAGE_HEADER_LEN,
            "frame {} declared size must match its payload",
            index + 2
        );
    }
}

#[test]
fn fixture_parses_with_expected_frames_and_timestamps() {
    let fixture = ReplayFixture::parse(&read_fixture_bytes()).expect("fixture should parse");

    assert_eq!(fixture.hello, HELLO_JSON);
    assert_eq!(
        (fixture.width, fixture.height),
        (EXPECTED_WIDTH, EXPECTED_HEIGHT)
    );
    assert_eq!(fixture.frames.len(), EXPECTED_FRAMES);
    assert_eq!(fixture.frames[0].pts_us, 0, "first PTS must be normalized");

    for (index, pair) in fixture.frames.windows(2).enumerate() {
        assert!(
            pair[0].pts_us <= pair[1].pts_us,
            "frame {index} PTS {} is greater than frame {} PTS {}",
            pair[0].pts_us,
            index + 1,
            pair[1].pts_us
        );
    }

    let keyframes = fixture.frames.iter().filter(|frame| frame.keyframe).count();
    assert_eq!(keyframes, EXPECTED_KEYFRAMES);
    assert!(fixture.frames[0].keyframe, "first frame must be a keyframe");

    // Every frame must be AVCC: consecutive 4-byte big-endian NAL lengths that
    // cover the sample exactly, with no Annex B start codes.
    for (index, frame) in fixture.frames.iter().enumerate() {
        let mut offset = 0;
        let mut nal_count = 0;
        while offset < frame.data.len() {
            let length_bytes = frame
                .data
                .get(offset..offset + 4)
                .unwrap_or_else(|| panic!("frame {index} ends with a truncated NAL length prefix"));
            let length = u32::from_be_bytes(
                length_bytes
                    .try_into()
                    .expect("NAL length slice has size 4"),
            ) as usize;
            assert!(length > 0, "frame {index} contains an empty NAL unit");
            offset += 4 + length;
            nal_count += 1;
        }
        assert_eq!(
            offset,
            frame.data.len(),
            "frame {index} NAL lengths must cover the sample exactly"
        );
        assert!(nal_count > 0, "frame {index} contains no NAL units");
    }
}

#[test]
fn avcc_declares_one_sps_and_one_pps() {
    let fixture = ReplayFixture::parse(&read_fixture_bytes()).expect("fixture should parse");
    let avcc = parse_avcc(&fixture.avcc);

    // The fixture was generated from a real clip; parameter sets must carry the
    // SPS/PPS NAL types, not arbitrary bytes.
    assert_eq!(avcc.sps[0] & 0x1F, 7);
    assert_eq!(avcc.pps[0] & 0x1F, 8);
}

#[test]
fn metadata_matches_committed_device() {
    let path = metadata_path();
    let meta = FixtureMeta::load(&path)
        .unwrap_or_else(|error| panic!("cannot load {}: {error}", path.display()));

    assert_eq!(meta.v, 1);
    assert_eq!(meta.name, FIXTURE_NAME);
    assert!(meta.looping, "the fixture is expected to loop");

    let device = &meta.device;
    assert_eq!(device.serial, "REPLAY0001");
    assert_eq!(device.name, "Pixel 8 Pro (replay)");
    assert_eq!(device.transport, "usb");
    let resolution = device
        .resolution
        .expect("replay device must declare a resolution");
    assert_eq!(
        (resolution.width, resolution.height),
        (EXPECTED_WIDTH, EXPECTED_HEIGHT)
    );
}

#[test]
fn replay_source_loads_fixture_and_metadata_together() {
    let path = fixture_path();
    let source = ReplayFrameSource::load(&path)
        .unwrap_or_else(|error| panic!("cannot load {}: {error}", path.display()));

    assert_eq!(source.hello(), HELLO_JSON);
    assert_eq!(source.name(), FIXTURE_NAME);
    assert_eq!(source.device().serial, "REPLAY0001");
    assert!(source.meta().looping);
}
