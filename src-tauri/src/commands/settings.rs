//! M4 settings commands. Thin validation plus the `restartRequired` flag.

use lotu_app::dto::{SettingsDto, VideoSettingsDto};
use lotu_domain::VideoConfig;
use tauri::State;

use crate::commands::error::{ErrorCode, ErrorDto};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsDto {
    snapshot(state.inner())
}

#[tauri::command]
pub fn save_settings(
    video: VideoSettingsDto,
    state: State<'_, AppState>,
) -> Result<SettingsDto, ErrorDto> {
    save_impl(video, state.inner())
}

fn save_impl(video: VideoSettingsDto, state: &AppState) -> Result<SettingsDto, ErrorDto> {
    let config = VideoConfig::new(video.max_size, video.max_fps, video.bit_rate)
        .map_err(|error| ErrorDto::new(ErrorCode::SettingsInvalid, error.to_string()))?;
    let data = state.settings.save(config).map_err(ErrorDto::from)?;
    Ok(SettingsDto {
        video: data.video.into(),
        restart_required: state.session_serial().is_some(),
        onboarding_seen: data.onboarding_seen,
    })
}

#[tauri::command]
pub fn mark_onboarding_seen(state: State<'_, AppState>) -> Result<SettingsDto, ErrorDto> {
    let data = state
        .settings
        .mark_onboarding_seen()
        .map_err(ErrorDto::from)?;
    Ok(snapshot_with(&data, false))
}

fn snapshot(state: &AppState) -> SettingsDto {
    let data = state.settings.get();
    snapshot_with(&data, false)
}

fn snapshot_with(data: &lotu_app::ports::SettingsData, restart_required: bool) -> SettingsDto {
    SettingsDto {
        video: data.video.into(),
        restart_required,
        onboarding_seen: data.onboarding_seen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn state() -> AppState {
        crate::test_support::app_state()
    }

    #[test]
    fn defaults_are_the_m1_pipeline() {
        let state = state();
        let dto = snapshot(&state);
        assert_eq!(dto.video.max_size, 0);
        assert_eq!(dto.video.max_fps, 60);
        assert_eq!(dto.video.bit_rate, 8_000_000);
        assert!(!dto.restart_required);
    }

    #[test]
    fn saves_valid_settings() {
        let state = state();
        let result = save_impl(
            VideoSettingsDto {
                max_size: 1280,
                max_fps: 30,
                bit_rate: 4_000_000,
            },
            &state,
        )
        .expect("save");
        assert_eq!(result.video.max_size, 1280);
        assert_eq!(state.settings.video_config().max_fps, 30);
    }

    #[test]
    fn rejects_invalid_settings() {
        let state = state();
        let result = save_impl(
            VideoSettingsDto {
                max_size: 0,
                max_fps: 60,
                bit_rate: 0,
            },
            &state,
        );
        assert_eq!(
            result.expect_err("must fail").code,
            ErrorCode::SettingsInvalid
        );
    }
}
