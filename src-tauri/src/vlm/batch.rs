use std::{path::Path, time::Instant};

use reqwest::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_notification::NotificationExt;
use tokio::{
    sync::watch,
    time::{sleep, Duration},
};

use crate::{
    capture::remove_capture_artifacts,
    db::{
        clear_image_path, get_unprocessed, get_unprocessed_sessions, mark_processed,
        mark_session_processed, update_description,
    },
    models::{AppConfig, VlmBatchProgress},
    session::process_pending_sessions,
    state::AppState,
    vlm::{
        inference::{describe_screenshot, PromptContext},
        server::{
            default_thread_count, refresh_vlm_state, resolve_model_paths, update_vlm_state,
            VlmError,
        },
    },
};

#[derive(Debug, Clone)]
struct BatchOptions {
    auto_delete: bool,
    vlm_engine: String,
    model_path: Option<String>,
    mmproj_path: Option<String>,
    n_threads: usize,
    stop_server_when_done: bool,
    notify_on_completion: bool,
    include_active_session: bool,
}

#[derive(Debug, Clone)]
pub struct RunBatchRequest {
    pub auto_delete: Option<bool>,
    pub model_path: Option<String>,
    pub mmproj_path: Option<String>,
    pub n_threads: Option<usize>,
    pub max_concurrency: Option<usize>,
    pub stop_server_when_done: bool,
    pub notify_on_completion: bool,
    pub include_active_session: bool,
}

#[derive(Debug, Clone, Serialize)]
struct BatchResult {
    total: usize,
    completed: usize,
    failed: usize,
    cancelled: bool,
    error: Option<String>,
}

#[tauri::command]
pub async fn run_vlm_batch(
    app: AppHandle,
    state: State<'_, AppState>,
    auto_delete: Option<bool>,
    model_path: Option<String>,
    mmproj_path: Option<String>,
    n_threads: Option<usize>,
    max_concurrency: Option<usize>,
) -> Result<bool, String> {
    run_vlm_batch_inner(
        app,
        state.inner().clone(),
        RunBatchRequest {
            auto_delete,
            model_path,
            mmproj_path,
            n_threads,
            max_concurrency,
            stop_server_when_done: false,
            notify_on_completion: false,
            include_active_session: true,
        },
    )
    .await
}

pub async fn run_vlm_batch_inner(
    app: AppHandle,
    state: AppState,
    request: RunBatchRequest,
) -> Result<bool, String> {
    {
        let vlm_state = state.vlm_state.lock().await;
        if vlm_state.batch_running {
            return Ok(false);
        }
    }

    let config_snapshot = state.config.lock().await.clone();
    let auto_delete = match request.auto_delete {
        Some(value) => value,
        None => config_snapshot.auto_delete_images,
    };
    let vlm_engine = config_snapshot.vlm_engine.clone();
    let requested_concurrency = request.max_concurrency.unwrap_or(1).max(1);
    if requested_concurrency > 1 {
        eprintln!(
            "requested batch concurrency {requested_concurrency}, but Kiroku currently runs a single inference worker"
        );
    }

    let mut model_path = request.model_path;
    let mut mmproj_path = request.mmproj_path;
    let server_running = { state.vlm_state.lock().await.server_running };
    if vlm_engine != "copilot" && !server_running && (model_path.is_none() || mmproj_path.is_none())
    {
        if let Some((resolved_model_path, resolved_mmproj_path)) =
            resolve_model_paths(&state.app_paths)
        {
            model_path.get_or_insert_with(|| resolved_model_path.to_string_lossy().into_owned());
            mmproj_path.get_or_insert_with(|| resolved_mmproj_path.to_string_lossy().into_owned());
        }
    }

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let (pause_tx, pause_rx) = watch::channel(false);

    {
        let mut stop_signal = state.vlm_batch_stop_signal.lock().await;
        *stop_signal = Some(cancel_tx);
    }
    {
        let mut pause_signal = state.vlm_batch_pause_signal.lock().await;
        *pause_signal = Some(pause_tx);
    }

    let snapshot = update_vlm_state(&state, None, Some(true), None).await;
    let _ = app.emit("vlm-status", &snapshot);

    let options = BatchOptions {
        auto_delete,
        vlm_engine,
        model_path,
        mmproj_path,
        n_threads: request.n_threads.unwrap_or_else(default_thread_count),
        stop_server_when_done: request.stop_server_when_done,
        notify_on_completion: request.notify_on_completion,
        include_active_session: request.include_active_session,
    };
    let batch_state = state.clone();
    let batch_app = app.clone();
    let task = tokio::spawn(async move {
        vlm_batch_loop(batch_app, batch_state, cancel_rx, pause_rx, options).await;
    });

    {
        let mut batch_task = state.vlm_batch_task.lock().await;
        *batch_task = Some(task);
    }

    Ok(true)
}

