pub mod capture;
pub mod config;
pub mod dashboard;
pub mod db;
pub mod diff;
pub mod export;
pub mod models;
pub mod recorder;
pub mod state;
pub mod tray;
pub mod vlm;
pub mod window_meta;

use tauri::Manager;

use capture::capture_now;
use dashboard::{get_dashboard_snapshot, get_recent_captures_command, get_stats};
use export::{export_csv, preview_csv_export};
use recorder::{start_recording, stop_recording};
use state::AppState;
use tray::{handle_close_requested, setup_tray};
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
            setup_tray(app.handle())?;
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

                let toggle_shortcut =
                    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);

                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_shortcuts([toggle_shortcut])?
                        .with_handler(move |app, shortcut, event| {
                            if shortcut == &toggle_shortcut
                                && event.state() == ShortcutState::Pressed
                            {
                                if let Some(state) = app.try_state::<AppState>() {
                                    let app_handle = app.clone();
                                    let state = state.inner().clone();
                                    tauri::async_runtime::spawn(async move {
                                        let _ = crate::recorder::toggle_recording_inner(
                                            app_handle, state,
                                        )
                                        .await;
                                    });
                                }
                            }
                        })
                        .build(),
                )?;
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                handle_close_requested(&window.app_handle());
            }
            tauri::WindowEvent::Destroyed => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.shutdown_vlm_server_blocking();
                }
            }
            _ => {}
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
            get_stats,
            get_recent_captures_command,
            get_dashboard_snapshot,
            preview_csv_export,
            export_csv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
