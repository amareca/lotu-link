use lotu_app::dto::DeviceDto;
use tauri::State;

use crate::commands::error::ErrorDto;
use crate::state::AppState;

/// Lists the devices known to ADB. No business rules live here: the command
/// delegates to `DeviceService` and translates adapter errors to wire codes.
#[tauri::command]
pub async fn list_devices(state: State<'_, AppState>) -> Result<Vec<DeviceDto>, ErrorDto> {
    state.devices.list_devices().await.map_err(ErrorDto::from)
}