#[tauri::command]
pub async fn cancel_vlm_batch(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    if let Some(sender) = state.vlm_batch_stop_signal.lock().await.clone() {
        let _ = sender.send(true);
    }

    let task = { state.vlm_batch_task.lock().await.take() };
    if let Some(task) = task {
        let _ = task.await;
    }

    clear_batch_controls(state.inner()).await;
    let snapshot = update_vlm_state(state.inner(), None, Some(false), None).await;
    let _ = app.emit("vlm-status", &snapshot);

    Ok(true)
}

#[tauri::command]
pub async fn pause_vlm_batch(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let sender = state.vlm_batch_pause_signal.lock().await.clone();
    let Some(sender) = sender else {
        return Ok(false);
    };

    sender.send(true).map_err(|error| error.to_string())?;
    let snapshot = refresh_vlm_state(state.inner()).await;
    let _ = app.emit("vlm-status", &snapshot);
    Ok(true)
}

#[tauri::command]
pub async fn resume_vlm_batch(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let sender = state.vlm_batch_pause_signal.lock().await.clone();
    let Some(sender) = sender else {
        return Ok(false);
    };

    sender.send(false).map_err(|error| error.to_string())?;
    let snapshot = refresh_vlm_state(state.inner()).await;
    let _ = app.emit("vlm-status", &snapshot);
    Ok(true)
}

