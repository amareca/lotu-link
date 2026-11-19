//! File-backed MP4 recorder (M3).

use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::sync::Mutex;

use lotu_app::ports::{Recorder, RecordingError, RecordingStats, StreamMessage};

use super::mp4::Mp4Writer;

/// The single recorder of the application. Interiors are mutex-protected so the
/// session actor and Tauri commands can share one `Arc<dyn Recorder>`.
#[derive(Debug, Default)]
pub struct Mp4Recorder {
    inner: Mutex<Option<Mp4Writer<File>>>,
    /// Final destination and the temporary file used while recording.
    paths: Mutex<Option<(PathBuf, PathBuf)>>,
}

impl Mp4Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    fn writer(&self) -> Result<std::sync::MutexGuard<'_, Option<Mp4Writer<File>>>, RecordingError> {
        self.inner
            .lock()
            .map_err(|_| RecordingError::Failed("recorder lock is poisoned".to_owned()))
    }
}

impl Recorder for Mp4Recorder {
    fn start(&self, path: PathBuf, seed: Option<&StreamMessage>) -> Result<(), RecordingError> {
        let mut inner = self.writer()?;
        if inner.is_some() {
            return Err(RecordingError::AlreadyActive);
        }
        if path.as_os_str().is_empty() {
            return Err(RecordingError::Invalid("empty output path".to_owned()));
        }

        let seed = match seed {
            None => None,
            Some(StreamMessage::Config {
                width,
                height,
                avcc,
            }) => Some((*width, *height, avcc.as_slice())),
            Some(_) => {
                return Err(RecordingError::Invalid(
                    "recording seed must be a decoder configuration".to_owned(),
                ))
            }
        };

        let part = part_path(&path);
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&part)
            .map_err(|error| RecordingError::Failed(format!("cannot create {part:?}: {error}")))?;
        let writer = Mp4Writer::new(file, seed.as_ref())?;
        *inner = Some(writer);
        *self
            .paths
            .lock()
            .map_err(|_| RecordingError::Failed("recorder lock is poisoned".to_owned()))? =
            Some((path, part));
        Ok(())
    }

    fn write(&self, message: &StreamMessage) -> Result<(), RecordingError> {
        let mut inner = self.writer()?;
        let writer = inner.as_mut().ok_or(RecordingError::Inactive)?;

        match message {
            StreamMessage::Config {
                width,
                height,
                avcc,
            } => writer.set_config(*width, *height, avcc),
            StreamMessage::Frame {
                pts_us,
                keyframe,
                data,
            } => {
                writer.write_frame(*pts_us, *keyframe, data)?;
            }
        }
        Ok(())
    }

    fn finish(&self) -> Result<RecordingStats, RecordingError> {
        let mut inner = self.writer()?;
        let mut writer = inner.take().ok_or(RecordingError::Inactive)?;
        let paths = self
            .paths
            .lock()
            .map_err(|_| RecordingError::Failed("recorder lock is poisoned".to_owned()))?
            .take();

        let stats = match writer.finish() {
            Ok(stats) => stats,
            Err(error) => {
                if let Some((_, part)) = paths {
                    let _ = std::fs::remove_file(part);
                }
                return Err(error);
            }
        };

        if let Some((final_path, part)) = paths {
            // `rename` cannot replace an existing file on Windows: remove the
            // old destination first, then commit the complete file.
            if final_path.exists() {
                std::fs::remove_file(&final_path).map_err(|error| {
                    RecordingError::Failed(format!("cannot replace {final_path:?}: {error}"))
                })?;
            }
            std::fs::rename(&part, &final_path).map_err(|error| {
                let _ = std::fs::remove_file(&part);
                RecordingError::Failed(format!("cannot rename {part:?}: {error}"))
            })?;
        }
        Ok(stats)
    }

    fn is_active(&self) -> bool {
        self.writer().map(|inner| inner.is_some()).unwrap_or(false)
    }
}

/// Appends `.part.mp4` to the chosen path so an interrupted recording never
/// leaves a playable-looking but corrupt final file.
fn part_path(path: &std::path::Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".part.mp4");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn config() -> StreamMessage {
        StreamMessage::Config {
            width: 1080,
            height: 2400,
            avcc: vec![1, 0x42, 0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68],
        }
    }

    #[test]
    fn writes_a_valid_file_to_disk() {
        let dir = std::env::temp_dir().join(format!("lotu-mp4-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("clip.mp4");

        let recorder = Arc::new(Mp4Recorder::new());
        recorder
            .start(path.clone(), Some(&config()))
            .expect("start");
        assert!(recorder.is_active());

        recorder
            .write(&StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                data: vec![0, 0, 0, 2, 0x65],
            })
            .expect("first frame");
        recorder
            .write(&StreamMessage::Frame {
                pts_us: 16_000,
                keyframe: false,
                data: vec![0, 0, 0, 2, 0x41],
            })
            .expect("second frame");

        let stats = recorder.finish().expect("finish");
        assert_eq!(stats.frames, 2);
        assert_eq!(stats.duration_us, 32_000);
        assert!(!recorder.is_active());

        assert!(!part_path(&path).exists(), "temporary file must be renamed");
        let bytes = std::fs::read(&path).expect("read mp4");
        assert_eq!(&bytes[4..8], b"ftyp");
        assert!(bytes.windows(4).any(|window| window == b"moov"));
        std::fs::remove_dir_all(&dir).expect("remove temp dir");
    }

    #[test]
    fn rejects_start_while_active_and_finish_while_inactive() {
        let recorder = Mp4Recorder::new();
        assert_eq!(recorder.finish(), Err(RecordingError::Inactive));

        let path =
            std::env::temp_dir().join(format!("lotu-mp4-inactive-{}.mp4", std::process::id()));
        recorder
            .start(path.clone(), Some(&config()))
            .expect("start");
        recorder
            .write(&StreamMessage::Frame {
                pts_us: 0,
                keyframe: true,
                data: vec![0, 0, 0, 1, 0x65],
            })
            .expect("frame");
        assert_eq!(
            recorder.start(path.clone(), Some(&config())),
            Err(RecordingError::AlreadyActive)
        );
        recorder.finish().expect("finish");
        assert!(!part_path(&path).exists());
        let _ = std::fs::remove_file(&path);
    }
}
