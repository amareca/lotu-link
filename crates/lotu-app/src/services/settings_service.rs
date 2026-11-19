//! Application service for settings: one validated in-memory snapshot.

use std::sync::Mutex;

use lotu_domain::VideoConfig;

use crate::ports::{SettingsData, SettingsError, SettingsStore};

/// Owns the current settings. The store is the durable source; this service is
/// the only reader used by commands and by session startup.
pub struct SettingsService {
    store: Box<dyn SettingsStore>,
    current: Mutex<SettingsData>,
}

impl SettingsService {
    pub fn new(store: Box<dyn SettingsStore>) -> Self {
        let current = match store.load() {
            Ok(data) => data,
            Err(error) => {
                eprintln!("using default settings: {error}");
                SettingsData::default()
            }
        };
        Self {
            store,
            current: Mutex::new(current),
        }
    }

    /// Current snapshot, copied so callers never hold the lock.
    pub fn get(&self) -> SettingsData {
        *self.lock()
    }

    pub fn video_config(&self) -> VideoConfig {
        self.get().video
    }

    /// Validates, persists and installs the new snapshot.
    pub fn save(&self, video: VideoConfig) -> Result<SettingsData, SettingsError> {
        let mut next = *self.lock();
        next.video = video;
        self.store.save(&next)?;
        *self.lock() = next;
        Ok(next)
    }

    /// Marks the onboarding help as seen without touching video settings.
    pub fn mark_onboarding_seen(&self) -> Result<SettingsData, SettingsError> {
        let mut next = *self.lock();
        next.onboarding_seen = true;
        self.store.save(&next)?;
        *self.lock() = next;
        Ok(next)
    }

    pub fn onboarding_seen(&self) -> bool {
        self.lock().onboarding_seen
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SettingsData> {
        self.current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeStore {
        data: Mutex<SettingsData>,
        saves: Mutex<usize>,
        fail: bool,
    }

    impl SettingsStore for FakeStore {
        fn load(&self) -> Result<SettingsData, SettingsError> {
            if self.fail {
                return Err(SettingsError::Invalid("corrupt".to_owned()));
            }
            Ok(*self.data.lock().expect("data lock"))
        }

        fn save(&self, data: &SettingsData) -> Result<(), SettingsError> {
            *self.data.lock().expect("data lock") = *data;
            *self.saves.lock().expect("saves lock") += 1;
            Ok(())
        }
    }

    #[test]
    fn loads_the_stored_snapshot() {
        let store = Box::new(FakeStore {
            data: Mutex::new(SettingsData {
                video: VideoConfig::new(1280, 30, 4_000_000).expect("valid"),
                onboarding_seen: false,
            }),
            ..FakeStore::default()
        });
        let service = SettingsService::new(store);
        assert_eq!(
            service.video_config(),
            VideoConfig::new(1280, 30, 4_000_000).expect("valid")
        );
    }

    #[test]
    fn falls_back_to_defaults_on_a_corrupt_store() {
        let store = FakeStore {
            fail: true,
            ..FakeStore::default()
        };
        let service = SettingsService::new(Box::new(store));
        assert_eq!(service.video_config(), VideoConfig::default());
    }

    #[test]
    fn marks_onboarding_without_changing_video() {
        let store = Box::new(FakeStore::default());
        let service = SettingsService::new(store);
        let before = service.video_config();

        service.mark_onboarding_seen().expect("mark seen");
        assert!(service.onboarding_seen());
        assert_eq!(service.video_config(), before);
    }

    #[test]
    fn saves_and_replaces_the_snapshot() {
        let store = Box::new(FakeStore::default());
        let service = SettingsService::new(store);
        let next = VideoConfig::new(1920, 30, 6_000_000).expect("valid");

        assert_eq!(service.save(next).expect("save").video, next);
        assert_eq!(service.video_config(), next);
    }
}