async fn vlm_batch_loop(
    app: AppHandle,
    state: AppState,
    mut cancel_rx: watch::Receiver<bool>,
    mut pause_rx: watch::Receiver<bool>,
    options: BatchOptions,
) {
    if let Err(error) = ensure_engine_running(&app, &state, &options).await {
        finish_batch(
            &app,
            &state,
            BatchResult {
                total: 0,
                completed: 0,
                failed: 0,
                cancelled: false,
                error: None,
            },
            Some(error.to_string()),
            &options,
        )
        .await;
        return;
    }

    let config = state.config.lock().await.clone();
    if options.vlm_engine == "copilot" && config.session_enabled {
        run_session_batch_loop(app, state, cancel_rx, pause_rx, options, config).await;
        return;
    }

    let unprocessed = {
        let db = state.db.lock().await;
        get_unprocessed(&db)
    };
    let unprocessed = match unprocessed {
        Ok(records) => records,
        Err(error) => {
            finish_batch(
                &app,
                &state,
                BatchResult {
                    total: 0,
                    completed: 0,
                    failed: 0,
                    cancelled: false,
                    error: None,
                },
                Some(error.to_string()),
                &options,
            )
            .await;
            return;
        }
    };

    let total = unprocessed.len();
    let progress = VlmBatchProgress {
        total,
        completed: 0,
        failed: 0,
        current_id: None,
        estimated_remaining_secs: if total == 0 { Some(0) } else { None },
    };
    emit_progress(&app, &state, progress).await;

    if total == 0 {
        finish_batch(
            &app,
            &state,
            BatchResult {
                total: 0,
                completed: 0,
                failed: 0,
                cancelled: false,
                error: None,
            },
            None,
            &options,
        )
        .await;
        return;
    }

    let client = Client::new();
    let config = state.config.lock().await.clone();
    let max_tokens = config.vlm_max_tokens;
    let server_url = if options.vlm_engine == "copilot" {
        state.copilot_server.lock().await.server_url()
    } else {
        state.vlm_server.lock().await.server_url()
    };

    let mut completed = 0;
    let mut failed = 0;
    let mut elapsed_times = Vec::new();
    let mut server_restarted = false;
    let mut index = 0;

    while index < unprocessed.len() {
        let record = &unprocessed[index];
        if should_stop(&cancel_rx) {
            break;
        }

        if let Err(error) = wait_if_paused(&mut cancel_rx, &mut pause_rx).await {
            finish_batch(
                &app,
                &state,
                BatchResult {
                    total,
                    completed,
                    failed,
                    cancelled: true,
                    error: None,
                },
                Some(error.to_string()),
                &options,
            )
            .await;
            return;
        }

        let progress = VlmBatchProgress {
            total,
            completed,
            failed,
            current_id: Some(record.id.clone()),
            estimated_remaining_secs: estimate_remaining_secs(&elapsed_times, total, completed),
        };
        emit_progress(&app, &state, progress).await;

        let image_path = match record.image_path.as_deref() {
            Some(path) => path.to_string(),
            None => {
                failed += 1;
                index += 1;
                continue;
            }
        };

        let started_at = Instant::now();
        match describe_screenshot(
            &client,
            Path::new(&image_path),
            &server_url,
            max_tokens,
            PromptContext {
                app: Some(&record.app),
                window_title: Some(&record.window_title),
                system_prompt: Some(&config.system_prompt),
                user_prompt: Some(&config.user_prompt),
            },
        )
        .await
        {
            Ok(description) => {
                let update_result = {
                    let db = state.db.lock().await;
                    update_description(&db, &record.id, &description)
                        .and_then(|_| mark_processed(&db, &record.id))
                };

                match update_result {
                    Ok(()) => {
                        if options.auto_delete {
                            let _ = remove_capture_artifacts(Path::new(&image_path));
                        }
                        completed += 1;
                        elapsed_times.push(started_at.elapsed().as_secs());
                        index += 1;
                    }
                    Err(error) => {
                        failed += 1;
                        eprintln!("failed to update VLM result for {}: {error}", record.id);
                        index += 1;
                    }
                }
            }
            Err(error) => {
                if matches!(&error, VlmError::Http(_)) {
                    let health = if options.vlm_engine == "copilot" {
                        let server = state.copilot_server.lock().await;
                        server.health_check().await
                    } else {
                        let server = state.vlm_server.lock().await;
                        server.health_check().await
                    };

                    if health.is_err() {
                        let engine_name = if options.vlm_engine == "copilot" {
                            "Copilot"
                        } else {
                            "llama-server"
                        };
                        if server_restarted {
                            eprintln!(
                                "{engine_name} appears to have crashed again during batch processing"
                            );
                            finish_batch(
                                &app,
                                &state,
                                BatchResult {
                                    total,
                                    completed,
                                    failed,
                                    cancelled: false,
                                    error: None,
                                },
                                Some(format!(
                                    "分析エンジンが再起動後も停止しました。{} を確認してください。",
                                    if options.vlm_engine == "copilot" {
                                        "copilot-server.log"
                                    } else {
                                        "llama-server.log"
                                    }
                                )),
                                &options,
                            )
                            .await;
                            return;
                        }

                        eprintln!("{engine_name} appears to have crashed, attempting restart...");
                        if let Err(restart_error) =
                            ensure_engine_running(&app, &state, &options).await
                        {
                            eprintln!("failed to restart {engine_name}: {restart_error}");
                            finish_batch(
                                &app,
                                &state,
                                BatchResult {
                                    total,
                                    completed,
                                    failed,
                                    cancelled: false,
                                    error: None,
                                },
                                Some(restart_error.to_string()),
                                &options,
                            )
                            .await;
                            return;
                        }

                        server_restarted = true;
                        continue;
                    }
                }

                failed += 1;
                eprintln!("failed to infer VLM description for {}: {error}", record.id);
                index += 1;
            }
        }
    }

    let cancelled = should_stop(&cancel_rx);
    let result = BatchResult {
        total,
        completed,
        failed,
        cancelled,
        error: None,
    };

    finish_batch(&app, &state, result, None, &options).await;
}

