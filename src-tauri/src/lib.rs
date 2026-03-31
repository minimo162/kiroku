pub mod capture;
pub mod config;
pub mod dashboard;
pub mod db;
pub mod diff;
pub mod export;
pub mod history;
pub mod model_manager;
pub mod models;
pub mod preview;
pub mod recorder;
pub mod scheduler;
pub mod session;
pub mod state;
pub mod tray;
pub mod vlm;
pub mod window_meta;

use tauri::Manager;

use capture::{capture_now, cleanup_capture_storage};
use config::{get_config, save_config_command, select_data_dir, test_vlm_connection};
use dashboard::{clear_last_error, get_dashboard_snapshot, get_recent_captures_command, get_stats};
use export::{export_csv, list_export_options, preview_csv_export};
use history::search_captures;
use model_manager::{complete_setup, get_setup_status};
use preview::{
    get_capture_description_history, get_capture_preview_page, update_capture_description,
};
use recorder::{start_recording, stop_recording};
use scheduler::spawn_scheduler;
use state::AppState;
use tray::{handle_close_requested, setup_tray};
use vlm::batch::{cancel_vlm_batch, pause_vlm_batch, resume_vlm_batch, run_vlm_batch};
use vlm::server::{check_vlm_status, start_vlm_server, stop_vlm_server};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            let scheduler_state = state.clone();
            if let Err(error) = tauri::async_runtime::block_on(cleanup_capture_storage(&state)) {
                eprintln!("startup capture cleanup failed: {error}");
            }
            app.manage(state);
            setup_tray(app.handle())?;
            spawn_scheduler(app.handle().clone(), scheduler_state);
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, ShortcutState};

                let toggle_shortcut =
                    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyK);

                match tauri_plugin_global_shortcut::Builder::new()
                    .with_shortcuts([toggle_shortcut])
                {
                    Ok(builder) => {
                        if let Err(error) = app.handle().plugin(
                            builder
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
                        ) {
                            eprintln!("global-shortcut plugin init failed (continuing without shortcut): {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("global-shortcut registration failed (continuing without shortcut): {error}");
                    }
                }
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
            clear_last_error,
            get_recent_captures_command,
            get_dashboard_snapshot,
            get_config,
            save_config_command,
            select_data_dir,
            test_vlm_connection,
            get_setup_status,
            complete_setup,
            preview_csv_export,
            export_csv,
            list_export_options,
            search_captures,
            get_capture_preview_page,
            update_capture_description,
            get_capture_description_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
