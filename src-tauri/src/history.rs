use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    db::{
        count_search_captures, list_capture_apps, search_captures as search_capture_records,
        CaptureAppGroup,
    },
    state::AppState,
};

const HISTORY_PAGE_SIZE: u64 = 50;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchCapturesRequest {
    pub query: Option<String>,
    pub app_filter: Option<Vec<String>>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub page: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryCaptureRecord {
    pub id: String,
    pub timestamp: String,
    pub capture_date: String,
    pub app: String,
    pub window_title: String,
    pub description: Option<String>,
    pub vlm_processed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistorySearchResponse {
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub apps: Vec<CaptureAppGroup>,
    pub results: Vec<HistoryCaptureRecord>,
}

#[tauri::command]
pub async fn search_captures(
    state: State<'_, AppState>,
    request: SearchCapturesRequest,
) -> Result<HistorySearchResponse, String> {
    let page = request.page.unwrap_or(1).max(1);
    let db = state.db.lock().await;
    let apps = list_capture_apps(&db).map_err(|error| error.to_string())?;
    let total = count_search_captures(
        &db,
        request.query.as_deref(),
        request.app_filter.as_deref(),
        request.date_from.as_deref(),
        request.date_to.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    let results = search_capture_records(
        &db,
        request.query.as_deref(),
        request.app_filter.as_deref(),
        request.date_from.as_deref(),
        request.date_to.as_deref(),
        page,
        HISTORY_PAGE_SIZE,
    )
    .map_err(|error| error.to_string())?
    .into_iter()
    .map(|record| HistoryCaptureRecord {
        capture_date: record.timestamp.chars().take(10).collect(),
        id: record.id,
        timestamp: record.timestamp,
        app: record.app,
        window_title: record.window_title,
        description: record.description,
        vlm_processed: record.vlm_processed,
    })
    .collect();

    Ok(HistorySearchResponse {
        total,
        page,
        page_size: HISTORY_PAGE_SIZE,
        apps,
        results,
    })
}
