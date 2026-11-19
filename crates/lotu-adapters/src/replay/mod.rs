//! Replay frame source: serves a pre-normalized fixture at PTS pace.
//!
//! The adapter has no dependency on ADB, scrcpy or the network. It is a
//! development aid for M0b and the seed of the M1 `FrameSource` implementations.

pub mod annex_b;
pub mod fixture;

use std::path::Path;
use std::time::Duration;

use async_trait::async_trait;
use lotu_app::dto::DeviceDto;
use lotu_app::ports::{FrameSource, StreamMessage};

pub use fixture::{FixtureError, FixtureFrame, FixtureMeta, ReplayFixture};

use fixture::read_file;

/// Replays a `.lotureplay` fixture as a [`FrameSource`].
///
/// Messages follow the normalized stream contract: a [`StreamMessage::Config`]
/// first, then frames. With `loop` enabled in the metadata, the configuration is
/// sent again before the first frame of every new cycle.
pub struct ReplayFrameSource {
    fixture: ReplayFixture,
    meta: FixtureMeta,
    cursor: usize,
    config_pending: bool,
    last_pts: Option<i64>,
}

impl ReplayFrameSource {
    /// Loads a fixture and its sibling `<name>.json` metadata.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FixtureError> {
        let path = path.as_ref();
        let bytes = read_file(path)?;
        let fixture = ReplayFixture::parse(&bytes)?;
        let meta = FixtureMeta::load(path.with_extension("json"))?;

        Ok(Self {
            fixture,
            meta,
            cursor: 0,
            config_pending: true,
            last_pts: None,
        })
    }

    /// `hello` text to send before the binary stream.
    pub fn hello(&self) -> &str {
        &self.fixture.hello
    }

    /// Fixture metadata (name, loop flag and device DTO).
    pub fn meta(&self) -> &FixtureMeta {
        &self.meta
    }

    /// Fixture name from the metadata.
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    /// Device exposed for the replay session DTO.
    pub fn device(&self) -> &DeviceDto {
        &self.meta.device
    }

    fn config_message(&self) -> StreamMessage {
        StreamMessage::Config {
            width: self.fixture.width,
            height: self.fixture.height,
            avcc: self.fixture.avcc.clone(),
        }
    }
}