async fn run_session_batch_loop(
    app: AppHandle,
    state: AppState,
    mut cancel_rx: watch::Receiver<bool>,
    mut pause_rx: watch::Receiver<bool>,
    options: BatchOptions,
    config: AppConfig,
) {
    {
        let db = state.db.lock().await;
        let session_result = if options.include_active_session {
            crate::session::process_pending_sessions_including_active(&db, &config)
        } else {
            process_pending_sessions(&db, &config)
        };
        if let Err(error) = session_result {
            finish_batch(
                &app,
                &state,
                BatchResult {
                    total: 0,
                    completed: 0,
                    failed: 0,
                    cancelled: false,
                    error: None,
                },
                Some(error.to_string()),
                &options,
            )
            .await;
            return;
        }
    }

    let unprocessed = {
        let db = state.db.lock().await;
        match get_unprocessed_sessions(&db) {
            Ok(sessions) => sessions,
            Err(error) => {
                finish_batch(
                    &app,
                    &state,
                    BatchResult {
                        total: 0,
                        completed: 0,
                        failed: 0,
                        cancelled: false,
                        error: None,
                    },
                    Some(error.to_string()),
                    &options,
                )
                .await;
                return;
            }
        }
    };

    let total = unprocessed.len();
    emit_progress(
        &app,
        &state,
        VlmBatchProgress {
            total,
            completed: 0,
            failed: 0,
            current_id: None,
            estimated_remaining_secs: if total == 0 { Some(0) } else { None },
        },
    )
    .await;

    if total == 0 {
        finish_batch(
            &app,
            &state,
            BatchResult {
                total: 0,
                completed: 0,
                failed: 0,
                cancelled: false,
                error: None,
            },
            None,
            &options,
        )
        .await;
        return;
    }

    let client = reqwest::Client::new();
    let server_url = state.copilot_server.lock().await.server_url();
    let mut completed = 0;
    let mut failed = 0;
    let mut elapsed_times = Vec::new();

    for session in &unprocessed {
        if should_stop(&cancel_rx) {
            break;
        }
        if let Err(error) = wait_if_paused(&mut cancel_rx, &mut pause_rx).await {
            finish_batch(
                &app,
                &state,
                BatchResult {
                    total,
                    completed,
                    failed,
                    cancelled: true,
                    error: None,
                },
                Some(error.to_string()),
                &options,
            )
            .await;
            return;
        }

        emit_progress(
            &app,
            &state,
            VlmBatchProgress {
                total,
                completed,
                failed,
                current_id: Some(session.id.clone()),
                estimated_remaining_secs: estimate_remaining_secs(&elapsed_times, total, completed),
            },
        )
        .await;

        let collage_path = match session.collage_path.as_deref() {
            Some(path) => path.to_string(),
            None => {
                failed += 1;
                continue;
            }
        };

        let duration_min = calc_duration_min(&session.start_time, &session.end_time);
        let user_prompt = config
            .session_user_prompt
            .replace("{start_time}", &format_time_hhmm(&session.start_time))
            .replace("{end_time}", &format_time_hhmm(&session.end_time))
            .replace("{duration_min}", &duration_min.to_string())
            .replace("{frame_count}", &session.frame_count.to_string());

        let started_at = Instant::now();
        let result = describe_screenshot(
            &client,
            Path::new(&collage_path),
            &server_url,
            config.vlm_max_tokens,
            PromptContext {
                app: None,
                window_title: None,
                system_prompt: Some(&config.system_prompt),
                user_prompt: Some(&user_prompt),
            },
        )
        .await;

        match result {
            Ok(description) => {
                let db = state.db.lock().await;
                let _ = mark_session_processed(&db, &session.id, &description);

                if let Ok(capture_ids) = get_capture_ids_for_session(&db, &session.id) {
                    for capture_id in &capture_ids {
                        let _ = update_description(&db, capture_id, &description);
                        let _ = mark_processed(&db, capture_id);
                    }

                    if config.auto_delete_images {
                        for capture_id in &capture_ids {
                            if let Ok(Some(image_path)) = get_capture_image_path(&db, capture_id) {
                                let _ = std::fs::remove_file(&image_path);
                                let _ = clear_image_path(&db, capture_id);
                            }
                        }
                    }
                }

                elapsed_times.push(started_at.elapsed().as_secs());
                completed += 1;
            }
            Err(error) => {
                eprintln!("session {} description failed: {}", session.id, error);
                failed += 1;
            }
        }
    }

    finish_batch(
        &app,
        &state,
        BatchResult {
            total,
            completed,
            failed,
            cancelled: should_stop(&cancel_rx),
            error: None,
        },
        None,
        &options,
    )
    .await;
}

