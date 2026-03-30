use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, State};
use tokio::sync::watch;
use tokio::time::{interval, Duration, MissedTickBehavior};

use crate::{
    capture::{
        capture_output_dir, capture_primary_monitor, persist_capture_metadata,
        remove_capture_artifacts,
    },
    db::insert_capture,
    diff::{compute_dhash, has_significant_change},
    models::CaptureRecord,
    state::AppState,
    tray::update_recording_tray_state,
    window_meta::{get_active_window_metadata, WindowMetadata},
};

pub async fn recording_loop(app: AppHandle, state: AppState, mut stop_rx: watch::Receiver<bool>) {
    let interval_secs = {
        let config = state.config.lock().await;
        config.capture_interval_secs
    };

    let mut ticker = interval(Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if *stop_rx.borrow() {
                    break;
                }

                if let Err(error) = capture_and_process_frame(&app, &state).await {
                    eprintln!("recording loop capture failed: {error}");
                }
            }
            changed = stop_rx.changed() => {
                if changed.is_err() || *stop_rx.borrow() {
                    break;
                }
            }
        }
    }

    *state.is_recording.lock().await = false;
    let _ = update_recording_tray_state(&app, false);
    let _ = app.emit("recording-status", false);
}

pub async fn start_recording_inner(app: AppHandle, state: AppState) -> Result<bool, String> {
    {
        let is_recording = state.is_recording.lock().await;
        if *is_recording {
            return Ok(false);
        }
    }

    let (stop_tx, stop_rx) = watch::channel(false);

    {
        let mut sender = state.stop_signal.lock().await;
        *sender = Some(stop_tx);
    }

    {
        let mut is_recording = state.is_recording.lock().await;
        *is_recording = true;
    }

    let recording_state = state.clone();
    let task = tokio::spawn(recording_loop(app.clone(), recording_state, stop_rx));

    {
        let mut recording_task = state.recording_task.lock().await;
        *recording_task = Some(task);
    }

    let _ = update_recording_tray_state(&app, true);
    let _ = app.emit("recording-status", true);

    Ok(true)
}

pub async fn stop_recording_inner(app: AppHandle, state: AppState) -> Result<bool, String> {
    let stop_signal = { state.stop_signal.lock().await.clone() };
    if let Some(sender) = stop_signal {
        let _ = sender.send(true);
    }

    let task = { state.recording_task.lock().await.take() };
    if let Some(task) = task {
        let _ = task.await;
    }

    {
        let mut is_recording = state.is_recording.lock().await;
        *is_recording = false;
    }

    {
        let mut sender = state.stop_signal.lock().await;
        *sender = None;
    }

    let _ = update_recording_tray_state(&app, false);

    Ok(true)
}

pub async fn toggle_recording_inner(app: AppHandle, state: AppState) -> Result<bool, String> {
    let is_recording = *state.is_recording.lock().await;
    if is_recording {
        stop_recording_inner(app, state).await
    } else {
        start_recording_inner(app, state).await
    }
}

#[tauri::command]
pub async fn start_recording(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    start_recording_inner(app, state.inner().clone()).await
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    stop_recording_inner(app, state.inner().clone()).await
}

async fn capture_and_process_frame(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let base_dir = state.capture_base_dir().await;
    let output_dir = capture_output_dir(&base_dir).map_err(|err| err.to_string())?;
    let frame = capture_primary_monitor(&output_dir)
        .await
        .map_err(|err| err.to_string())?;
    let image_path = record_image_path(&frame.record)?;

    let image = image::open(&image_path).map_err(|err| err.to_string())?;
    let hash = compute_dhash(&image);
    let threshold = {
        let config = state.config.lock().await;
        config.dhash_threshold
    };

    let mut previous_dhash = state.previous_dhash.lock().await;
    if let Some(previous_hash) = *previous_dhash {
        if !has_significant_change(previous_hash, hash, threshold) {
            drop(previous_dhash);
            remove_capture_artifacts(&image_path).map_err(|err| err.to_string())?;

            let mut stats = state.capture_stats.lock().await;
            stats.total_captures += 1;
            stats.skipped_captures += 1;
            stats.last_capture_at = Some(frame.record.timestamp.clone());
            return Ok(());
        }
    }
    *previous_dhash = Some(hash);
    drop(previous_dhash);

    let metadata = get_active_window_metadata().unwrap_or_else(|_| WindowMetadata::unknown(0));
    let record = enrich_record(frame.record, metadata, hash);
    persist_capture_metadata(&record, &image_path).map_err(|err| err.to_string())?;

    {
        let db = state.db.lock().await;
        insert_capture(&db, &record).map_err(|err| err.to_string())?;
    }

    {
        let mut stats = state.capture_stats.lock().await;
        stats.total_captures += 1;
        stats.effective_captures += 1;
        stats.last_capture_at = Some(record.timestamp.clone());
    }

    let _ = app.emit("capture-added", &record);

    Ok(())
}

fn enrich_record(mut record: CaptureRecord, metadata: WindowMetadata, dhash: u64) -> CaptureRecord {
    record.app = metadata.process_name;
    record.window_title = metadata.window_title;
    record.dhash = Some(format!("{dhash:016x}"));
    record
}

fn record_image_path(record: &CaptureRecord) -> Result<PathBuf, String> {
    record
        .image_path
        .as_deref()
        .map(Path::new)
        .map(Path::to_path_buf)
        .ok_or_else(|| "capture record does not contain an image path".to_string())
}