#[async_trait]
impl FrameSource for ReplayFrameSource {
    async fn next_message(&mut self) -> Option<StreamMessage> {
        if self.config_pending {
            self.config_pending = false;
            self.last_pts = None;
            return Some(self.config_message());
        }

        if self.cursor >= self.fixture.frames.len() {
            if !self.meta.looping {
                return None;
            }
            self.cursor = 0;
            self.last_pts = None;
            return Some(self.config_message());
        }

        let frame = self.fixture.frames[self.cursor].clone();
        if let Some(previous_pts) = self.last_pts {
            let delta_us = frame.pts_us - previous_pts;
            if delta_us > 0 {
                // The first frame of a cycle is immediate: `last_pts` is reset
                // when a config message is emitted.
                tokio::time::sleep(Duration::from_micros(delta_us as u64)).await;
            }
        }

        self.last_pts = Some(frame.pts_us);
        self.cursor += 1;

        Some(StreamMessage::Frame {
            pts_us: frame.pts_us,
            keyframe: frame.keyframe,
            data: frame.data,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::fixture::HELLO_JSON;
    use super::*;

    const META_TEMPLATE: &str = r#"{
  "v": 1,
  "name": "test-fixture",
  "loop": LOOP,
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

    fn frames(pts: &[i64]) -> Vec<FixtureFrame> {
        pts.iter()
            .enumerate()
            .map(|(index, pts_us)| FixtureFrame {
                pts_us: *pts_us,
                keyframe: index == 0,
                data: if index == 0 {
                    vec![0, 0, 0, 2, 0x65, 0x88]
                } else {
                    vec![0, 0, 0, 2, 0x41, 0x9A]
                },
            })
            .collect()
    }

    fn write_fixture(tag: &str, looping: bool, frames: &[FixtureFrame]) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let dir =
            std::env::temp_dir().join(format!("lotu-replay-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temporary directory should be created");

        let bytes = fixture::encode_fixture(HELLO_JSON, 1080, 2400, &synthetic_avcc(), frames)
            .expect("test fixture should encode");
        let fixture_path = dir.join("test-fixture.lotureplay");
        std::fs::write(&fixture_path, bytes).expect("fixture should be written");

        let meta = META_TEMPLATE.replace("LOOP", if looping { "true" } else { "false" });
        std::fs::write(dir.join("test-fixture.json"), meta).expect("metadata should be written");

        fixture_path
    }

    #[tokio::test]
    async fn replays_config_then_frames_and_ends_without_loop() {
        let path = write_fixture("order", false, &frames(&[0, 1_000, 2_000]));
        let mut source = ReplayFrameSource::load(&path).expect("fixture should load");

        assert_eq!(source.hello(), HELLO_JSON);
        assert_eq!(source.name(), "test-fixture");
        assert_eq!(source.device().serial, "REPLAY0001");

        match source.next_message().await {
            Some(StreamMessage::Config {
                width,
                height,
                avcc,
            }) => {
                assert_eq!(width, 1080);
                assert_eq!(height, 2400);
                assert_eq!(avcc, synthetic_avcc());
            }
            other => panic!("expected config, got {other:?}"),
        }

        for expected_pts in [0, 1_000, 2_000] {
            match source.next_message().await {
                Some(StreamMessage::Frame { pts_us, .. }) => assert_eq!(pts_us, expected_pts),
                other => panic!("expected frame, got {other:?}"),
            }
        }

        assert!(source.next_message().await.is_none());
        assert!(source.next_message().await.is_none());
    }

    #[tokio::test]
    async fn loop_resends_config_and_restarts_pts() {
        let path = write_fixture("loop", true, &frames(&[0, 1_000]));
        let mut source = ReplayFrameSource::load(&path).expect("fixture should load");

        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame { pts_us: 1_000, .. })
        ));

        // The second cycle starts with a fresh config and PTS reset to zero.
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                ..
            })
        ));
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame { pts_us: 1_000, .. })
        ));
    }

    #[tokio::test]
    async fn first_frame_is_immediate_and_later_frames_are_paced() {
        let path = write_fixture("pace", false, &frames(&[0, 50_000]));
        let mut source = ReplayFrameSource::load(&path).expect("fixture should load");

        let start = tokio::time::Instant::now();
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Config { .. })
        ));
        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame { pts_us: 0, .. })
        ));
        assert!(
            start.elapsed() < Duration::from_millis(40),
            "the first frame must not wait for its PTS"
        );

        assert!(matches!(
            source.next_message().await,
            Some(StreamMessage::Frame { pts_us: 50_000, .. })
        ));
        assert!(
            start.elapsed() >= Duration::from_millis(40),
            "the second frame should follow the PTS delta"
        );
    }

    #[tokio::test]
    async fn invalid_container_fails_to_load() {
        let path = write_fixture("invalid", false, &frames(&[0]));
        let mut bytes = std::fs::read(&path).expect("fixture should be readable");
        bytes[..8].copy_from_slice(b"BROKEN01");
        std::fs::write(&path, bytes).expect("fixture should be writable");

        assert!(matches!(
            ReplayFrameSource::load(&path),
            Err(FixtureError::InvalidMagic)
        ));
    }

    #[tokio::test]
    async fn missing_metadata_fails_to_load() {
        let path = write_fixture("no-meta", false, &frames(&[0]));
        let meta_path = path.with_extension("json");
        std::fs::remove_file(&meta_path).expect("metadata should be removable");

        assert!(matches!(
            ReplayFrameSource::load(&path),
            Err(FixtureError::Io { .. })
        ));
    }
}
