//! Byte-for-byte regeneration of the committed M0b replay fixture.
//!
//! `make_replay_fixture` is an offline tool, but the normalization it performs
//! (Annex B → AVCC, `avcC`, IDR flags, PTS) now lives in production code
//! (`media::h264`). This test proves that the production module still reproduces
//! the committed container exactly, so promoting it did not change the fixture.

use std::path::{Path, PathBuf};

use lotu_adapters::media::h264::convert_annex_b;
use lotu_adapters::replay::fixture::{encode_fixture, FixtureFrame, ReplayFixture, HELLO_JSON};

const FIXTURE_NAME: &str = "pixel-portrait";
const FPS: u32 = 30;
const WIDTH: u32 = 1080;
const HEIGHT: u32 = 2400;
const EXPECTED_FRAMES: usize = 180;
const EXPECTED_KEYFRAMES: usize = 6;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("replay")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn make_replay_fixture_regenerates_the_committed_container() {
    let source_path = fixtures_dir().join(format!("{FIXTURE_NAME}.h264"));
    let fixture_path = fixtures_dir().join(format!("{FIXTURE_NAME}.lotureplay"));

    let source = read(&source_path);
    let committed = read(&fixture_path);

    let clip = convert_annex_b(&source).expect("the committed clip should normalize");
    let interval_us = 1_000_000 / i64::from(FPS);
    let frames: Vec<FixtureFrame> = clip
        .samples
        .into_iter()
        .enumerate()
        .map(|(index, sample)| FixtureFrame {
            pts_us: i64::try_from(index).expect("frame index fits in i64") * interval_us,
            keyframe: sample.keyframe,
            data: sample.data,
        })
        .collect();

    let regenerated = encode_fixture(HELLO_JSON, WIDTH, HEIGHT, &clip.avcc, &frames)
        .expect("the regenerated fixture should encode");

    assert_eq!(frames.len(), EXPECTED_FRAMES);
    assert_eq!(
        frames.iter().filter(|frame| frame.keyframe).count(),
        EXPECTED_KEYFRAMES
    );
    assert_eq!(frames.first().map(|frame| frame.pts_us), Some(0));

    let parsed = ReplayFixture::parse(&regenerated).expect("the regenerated fixture should parse");
    assert_eq!(parsed.frames.len(), EXPECTED_FRAMES);
    assert_eq!(
        parsed.frames.iter().filter(|frame| frame.keyframe).count(),
        EXPECTED_KEYFRAMES
    );

    // hello + config + one record per frame.
    assert_eq!(parsed.frames.len() + 2, 182);

    assert_eq!(
        regenerated, committed,
        "the committed fixture must be reproducible byte for byte"
    );
}
