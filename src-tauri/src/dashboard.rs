use serde::Serialize;
use tauri::State;

use crate::{
    db::{count_processed_captures, get_recent_captures, StoredCaptureRecord},
    models::{CaptureStats, VlmBatchProgress, VlmState},
    state::AppState,
};

const DEFAULT_RECENT_CAPTURE_LIMIT: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct DashboardStats {
    pub total_captures: u64,
    pub effective_captures: u64,
    pub skipped_captures: u64,
    pub vlm_processed: u64,
    pub scheduler_enabled: bool,
    pub is_recording: bool,
    pub server_running: bool,
    pub batch_running: bool,
    pub next_batch_run_at: Option<String>,
    pub last_capture_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub stats: DashboardStats,
    pub vlm_progress: VlmBatchProgress,
    pub recent_captures: Vec<StoredCaptureRecord>,
}

#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> Result<DashboardStats, String> {
    build_dashboard_stats(state.inner()).await
}

#[tauri::command]
pub async fn get_recent_captures_command(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<StoredCaptureRecord>, String> {
    let db = state.db.lock().await;
    get_recent_captures(&db, limit.unwrap_or(DEFAULT_RECENT_CAPTURE_LIMIT))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_dashboard_snapshot(
    state: State<'_, AppState>,
) -> Result<DashboardSnapshot, String> {
    let stats = build_dashboard_stats(state.inner()).await?;
    let vlm_progress = { state.vlm_progress.lock().await.clone() };
    let recent_captures = {
        let db = state.db.lock().await;
        get_recent_captures(&db, DEFAULT_RECENT_CAPTURE_LIMIT).map_err(|error| error.to_string())?
    };

    Ok(DashboardSnapshot {
        stats,
        vlm_progress,
        recent_captures,
    })
}

async fn build_dashboard_stats(state: &AppState) -> Result<DashboardStats, String> {
    let capture_stats = { state.capture_stats.lock().await.clone() };
    let vlm_state = { state.vlm_state.lock().await.clone() };
    let config = { state.config.lock().await.clone() };
    let is_recording = { *state.is_recording.lock().await };
    let next_batch_run_at = { state.next_batch_run_at.lock().await.clone() };
    let vlm_processed = {
        let db = state.db.lock().await;
        count_processed_captures(&db).map_err(|error| error.to_string())?
    };

    Ok(merge_dashboard_stats(
        capture_stats,
        vlm_state,
        config.scheduler_enabled,
        is_recording,
        next_batch_run_at,
        vlm_processed,
    ))
}

fn merge_dashboard_stats(
    capture_stats: CaptureStats,
    vlm_state: VlmState,
    scheduler_enabled: bool,
    is_recording: bool,
    next_batch_run_at: Option<String>,
    vlm_processed: u64,
) -> DashboardStats {
    DashboardStats {
        total_captures: capture_stats.total_captures,
        effective_captures: capture_stats.effective_captures,
        skipped_captures: capture_stats.skipped_captures,
        vlm_processed,
        scheduler_enabled,
        is_recording,
        server_running: vlm_state.server_running,
        batch_running: vlm_state.batch_running,
        next_batch_run_at,
        last_capture_at: capture_stats.last_capture_at,
        last_error: vlm_state.last_error,
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{CaptureStats, VlmState};

    use super::merge_dashboard_stats;

    #[test]
    fn merge_dashboard_stats_preserves_runtime_and_vlm_fields() {
        let stats = merge_dashboard_stats(
            CaptureStats {
                total_captures: 12,
                effective_captures: 9,
                skipped_captures: 3,
                last_capture_at: Some("2026-04-01T10:00:00+09:00".to_string()),
            },
            VlmState {
                server_running: true,
                batch_running: false,
                last_error: Some("none".to_string()),
            },
            true,
            true,
            Some("2026-04-01T22:00:00+09:00".to_string()),
            7,
        );

        assert_eq!(stats.total_captures, 12);
        assert_eq!(stats.effective_captures, 9);
        assert_eq!(stats.skipped_captures, 3);
        assert_eq!(stats.vlm_processed, 7);
        assert!(stats.scheduler_enabled);
        assert!(stats.is_recording);
        assert!(stats.server_running);
        assert_eq!(
            stats.next_batch_run_at.as_deref(),
            Some("2026-04-01T22:00:00+09:00")
        );
        assert_eq!(stats.last_error.as_deref(), Some("none"));
    }
}
