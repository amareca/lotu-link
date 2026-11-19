//! File-backed MP4 recorder (M3, rotation-aware since M4).

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lotu_app::ports::{Recorder, RecordingError, RecordingStats, StreamMessage};

use super::mp4::Mp4Writer;

/// Durable state of one recording session. A device rotation changes the
/// encoded frame size, which cannot share an MP4 track with the old frames, so
/// each configuration becomes its own file.
#[derive(Debug)]
struct Session {
    base: PathBuf,
    part: PathBuf,
    segment: u32,
    paths: Vec<PathBuf>,
    frames: u64,
    duration_us: u64,
}

/// The single recorder of the application. Interiors are mutex-protected so the
/// session actor and Tauri commands can share one `Arc<dyn Recorder>`.
#[derive(Debug, Default)]
pub struct Mp4Recorder {
    inner: Mutex<Option<Mp4Writer<File>>>,
    session: Mutex<Option<Session>>,
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

    fn session(&self) -> Result<std::sync::MutexGuard<'_, Option<Session>>, RecordingError> {
        self.session
            .lock()
            .map_err(|_| RecordingError::Failed("recorder session lock is poisoned".to_owned()))
    }

    /// Opens a new part file for the current segment.
    fn open_segment(
        &self,
        session: &Session,
        seed: Option<&(u32, u32, &[u8])>,
    ) -> Result<Mp4Writer<File>, RecordingError> {
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&session.part)
            .map_err(|error| {
                RecordingError::Failed(format!("cannot create {:?}: {error}", session.part))
            })?;
        Mp4Writer::new(file, seed)
    }

    /// Finalizes the current segment and commits its temporary file.
    fn commit(
        &self,
        writer: &mut Mp4Writer<File>,
        session: &mut Session,
    ) -> Result<(), RecordingError> {
        let stats = writer.finish()?;
        let destination = segment_path(&session.base, session.segment);
        if destination.exists() {
            std::fs::remove_file(&destination).map_err(|error| {
                RecordingError::Failed(format!("cannot replace {destination:?}: {error}"))
            })?;
        }
        std::fs::rename(&session.part, &destination).map_err(|error| {
            let _ = std::fs::remove_file(&session.part);
            RecordingError::Failed(format!("cannot rename {:?}: {error}", session.part))
        })?;
        session.paths.push(destination);
        session.frames += stats.frames;
        session.duration_us += stats.duration_us;
        Ok(())
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

        let seed = config_seed(seed)?;
        let session = Session {
            part: part_path(&path),
            base: path,
            segment: 1,
            paths: Vec::new(),
            frames: 0,
            duration_us: 0,
        };
        *inner = Some(self.open_segment(&session, seed.as_ref())?);
        *self.session()? = Some(session);
        Ok(())
    }

    fn write(&self, message: &StreamMessage) -> Result<(), RecordingError> {
        let mut inner = self.writer()?;
        let writer = inner.as_mut().ok_or(RecordingError::Inactive)?;
        let mut session = self.session()?;
        let session = session.as_mut().ok_or(RecordingError::Inactive)?;

        match message {
            StreamMessage::Config {
                width,
                height,
                avcc,
            } => {
                if writer.needs_new_segment(*width, *height) {
                    self.commit(writer, session)?;
                    session.segment += 1;
                    session.part = part_path(&segment_path(&session.base, session.segment));
                    *writer =
                        self.open_segment(session, Some(&(*width, *height, avcc.as_slice())))?;
                } else {
                    writer.set_config(*width, *height, avcc);
                }
            }
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
        let mut session = self.session()?.take().ok_or(RecordingError::Inactive)?;

        if let Err(error) = self.commit(&mut writer, &mut session) {
            let _ = std::fs::remove_file(&session.part);
            return Err(error);
        }

        Ok(RecordingStats {
            frames: session.frames,
            duration_us: session.duration_us,
            segments: session.paths.len() as u32,
            paths: session.paths,
        })
    }

    fn is_active(&self) -> bool {
        self.writer().map(|inner| inner.is_some()).unwrap_or(false)
    }
}

type Seed<'a> = Option<(u32, u32, &'a [u8])>;