async fn ensure_engine_running(
    app: &AppHandle,
    state: &AppState,
    options: &BatchOptions,
) -> Result<(), VlmError> {
    if options.vlm_engine == "copilot" {
        let data_dir = state.app_paths.data_dir.clone();
        let snapshot = refresh_vlm_state(state).await;
        if snapshot.server_running {
            return Ok(());
        }

        let mut server = state.copilot_server.lock().await;
        server.start(&data_dir).await?;
        drop(server);

        let snapshot = update_vlm_state(state, Some(true), None, None).await;
        let _ = app.emit("vlm-status", &snapshot);
        Ok(())
    } else {
        ensure_llama_server_running(app, state, options).await
    }
}

async fn ensure_llama_server_running(
    app: &AppHandle,
    state: &AppState,
    options: &BatchOptions,
) -> Result<(), VlmError> {
    let snapshot = refresh_vlm_state(state).await;
    if snapshot.server_running {
        return Ok(());
    }

    let Some(model_path) = options.model_path.as_deref() else {
        return Err(VlmError::InvalidResponse(
            "VLM server is not running and model_path was not provided".to_string(),
        ));
    };
    let Some(mmproj_path) = options.mmproj_path.as_deref() else {
        return Err(VlmError::InvalidResponse(
            "VLM server is not running and mmproj_path was not provided".to_string(),
        ));
    };

    let data_dir = state.app_paths.data_dir.clone();
    let mut server = state.vlm_server.lock().await;
    server
        .start(
            Path::new(model_path),
            Path::new(mmproj_path),
            &data_dir,
            options.n_threads,
        )
        .await?;
    drop(server);

    let snapshot = update_vlm_state(state, Some(true), None, None).await;
    let _ = app.emit("vlm-status", &snapshot);
    Ok(())
}

async fn finish_batch(
    app: &AppHandle,
    state: &AppState,
    mut result: BatchResult,
    last_error: Option<String>,
    options: &BatchOptions,
) {
    clear_batch_controls(state).await;
    let stop_error = if options.stop_server_when_done {
        let result = if options.vlm_engine == "copilot" {
            let mut server = state.copilot_server.lock().await;
            server.stop()
        } else {
            let mut server = state.vlm_server.lock().await;
            server.stop()
        };
        result.err().map(|error| error.to_string())
    } else {
        None
    };
    let last_error = last_error.or(stop_error);
    result.error = last_error.clone();
    let snapshot = update_vlm_state(
        state,
        options.stop_server_when_done.then_some(false),
        Some(false),
        last_error.clone(),
    )
    .await;
    let _ = app.emit("vlm-status", &snapshot);
    emit_progress(
        app,
        state,
        VlmBatchProgress {
            total: result.total,
            completed: result.completed,
            failed: result.failed,
            current_id: None,
            estimated_remaining_secs: Some(0),
        },
    )
    .await;
    let _ = app.emit("vlm-batch-complete", &result);

    if options.notify_on_completion {
        let body = if let Some(error) = result.error.clone() {
            format!("夜間バッチの完了時にエラーが発生しました: {error}")
        } else if result.cancelled {
            format!(
                "夜間バッチを中断しました。完了 {} 件 / 失敗 {} 件",
                result.completed, result.failed
            )
        } else {
            format!(
                "夜間バッチが完了しました。完了 {} 件 / 失敗 {} 件",
                result.completed, result.failed
            )
        };

        let _ = app
            .notification()
            .builder()
            .title("Kiroku バッチ処理")
            .body(&body)
            .show();
    }
}

