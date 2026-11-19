//! Tauri delivery adapter and composition root for LOTU Link.

pub mod commands;
mod composition;
pub mod state;
pub mod stream_ws;

use tauri::{Manager, RunEvent};

use crate::state::AppState;

pub fn run() {
    let app_state = composition::build_app_state();

    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::devices::list_devices,
            commands::replay::start_replay,
            commands::session::start_session,
            commands::session::stop_session,
            commands::input::send_pointer,
            commands::input::send_scroll,
            commands::input::send_key,
            commands::input::send_text,
            commands::input::send_back,
            commands::input::rotate,
            commands::input::paste_text,
            commands::input::copy_text,
            commands::capture::capture_screenshot,
            commands::capture::start_recording,
            commands::capture::stop_recording,
            commands::capture::recording_state
        ])
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(error) => {
            eprintln!("LOTU Link failed to start: {error}");
            std::process::exit(1);
        }
    };

    app.run(|app_handle, event| {
        // Both actors own a listener and a task that must not outlive the app
        // (`docs/m0-contract.md` 6.3, `docs/m1-contract.md` 4.6). The shutdown
        // path cannot await, so the live session is aborted (its `Drop` kills
        // the server process and schedules the remote cleanup, D8) and the
        // replay task is cancelled; the process exit drops the rest.
        if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.abort_session();
                state.abort_replay();
            }
        }
    });
}