fn config_seed(seed: Option<&StreamMessage>) -> Result<Seed<'_>, RecordingError> {
    match seed {
        None => Ok(None),
        Some(StreamMessage::Config {
            width,
            height,
            avcc,
        }) => Ok(Some((*width, *height, avcc.as_slice()))),
        Some(_) => Err(RecordingError::Invalid(
            "recording seed must be a decoder configuration".to_owned(),
        )),
    }
}

/// Appends `.part.mp4` so an interrupted recording never leaves a corrupt final file.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".part.mp4");
    PathBuf::from(name)
}

/// First segment keeps the chosen name; later ones get `-2`, `-3`, ...
fn segment_path(base: &Path, segment: u32) -> PathBuf {
    if segment == 1 {
        return base.to_path_buf();
    }
    let stem = base
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "recording".to_owned());
    let extension = base
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default();
    base.with_file_name(format!("{stem}-{segment}{extension}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn config(width: u32, height: u32) -> StreamMessage {
        StreamMessage::Config {
            width,
            height,
            avcc: vec![1, 0x42, 0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68],
        }
    }

    fn frame(pts_us: i64, keyframe: bool) -> StreamMessage {
        StreamMessage::Frame {
            pts_us,
            keyframe,
            data: vec![0, 0, 0, 2, if keyframe { 0x65 } else { 0x41 }],
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lotu-mp4-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        dir
    }

    #[test]
    fn writes_a_valid_file_to_disk() {
        let dir = temp_dir("single");
        let path = dir.join("clip.mp4");
        let recorder = Arc::new(Mp4Recorder::new());
        recorder
            .start(path.clone(), Some(&config(1080, 2400)))
            .expect("start");
        assert!(recorder.is_active());
        recorder.write(&frame(0, true)).expect("first frame");
        recorder.write(&frame(16_000, false)).expect("second frame");

        let stats = recorder.finish().expect("finish");
        assert_eq!(stats.frames, 2);
        assert_eq!(stats.duration_us, 32_000);
        assert_eq!(stats.segments, 1);
        assert_eq!(stats.paths, vec![path.clone()]);
        assert!(!part_path(&path).exists());
        let bytes = std::fs::read(&path).expect("read mp4");
        assert_eq!(&bytes[4..8], b"ftyp");
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn rotation_produces_two_independent_segments() {
        let dir = temp_dir("rotation");
        let path = dir.join("clip.mp4");
        let recorder = Mp4Recorder::new();
        recorder
            .start(path.clone(), Some(&config(1080, 2400)))
            .expect("start");
        recorder.write(&frame(0, true)).expect("portrait IDR");
        recorder.write(&frame(16_000, false)).expect("portrait P");
        recorder
            .write(&config(2400, 1080))
            .expect("rotation config");
        recorder.write(&frame(32_000, true)).expect("landscape IDR");

        let stats = recorder.finish().expect("finish");
        assert_eq!(stats.segments, 2);
        assert_eq!(stats.paths.len(), 2);
        assert_eq!(stats.paths[0], path);
        assert_eq!(stats.paths[1], dir.join("clip-2.mp4"));
        for segment in &stats.paths {
            assert!(segment.is_file(), "{segment:?} must exist");
            assert!(!part_path(segment).exists());
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_config_before_the_first_frame_does_not_open_a_segment() {
        let dir = temp_dir("early-config");
        let path = dir.join("clip.mp4");
        let recorder = Mp4Recorder::new();
        recorder
            .start(path.clone(), Some(&config(1080, 2400)))
            .expect("start");
        recorder.write(&config(2400, 1080)).expect("early config");
        recorder.write(&frame(0, true)).expect("IDR");

        let stats = recorder.finish().expect("finish");
        assert_eq!(stats.segments, 1);
        assert_eq!(stats.paths, vec![path]);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn rejects_start_while_active_and_finish_while_inactive() {
        let recorder = Mp4Recorder::new();
        assert_eq!(recorder.finish(), Err(RecordingError::Inactive));

        let path =
            std::env::temp_dir().join(format!("lotu-mp4-inactive-{}.mp4", std::process::id()));
        recorder
            .start(path.clone(), Some(&config(1080, 2400)))
            .expect("start");
        recorder.write(&frame(0, true)).expect("frame");
        assert_eq!(
            recorder.start(path.clone(), Some(&config(1080, 2400))),
            Err(RecordingError::AlreadyActive)
        );
        recorder.finish().expect("finish");
        let _ = std::fs::remove_file(&path);
    }
}