async fn clear_batch_controls(state: &AppState) {
    {
        let mut stop_signal = state.vlm_batch_stop_signal.lock().await;
        *stop_signal = None;
    }
    {
        let mut pause_signal = state.vlm_batch_pause_signal.lock().await;
        *pause_signal = None;
    }
    {
        let mut batch_task = state.vlm_batch_task.lock().await;
        *batch_task = None;
    }
}

async fn wait_if_paused(
    cancel_rx: &mut watch::Receiver<bool>,
    pause_rx: &mut watch::Receiver<bool>,
) -> Result<(), VlmError> {
    while *pause_rx.borrow() {
        if should_stop(cancel_rx) {
            return Err(VlmError::InvalidResponse(
                "VLM batch cancelled while paused".to_string(),
            ));
        }

        tokio::select! {
            changed = cancel_rx.changed() => {
                if changed.is_err() || should_stop(cancel_rx) {
                    return Err(VlmError::InvalidResponse(
                        "VLM batch cancelled while paused".to_string(),
                    ));
                }
            }
            changed = pause_rx.changed() => {
                if changed.is_err() {
                    sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }

    Ok(())
}

fn should_stop(cancel_rx: &watch::Receiver<bool>) -> bool {
    *cancel_rx.borrow()
}

fn get_capture_ids_for_session(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT id FROM captures WHERE session_id = ?1 ORDER BY timestamp ASC")?;
    let rows = stmt.query_map(rusqlite::params![session_id], |row| row.get(0))?;
    rows.collect()
}

fn get_capture_image_path(
    conn: &rusqlite::Connection,
    capture_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT image_path FROM captures WHERE id = ?1",
        rusqlite::params![capture_id],
        |row| row.get(0),
    )
}

fn calc_duration_min(start: &str, end: &str) -> i64 {
    use chrono::DateTime;

    let Ok(start) = DateTime::parse_from_rfc3339(start) else {
        return 0;
    };
    let Ok(end) = DateTime::parse_from_rfc3339(end) else {
        return 0;
    };

    (end - start).num_minutes().max(0)
}

fn format_time_hhmm(ts: &str) -> String {
    use chrono::DateTime;

    let Ok(timestamp) = DateTime::parse_from_rfc3339(ts) else {
        return ts.to_string();
    };
    timestamp.format("%H:%M").to_string()
}

fn estimate_remaining_secs(elapsed_times: &[u64], total: usize, completed: usize) -> Option<u64> {
    if elapsed_times.is_empty() {
        return None;
    }

    let average = elapsed_times.iter().sum::<u64>() / elapsed_times.len() as u64;
    Some(average * total.saturating_sub(completed) as u64)
}

async fn emit_progress(app: &AppHandle, state: &AppState, progress: VlmBatchProgress) {
    {
        let mut current = state.vlm_progress.lock().await;
        *current = progress.clone();
    }

    let _ = app.emit("vlm-progress", &progress);
}

#[cfg(test)]
mod tests {
    use super::estimate_remaining_secs;

    #[test]
    fn estimate_remaining_secs_uses_average_elapsed_time() {
        let remaining = estimate_remaining_secs(&[20, 40], 5, 2);
        assert_eq!(remaining, Some(90));
    }

    #[test]
    fn estimate_remaining_secs_returns_none_without_history() {
        let remaining = estimate_remaining_secs(&[], 5, 2);
        assert_eq!(remaining, None);
    }
}
