use std::{path::Path, time::Instant};

use reqwest::Client;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::{
    sync::watch,
    time::{sleep, Duration},
};

use crate::{
    capture::remove_capture_artifacts,
    db::{get_unprocessed, mark_processed, update_description},
    models::VlmBatchProgress,
    state::AppState,
    vlm::{
        inference::describe_screenshot,
        server::{default_thread_count, refresh_vlm_state, update_vlm_state, VlmError},
    },
};

#[derive(Debug, Clone)]
struct BatchOptions {
    auto_delete: bool,
    model_path: Option<String>,
    mmproj_path: Option<String>,
    n_threads: usize,
}

#[derive(Debug, Clone, Serialize)]
struct BatchResult {
    total: usize,
    completed: usize,
    failed: usize,
    cancelled: bool,
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
    {
        let vlm_state = state.vlm_state.lock().await;
        if vlm_state.batch_running {
            return Ok(false);
        }
    }

    let auto_delete = match auto_delete {
        Some(value) => value,
        None => state.config.lock().await.auto_delete_images,
    };
    let requested_concurrency = max_concurrency.unwrap_or(1).max(1);
    if requested_concurrency > 1 {
        eprintln!(
            "requested batch concurrency {requested_concurrency}, but Kiroku currently runs a single inference worker"
        );
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

    let snapshot = update_vlm_state(state.inner(), None, Some(true), None).await;
    let _ = app.emit("vlm-status", &snapshot);

    let options = BatchOptions {
        auto_delete,
        model_path,
        mmproj_path,
        n_threads: n_threads.unwrap_or_else(default_thread_count),
    };
    let batch_state = state.inner().clone();
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
    if let Err(error) = ensure_vlm_server_running(&state, &options).await {
        finish_batch(
            &app,
            &state,
            BatchResult {
                total: 0,
                completed: 0,
                failed: 0,
                cancelled: false,
            },
            Some(error.to_string()),
        )
        .await;
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
                },
                Some(error.to_string()),
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
            },
            None,
        )
        .await;
        return;
    }

    let client = Client::new();
    let max_tokens = state.config.lock().await.vlm_max_tokens;
    let server_url = {
        let server = state.vlm_server.lock().await;
        server.server_url()
    };

    let mut completed = 0;
    let mut failed = 0;
    let mut elapsed_times = Vec::new();

    for record in unprocessed {
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
                },
                Some(error.to_string()),
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
                continue;
            }
        };

        let started_at = Instant::now();
        match describe_screenshot(&client, Path::new(&image_path), &server_url, max_tokens).await {
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
                    }
                    Err(error) => {
                        failed += 1;
                        eprintln!("failed to update VLM result for {}: {error}", record.id);
                    }
                }
            }
            Err(error) => {
                failed += 1;
                eprintln!("failed to infer VLM description for {}: {error}", record.id);
            }
        }
    }

    let cancelled = should_stop(&cancel_rx);
    let result = BatchResult {
        total,
        completed,
        failed,
        cancelled,
    };

    finish_batch(&app, &state, result, None).await;
}

async fn ensure_vlm_server_running(
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

    let mut server = state.vlm_server.lock().await;
    server
        .start(
            Path::new(model_path),
            Path::new(mmproj_path),
            options.n_threads,
        )
        .await
}

async fn finish_batch(
    app: &AppHandle,
    state: &AppState,
    result: BatchResult,
    last_error: Option<String>,
) {
    clear_batch_controls(state).await;
    let snapshot = update_vlm_state(state, None, Some(false), last_error).await;
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
