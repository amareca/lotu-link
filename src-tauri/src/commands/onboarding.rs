//! M4 onboarding command: the real ADB RSA fingerprint.

use lotu_app::dto::RsaFingerprintDto;
use tauri::State;

use crate::commands::error::ErrorDto;
use crate::state::AppState;

#[tauri::command]
pub async fn get_rsa_fingerprint(
    state: State<'_, AppState>,
) -> Result<RsaFingerprintDto, ErrorDto> {
    // The fingerprint or its failure reason is part of the DTO: a missing ADB
    // key is an expected onboarding state, not a command error.
    Ok(match state.fingerprint.current().await {
        Ok(fingerprint) => RsaFingerprintDto {
            fingerprint: Some(fingerprint),
            error: None,
        },
        Err(error) => RsaFingerprintDto {
            fingerprint: None,
            error: Some(error.to_string()),
        },
    })
}
