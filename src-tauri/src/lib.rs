pub mod capture;
pub mod config;
pub mod db;
pub mod diff;
pub mod export;
pub mod models;
pub mod recorder;
pub mod state;
pub mod vlm;
pub mod window_meta;

use tauri::Manager;

use capture::capture_now;
use export::{export_csv, preview_csv_export};
use recorder::{start_recording, stop_recording};
use state::AppState;
use vlm::batch::{cancel_vlm_batch, pause_vlm_batch, resume_vlm_batch, run_vlm_batch};
use vlm::server::{check_vlm_status, start_vlm_server, stop_vlm_server};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.shutdown_vlm_server_blocking();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            capture_now,
            start_recording,
            stop_recording,
            check_vlm_status,
            start_vlm_server,
            stop_vlm_server,
            run_vlm_batch,
            cancel_vlm_batch,
            pause_vlm_batch,
            resume_vlm_batch,
            preview_csv_export,
            export_csv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
