//! M3 commands: native PNG screenshots and MP4 recording.
//!
//! File paths come from the native Save dialog (`tauri-plugin-dialog`). The
//! WebView never sees a file path it did not choose, and no data leaves the
//! machine.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use lotu_app::dto::{RecordingDto, RecordingStateDto, ScreenshotDto};
use lotu_app::ports::InputMessage;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::commands::error::{ErrorCode, ErrorDto};
use crate::state::AppState;

/// Emits the current recording slot so the UI can correct its local state when
/// the backend finalizes a file on session stop.
pub fn emit_recording_state(app: &AppHandle, state: &AppState) {
    let payload = RecordingStateDto {
        active: state.recording.is_active(),
        path: state.recording.output_path().as_deref().map(path_text),
    };
    let _ = app.emit("recording-state", payload);
}

/// Captures the active device at native resolution and writes a PNG.
#[tauri::command]
pub async fn capture_screenshot(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ScreenshotDto, ErrorDto> {
    let serial = state
        .session_serial()
        .ok_or_else(|| ErrorDto::new(ErrorCode::SessionInactive, "no live session is active"))?;
    let capture = state.capture.capture(&parse_serial(&serial)?).await?;

    let suggested = format!("lotu-capture-{}.png", file_stamp());
    let chosen = app
        .dialog()
        .file()
        .set_title("Guardar captura")
        .add_filter("Imagen PNG", &["png"])
        .set_file_name(suggested)
        .blocking_save_file()
        .ok_or_else(|| ErrorDto::new(ErrorCode::UserCancelled, "save dialog was cancelled"))?;
    let path = chosen
        .into_path()
        .map_err(|error| ErrorDto::new(ErrorCode::IoFailed, error.to_string()))?;
    let path = ensure_extension(path, "png");
    std::fs::write(&path, &capture.bytes).map_err(io_error)?;

    Ok(ScreenshotDto {
        path: path_text(&path),
        width: capture.width,
        height: capture.height,
    })
}

/// Asks for a destination and starts recording the live stream.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RecordingDto, ErrorDto> {
    if state.session_serial().is_none() {
        return Err(ErrorDto::new(
            ErrorCode::SessionInactive,
            "no live session is active",
        ));
    }
    // A stale slot (e.g. a failed session or a lost event) must not block a
    // new recording: finalize it best-effort and notify the UI.
    if state.recording.is_active() {
        let _ = state.recording.stop();
        emit_recording_state(&app, &state);
    }

    let suggested = format!("lotu-recording-{}.mp4", file_stamp());
    let chosen = app
        .dialog()
        .file()
        .set_title("Guardar grabación")
        .add_filter("Vídeo MP4", &["mp4"])
        .set_file_name(suggested)
        .blocking_save_file()
        .ok_or_else(|| ErrorDto::new(ErrorCode::UserCancelled, "save dialog was cancelled"))?;
    let path = chosen
        .into_path()
        .map_err(|error| ErrorDto::new(ErrorCode::IoFailed, error.to_string()))?;
    let path = ensure_extension(path, "mp4");

    state.recording.start(path.clone())?;

    // Force a fresh IDR right after the tap is armed, so the MP4 does not
    // begin with P-frames (black screen / artefacts) or wait a whole keyframe
    // interval before it has usable samples.
    if let Some(input) = state.session_input() {
        if let Err(error) = input.send(InputMessage::ResetVideo).await {
            let _ = state.recording.stop();
            return Err(ErrorDto::from(error));
        }
    }

    emit_recording_state(&app, &state);
    Ok(RecordingDto {
        path: path_text(&path),
        frames: 0,
        duration_us: 0,
        segments: 1,
        paths: vec![path_text(&path)],
    })
}

/// Finalizes the MP4 file. Idempotent when nothing was being recorded.
#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RecordingDto, ErrorDto> {
    let info = state
        .recording
        .stop()?
        .ok_or_else(|| ErrorDto::new(ErrorCode::RecordingInactive, "no recording is active"))?;
    emit_recording_state(&app, &state);
    let paths = if info.stats.paths.is_empty() {
        vec![path_text(&info.path)]
    } else {
        info.stats
            .paths
            .iter()
            .map(|path| path_text(path))
            .collect()
    };
    Ok(RecordingDto {
        path: path_text(&info.path),
        frames: info.stats.frames,
        duration_us: info.stats.duration_us,
        segments: info.stats.segments.max(1),
        paths,
    })
}

/// Current recording slot, used by the ViewModel on session changes.
#[tauri::command]
pub fn recording_state(state: State<'_, AppState>) -> RecordingStateDto {
    RecordingStateDto {
        active: state.recording.is_active(),
        path: state.recording.output_path().as_deref().map(path_text),
    }
}

fn parse_serial(raw: &str) -> Result<lotu_domain::Serial, ErrorDto> {
    lotu_domain::Serial::new(raw).map_err(|_| {
        ErrorDto::new(
            ErrorCode::DeviceNotFound,
            format!("invalid active device serial {raw:?}"),
        )
    })
}

fn ensure_extension(path: PathBuf, extension: &str) -> PathBuf {
    match path.extension() {
        Some(ext) if !ext.is_empty() => path,
        _ => path.with_extension(extension),
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn io_error(error: std::io::Error) -> ErrorDto {
    ErrorDto::new(ErrorCode::IoFailed, error.to_string())
}

/// `YYYYMMDD-HHMMSS` in UTC from the system clock, for default file names.
fn file_stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        time / 3600,
        (time % 3600) / 60,
        time % 60
    )
}

/// Howard Hinnant's days-from-civil inverse (proleptic Gregorian).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_has_a_stable_shape() {
        let stamp = file_stamp();
        assert_eq!(stamp.len(), 15, "{stamp}");
        assert_eq!(&stamp[8..9], "-");
        assert!(stamp.chars().all(|c| c.is_ascii_digit() || c == '-'));
    }

    #[test]
    fn civil_dates_are_chronological() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_358), (2025, 9, 27));
        assert_eq!(civil_from_days(20_714), (2026, 9, 18));
    }

    #[test]
    fn adds_a_missing_extension() {
        assert_eq!(
            ensure_extension(PathBuf::from("clip"), "mp4"),
            PathBuf::from("clip.mp4")
        );
        assert_eq!(
            ensure_extension(PathBuf::from("clip.MP4"), "mp4"),
            PathBuf::from("clip.MP4")
        );
    }
}
