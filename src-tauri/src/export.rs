use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use chrono::{Local, NaiveDate};
use csv::Writer;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use thiserror::Error;

use crate::db::{query_captures_filtered, DbError};
use crate::state::AppState;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExportFilter {
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub apps: Option<Vec<String>>,
    pub only_processed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportPreview {
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportResult {
    pub count: usize,
    pub path: String,
}

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("date must use YYYY-MM-DD format")]
    InvalidDate(#[from] chrono::ParseError),
    #[error("failed to read export data")]
    Db(#[from] DbError),
    #[error("failed to create export file")]
    Io(#[from] io::Error),
    #[error("failed to write CSV file")]
    Csv(#[from] csv::Error),
    #[error("save dialog returned a non-filesystem path")]
    InvalidSavePath,
}

pub fn export_to_csv(
    conn: &Connection,
    filter: &ExportFilter,
    output_path: &Path,
) -> Result<usize, ExportError> {
    validate_filter(filter)?;

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let records = query_captures_filtered(
        conn,
        filter.start_date.as_deref(),
        filter.end_date.as_deref(),
        filter.apps.as_deref(),
        filter.only_processed,
    )?;

    let mut file = File::create(output_path)?;
    file.write_all(&[0xEF, 0xBB, 0xBF])?;

    let mut writer = Writer::from_writer(file);
    writer.write_record(["timestamp", "app", "window_title", "description"])?;

    let count = records.len();
    for record in records {
        writer.write_record([
            record.timestamp,
            record.app,
            record.window_title,
            record.description.unwrap_or_default(),
        ])?;
    }

    writer.flush()?;
    Ok(count)
}

pub fn preview_export_count(
    conn: &Connection,
    filter: &ExportFilter,
) -> Result<ExportPreview, ExportError> {
    validate_filter(filter)?;

    let count = query_captures_filtered(
        conn,
        filter.start_date.as_deref(),
        filter.end_date.as_deref(),
        filter.apps.as_deref(),
        filter.only_processed,
    )?
    .len();

    Ok(ExportPreview { count })
}

#[tauri::command]
pub async fn preview_csv_export(
    state: State<'_, AppState>,
    filter: ExportFilter,
) -> Result<ExportPreview, String> {
    let db = state.db.lock().await;
    preview_export_count(&db, &filter).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn export_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    filter: ExportFilter,
    output_path: Option<String>,
) -> Result<Option<ExportResult>, String> {
    let output_path = match output_path {
        Some(path) => PathBuf::from(path),
        None => match app
            .dialog()
            .file()
            .set_title("CSV を保存")
            .set_file_name(&default_export_file_name())
            .add_filter("CSV", &["csv"])
            .blocking_save_file()
        {
            Some(path) => path
                .into_path()
                .map_err(|_| ExportError::InvalidSavePath.to_string())?,
            None => return Ok(None),
        },
    };

    let db = state.db.lock().await;
    let count = export_to_csv(&db, &filter, &output_path).map_err(|error| error.to_string())?;

    Ok(Some(ExportResult {
        count,
        path: output_path.to_string_lossy().into_owned(),
    }))
}

fn validate_filter(filter: &ExportFilter) -> Result<(), ExportError> {
    if let Some(start_date) = filter.start_date.as_deref() {
        NaiveDate::parse_from_str(start_date, "%Y-%m-%d")?;
    }
    if let Some(end_date) = filter.end_date.as_deref() {
        NaiveDate::parse_from_str(end_date, "%Y-%m-%d")?;
    }
    Ok(())
}

fn default_export_file_name() -> String {
    format!("kiroku_export_{}.csv", Local::now().format("%Y%m%d"))
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use rusqlite::Connection;

    use super::{export_to_csv, preview_export_count, ExportFilter};
    use crate::db::{initialize_db, insert_capture, mark_processed, update_description};
    use crate::models::CaptureRecord;

    fn test_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        env::temp_dir().join(format!(
            "kiroku-export-{test_name}-{}-{unique}",
            process::id()
        ))
    }

    fn seed_records(conn: &Connection) {
        let record_a = CaptureRecord {
            id: "capture-a".to_string(),
            timestamp: "2026-04-01T09:00:00+09:00".to_string(),
            app: "excel.exe".to_string(),
            window_title: "売上.xlsx".to_string(),
            image_path: Some("captures/a.png".to_string()),
            description: None,
            dhash: Some("0011".to_string()),
        };
        let record_b = CaptureRecord {
            id: "capture-b".to_string(),
            timestamp: "2026-04-02T10:00:00+09:00".to_string(),
            app: "outlook.exe".to_string(),
            window_title: "受信トレイ".to_string(),
            image_path: Some("captures/b.png".to_string()),
            description: None,
            dhash: Some("0022".to_string()),
        };

        insert_capture(conn, &record_a).expect("record A should insert");
        insert_capture(conn, &record_b).expect("record B should insert");
        update_description(conn, "capture-a", "Excel で売上表を確認している。")
            .expect("description should update");
        mark_processed(conn, "capture-a").expect("record A should mark processed");
    }

    #[test]
    fn preview_export_count_applies_filters() {
        let dir = test_dir("preview");
        fs::create_dir_all(&dir).expect("test directory should exist");
        let db_path = dir.join("kiroku.sqlite");
        let conn = initialize_db(&db_path).expect("database should initialize");
        seed_records(&conn);

        let preview = preview_export_count(
            &conn,
            &ExportFilter {
                start_date: Some("2026-04-01".to_string()),
                end_date: Some("2026-04-01".to_string()),
                apps: Some(vec!["excel.exe".to_string()]),
                only_processed: true,
            },
        )
        .expect("preview should succeed");

        assert_eq!(preview.count, 1);

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }

    #[test]
    fn export_to_csv_writes_bom_and_empty_description_as_blank() {
        let dir = test_dir("csv");
        fs::create_dir_all(&dir).expect("test directory should exist");
        let db_path = dir.join("kiroku.sqlite");
        let output_path = dir.join("export.csv");
        let conn = initialize_db(&db_path).expect("database should initialize");
        seed_records(&conn);

        let count = export_to_csv(
            &conn,
            &ExportFilter {
                start_date: None,
                end_date: None,
                apps: None,
                only_processed: false,
            },
            &output_path,
        )
        .expect("export should succeed");

        assert_eq!(count, 2);

        let bytes = fs::read(&output_path).expect("csv file should be readable");
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);

        let contents = String::from_utf8(bytes[3..].to_vec()).expect("csv should be utf-8");
        assert!(contents.contains("timestamp,app,window_title,description"));
        assert!(contents.contains("Excel で売上表を確認している。"));
        assert!(contents.contains("2026-04-02T10:00:00+09:00,outlook.exe,受信トレイ,"));

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }
}
