pub mod capture;
pub mod config;
pub mod diff;
pub mod models;
pub mod recorder;
pub mod state;
pub mod window_meta;

use tauri::Manager;

use capture::capture_now;
use recorder::{start_recording, stop_recording};
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            capture_now,
            start_recording,
            stop_recording
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
