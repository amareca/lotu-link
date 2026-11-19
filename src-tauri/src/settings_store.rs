//! Local JSON settings store (M4). Lives in the delivery layer because it
//! resolves paths through Tauri; the application only sees the port.

use std::path::PathBuf;

use lotu_app::ports::{SettingsData, SettingsError, SettingsStore};
use lotu_domain::VideoConfig;
use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "settings.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredVideo {
    max_size: u32,
    max_fps: u32,
    bit_rate: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct StoredSettings {
    v: u32,
    video: StoredVideo,
}

impl From<SettingsData> for StoredSettings {
    fn from(data: SettingsData) -> Self {
        Self {
            v: SCHEMA_VERSION,
            video: StoredVideo {
                max_size: data.video.max_size,
                max_fps: data.video.max_fps,
                bit_rate: data.video.bit_rate,
            },
        }
    }
}

/// Reads and writes `settings.json` under a directory chosen by the caller.
pub struct FileSettingsStore {
    directory: PathBuf,
}

impl FileSettingsStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(SETTINGS_FILE)
    }
}

impl SettingsStore for FileSettingsStore {
    fn load(&self) -> Result<SettingsData, SettingsError> {
        let path = self.path();
        if !path.exists() {
            return Ok(SettingsData::default());
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| SettingsError::Io(format!("cannot read {path:?}: {error}")))?;
        let stored: StoredSettings = serde_json::from_str(&text)
            .map_err(|error| SettingsError::Invalid(format!("bad settings.json: {error}")))?;
        if stored.v != SCHEMA_VERSION {
            return Err(SettingsError::Invalid(format!(
                "unsupported settings version {}",
                stored.v
            )));
        }
        let stored_video = stored.video;
        let video = VideoConfig::new(
            stored_video.max_size,
            stored_video.max_fps,
            stored_video.bit_rate,
        )
        .map_err(|error| SettingsError::Invalid(error.to_string()))?;
        Ok(SettingsData { video })
    }

    fn save(&self, data: &SettingsData) -> Result<(), SettingsError> {
        let path = self.path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| SettingsError::Io(format!("cannot create {parent:?}: {error}")))?;
        }
        let temp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&StoredSettings::from(*data))
            .map_err(|error| SettingsError::Io(error.to_string()))?;
        std::fs::write(&temp, &bytes)
            .map_err(|error| SettingsError::Io(format!("cannot write {temp:?}: {error}")))?;
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|error| SettingsError::Io(format!("cannot replace {path:?}: {error}")))?;
        }
        std::fs::rename(&temp, &path)
            .map_err(|error| SettingsError::Io(format!("cannot rename {temp:?}: {error}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lotu-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_uses_defaults() {
        let dir = temp_dir("missing");
        let store = FileSettingsStore::new(dir.clone());
        assert_eq!(store.load().expect("load").video, VideoConfig::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn round_trips_video_settings() {
        let dir = temp_dir("roundtrip");
        let store = FileSettingsStore::new(dir.clone());
        let expected = SettingsData {
            video: VideoConfig::new(1280, 30, 4_000_000).expect("valid"),
        };

        store.save(&expected).expect("save");
        assert!(dir.join(SETTINGS_FILE).is_file());
        assert!(!dir.join("settings.json.tmp").exists());
        assert_eq!(store.load().expect("load"), expected);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn rejects_corrupt_and_invalid_payloads() {
        let dir = temp_dir("invalid");
        let store = FileSettingsStore::new(dir.clone());
        std::fs::create_dir_all(&dir).expect("create dir");

        std::fs::write(dir.join(SETTINGS_FILE), b"{broken").expect("write");
        assert!(matches!(store.load(), Err(SettingsError::Invalid(_))));

        std::fs::write(
            dir.join(SETTINGS_FILE),
            br#"{"v":1,"video":{"maxSize":1280,"maxFps":30,"bitRate":0}}"#,
        )
        .expect("write");
        assert!(matches!(store.load(), Err(SettingsError::Invalid(_))));

        std::fs::write(
            dir.join(SETTINGS_FILE),
            br#"{"v":99,"video":{"maxSize":0,"maxFps":60,"bitRate":8000000}}"#,
        )
        .expect("write");
        assert!(matches!(store.load(), Err(SettingsError::Invalid(_))));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
