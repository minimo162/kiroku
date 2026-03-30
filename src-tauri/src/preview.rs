use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::{
    db::{
        count_captures_for_date, get_capture_by_id, get_captures_by_date, get_description_history,
        list_capture_dates, update_description_with_history, CaptureDateGroup,
        DescriptionHistoryRecord, StoredCaptureRecord,
    },
    state::AppState,
};

const PREVIEW_PAGE_SIZE: u64 = 50;

#[derive(Debug, Clone, Serialize)]
pub struct PreviewCaptureRecord {
    pub id: String,
    pub timestamp: String,
    pub app: String,
    pub window_title: String,
    pub image_path: Option<String>,
    pub image_exists: bool,
    pub description: Option<String>,
    pub dhash: Option<String>,
    pub vlm_processed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviewPagePayload {
    pub selected_date: Option<String>,
    pub available_dates: Vec<CaptureDateGroup>,
    pub total: u64,
    pub page: u64,
    pub page_size: u64,
    pub records: Vec<PreviewCaptureRecord>,
}

impl From<StoredCaptureRecord> for PreviewCaptureRecord {
    fn from(record: StoredCaptureRecord) -> Self {
        let image_exists = record
            .image_path
            .as_deref()
            .is_some_and(|path| Path::new(path).exists());

        Self {
            id: record.id,
            timestamp: record.timestamp,
            app: record.app,
            window_title: record.window_title,
            image_path: record.image_path,
            image_exists,
            description: record.description,
            dhash: record.dhash,
            vlm_processed: record.vlm_processed,
        }
    }
}

#[tauri::command]
pub async fn get_capture_preview_page(
    state: State<'_, AppState>,
    date: Option<String>,
    page: Option<u64>,
) -> Result<PreviewPagePayload, String> {
    let db = state.db.lock().await;
    let available_dates = list_capture_dates(&db).map_err(|error| error.to_string())?;
    let selected_date = date
        .filter(|value| !value.trim().is_empty())
        .or_else(|| available_dates.first().map(|entry| entry.date.clone()));

    let Some(selected_date) = selected_date else {
        return Ok(PreviewPagePayload {
            selected_date: None,
            available_dates,
            total: 0,
            page: 1,
            page_size: PREVIEW_PAGE_SIZE,
            records: Vec::new(),
        });
    };

    let total = count_captures_for_date(&db, &selected_date).map_err(|error| error.to_string())?;
    let max_page = if total == 0 {
        1
    } else {
        ((total - 1) / PREVIEW_PAGE_SIZE) + 1
    };
    let page = page.unwrap_or(1).max(1).min(max_page);
    let records = get_captures_by_date(&db, &selected_date, page, PREVIEW_PAGE_SIZE)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(PreviewCaptureRecord::from)
        .collect();

    Ok(PreviewPagePayload {
        selected_date: Some(selected_date),
        available_dates,
        total,
        page,
        page_size: PREVIEW_PAGE_SIZE,
        records,
    })
}

#[tauri::command]
pub async fn update_capture_description(
    state: State<'_, AppState>,
    capture_id: String,
    description: String,
) -> Result<PreviewCaptureRecord, String> {
    let db = state.db.lock().await;
    update_description_with_history(&db, &capture_id, Some(&description))
        .map_err(|error| error.to_string())?;
    let record = get_capture_by_id(&db, &capture_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "capture record was not found".to_string())?;

    Ok(PreviewCaptureRecord::from(record))
}

#[tauri::command]
pub async fn get_capture_description_history(
    state: State<'_, AppState>,
    capture_id: String,
) -> Result<Vec<DescriptionHistoryRecord>, String> {
    let db = state.db.lock().await;
    get_description_history(&db, &capture_id).map_err(|error| error.to_string())
}
