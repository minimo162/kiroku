use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(target_os = "windows")]
use chrono::{Local, SecondsFormat};
#[cfg(target_os = "windows")]
use image::ImageFormat;
#[cfg(target_os = "windows")]
use std::time::Instant;
use tauri::State;
use thiserror::Error;
#[cfg(target_os = "windows")]
use uuid::Uuid;
#[cfg(target_os = "windows")]
use xcap::Monitor;

use crate::{
    db::{
        clear_image_path, insert_capture, list_capture_image_paths, list_processed_capture_images,
        DbError,
    },
    models::CaptureRecord,
    state::AppState,
};

const CAPTURE_DIR_NAME: &str = "captures";

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub record: CaptureRecord,
    pub width: u32,
    pub height: u32,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub orphaned_images_deleted: usize,
    pub processed_images_deleted: usize,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("screen capture is unsupported on this platform")]
    UnsupportedPlatform,
    #[error("no primary monitor is available")]
    NoPrimaryMonitor,
    #[cfg(target_os = "windows")]
    #[error("failed to enumerate or capture monitor")]
    XCap(#[from] xcap::XCapError),
    #[error("failed to read or write capture files")]
    Io(#[from] io::Error),
    #[error("failed to query capture records")]
    Db(#[from] DbError),
    #[error("failed to encode screenshot")]
    Image(#[from] image::ImageError),
    #[error("failed to serialize capture metadata")]
    Serde(#[from] serde_json::Error),
    #[error("capture task failed to join")]
    Join(#[from] tokio::task::JoinError),
}

pub fn capture_output_dir(base_dir: &Path) -> Result<PathBuf, CaptureError> {
    let output_dir = base_dir.join(CAPTURE_DIR_NAME);
    fs::create_dir_all(&output_dir)?;
    Ok(output_dir)
}

pub async fn capture_primary_monitor(output_dir: &Path) -> Result<CapturedFrame, CaptureError> {
    let output_dir = output_dir.to_path_buf();
    tokio::task::spawn_blocking(move || capture_primary_monitor_blocking(&output_dir)).await?
}

fn capture_primary_monitor_blocking(output_dir: &Path) -> Result<CapturedFrame, CaptureError> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = output_dir;
        Err(CaptureError::UnsupportedPlatform)
    }

    #[cfg(target_os = "windows")]
    {
        fs::create_dir_all(output_dir)?;

        let start = Instant::now();
        let monitors = Monitor::all()?;
        let primary = monitors
            .into_iter()
            .find(|monitor| monitor.is_primary().unwrap_or(false))
            .ok_or(CaptureError::NoPrimaryMonitor)?;

        let image = primary.capture_image()?;

        let capture_id = Uuid::new_v4();
        let captured_at = Local::now();
        let filename = format!(
            "screenshot_{}_{}.png",
            captured_at.format("%Y%m%d_%H%M%S"),
            capture_id.simple()
        );
        let image_path = output_dir.join(filename);

        image.save_with_format(&image_path, ImageFormat::Png)?;

        let record = CaptureRecord {
            id: capture_id.to_string(),
            timestamp: captured_at.to_rfc3339_opts(SecondsFormat::Secs, false),
            app: "unknown".to_string(),
            window_title: "unknown".to_string(),
            image_path: Some(image_path.to_string_lossy().into_owned()),
            description: None,
            dhash: None,
        };

        persist_capture_metadata(&record, &image_path)?;

        let elapsed = start.elapsed();
        if elapsed > Duration::from_millis(500) {
            eprintln!("capture exceeded target duration: {:?}", elapsed);
        }

        Ok(CapturedFrame {
            record,
            width: primary.width().unwrap_or(0),
            height: primary.height().unwrap_or(0),
            elapsed,
        })
    }
}

pub fn persist_capture_metadata(
    record: &CaptureRecord,
    image_path: &Path,
) -> Result<(), CaptureError> {
    let metadata_path = metadata_path_for_image(image_path);
    let contents = serde_json::to_string_pretty(record)?;
    fs::write(metadata_path, contents)?;
    Ok(())
}

pub fn metadata_path_for_image(image_path: &Path) -> PathBuf {
    image_path.with_extension("json")
}

pub fn remove_capture_artifacts(image_path: &Path) -> Result<(), CaptureError> {
    let metadata_path = metadata_path_for_image(image_path);
    remove_file_if_exists(&metadata_path)?;
    remove_file_if_exists(image_path)?;
    Ok(())
}

pub fn cleanup_orphaned_images(
    data_dir: &Path,
    conn: &rusqlite::Connection,
) -> Result<CleanupReport, CaptureError> {
    let capture_dir = capture_output_dir(data_dir)?;
    let referenced_paths = list_capture_image_paths(conn)?
        .into_iter()
        .map(PathBuf::from)
        .collect::<HashSet<_>>();
    let mut report = CleanupReport::default();

    for entry in fs::read_dir(&capture_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|value| value.to_str()) != Some("png") {
            continue;
        }

        if !referenced_paths.contains(&path) {
            remove_capture_artifacts(&path)?;
            report.orphaned_images_deleted += 1;
        }
    }

    for (capture_id, image_path) in list_processed_capture_images(conn)? {
        remove_capture_artifacts(Path::new(&image_path))?;
        clear_image_path(conn, &capture_id)?;
        report.processed_images_deleted += 1;
    }

    Ok(report)
}

pub async fn cleanup_capture_storage(state: &AppState) -> Result<CleanupReport, CaptureError> {
    let base_dir = state.capture_base_dir().await;
    let db = state.db.lock().await;
    cleanup_orphaned_images(&base_dir, &db)
}

fn remove_file_if_exists(path: &Path) -> Result<(), io::Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[tauri::command]
pub async fn capture_now(state: State<'_, AppState>) -> Result<CaptureRecord, String> {
    let base_dir = state.capture_base_dir().await;
    let output_dir = capture_output_dir(&base_dir).map_err(|err| err.to_string())?;
    let frame = capture_primary_monitor(&output_dir)
        .await
        .map_err(|err| err.to_string())?;

    {
        let db = state.db.lock().await;
        insert_capture(&db, &frame.record).map_err(|err| err.to_string())?;
    }

    {
        let mut stats = state.capture_stats.lock().await;
        stats.total_captures += 1;
        stats.effective_captures += 1;
        stats.last_capture_at = Some(frame.record.timestamp.clone());
    }

    Ok(frame.record)
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        db::{get_capture_by_id, initialize_db, insert_capture, mark_processed},
        models::CaptureRecord,
    };

    use super::{
        capture_output_dir, cleanup_orphaned_images, metadata_path_for_image,
        persist_capture_metadata, remove_capture_artifacts,
    };

    fn test_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        env::temp_dir().join(format!(
            "kiroku-capture-{test_name}-{}-{unique}",
            process::id()
        ))
    }

    #[test]
    fn capture_output_dir_is_created_under_base_dir() {
        let base_dir = test_dir("output-dir");
        let output_dir =
            capture_output_dir(&base_dir).expect("capture directory should be created");

        assert_eq!(output_dir, base_dir.join("captures"));
        assert!(output_dir.exists(), "capture directory should exist");

        fs::remove_dir_all(&base_dir).expect("temporary capture directory should be removed");
    }

    #[test]
    fn persist_capture_metadata_writes_sidecar_json() {
        let base_dir = test_dir("metadata");
        fs::create_dir_all(&base_dir).expect("temporary capture directory should be created");

        let image_path = base_dir.join("sample.png");
        fs::write(&image_path, b"png").expect("image placeholder should be created");

        let record = CaptureRecord {
            id: "capture-id".to_string(),
            timestamp: "2026-03-30T22:00:00+09:00".to_string(),
            app: "unknown".to_string(),
            window_title: "unknown".to_string(),
            image_path: Some(image_path.to_string_lossy().into_owned()),
            description: None,
            dhash: None,
        };

        persist_capture_metadata(&record, &image_path).expect("metadata should be persisted");

        let metadata_path = metadata_path_for_image(&image_path);
        let contents =
            fs::read_to_string(&metadata_path).expect("metadata sidecar should be readable");
        let restored: CaptureRecord =
            serde_json::from_str(&contents).expect("metadata sidecar should be valid JSON");

        assert_eq!(restored, record);

        fs::remove_dir_all(&base_dir).expect("temporary capture directory should be removed");
    }

    #[test]
    fn remove_capture_artifacts_deletes_image_and_metadata() {
        let base_dir = test_dir("cleanup");
        fs::create_dir_all(&base_dir).expect("temporary capture directory should be created");

        let image_path = base_dir.join("sample.png");
        let metadata_path = base_dir.join("sample.json");
        fs::write(&image_path, b"png").expect("image placeholder should be created");
        fs::write(&metadata_path, b"{}").expect("metadata placeholder should be created");

        remove_capture_artifacts(&image_path).expect("capture artifacts should be removed");

        assert!(!image_path.exists(), "image placeholder should be removed");
        assert!(
            !metadata_path.exists(),
            "metadata placeholder should be removed"
        );

        fs::remove_dir_all(&base_dir).expect("temporary capture directory should be removed");
    }

    #[test]
    fn remove_capture_artifacts_ignores_missing_files() {
        let base_dir = test_dir("missing-cleanup");
        fs::create_dir_all(&base_dir).expect("temporary capture directory should be created");

        let image_path = base_dir.join("missing.png");
        remove_capture_artifacts(&image_path).expect("missing artifacts should be ignored");

        fs::remove_dir_all(&base_dir).expect("temporary capture directory should be removed");
    }

    #[test]
    fn cleanup_orphaned_images_deletes_untracked_png_files() {
        let base_dir = test_dir("orphaned-images");
        fs::create_dir_all(&base_dir).expect("temporary capture directory should be created");
        let capture_dir = capture_output_dir(&base_dir).expect("capture directory should exist");
        let db_path = base_dir.join("kiroku.sqlite");
        let conn = initialize_db(&db_path).expect("database should initialize");

        let orphaned_image = capture_dir.join("orphaned.png");
        let orphaned_metadata = capture_dir.join("orphaned.json");
        fs::write(&orphaned_image, b"png").expect("orphaned image should be created");
        fs::write(&orphaned_metadata, b"{}").expect("orphaned metadata should be created");

        let report = cleanup_orphaned_images(&base_dir, &conn).expect("cleanup should succeed");
        assert_eq!(report.orphaned_images_deleted, 1);
        assert_eq!(report.processed_images_deleted, 0);
        assert!(!orphaned_image.exists(), "orphaned image should be removed");
        assert!(
            !orphaned_metadata.exists(),
            "orphaned metadata should be removed"
        );

        fs::remove_dir_all(&base_dir).expect("temporary capture directory should be removed");
    }

    #[test]
    fn cleanup_orphaned_images_removes_processed_capture_files_and_clears_path() {
        let base_dir = test_dir("processed-images");
        fs::create_dir_all(&base_dir).expect("temporary capture directory should be created");
        let capture_dir = capture_output_dir(&base_dir).expect("capture directory should exist");
        let db_path = base_dir.join("kiroku.sqlite");
        let conn = initialize_db(&db_path).expect("database should initialize");

        let image_path = capture_dir.join("processed.png");
        let metadata_path = capture_dir.join("processed.json");
        fs::write(&image_path, b"png").expect("processed image should be created");
        fs::write(&metadata_path, b"{}").expect("processed metadata should be created");

        let record = CaptureRecord {
            id: "processed-capture".to_string(),
            timestamp: "2026-03-30T22:00:00+09:00".to_string(),
            app: "excel.exe".to_string(),
            window_title: "月次決算.xlsx".to_string(),
            image_path: Some(image_path.to_string_lossy().into_owned()),
            description: Some("Excel で月次決算シートを確認している。".to_string()),
            dhash: Some("0011aa22bb33cc44".to_string()),
        };

        insert_capture(&conn, &record).expect("processed capture should insert");
        mark_processed(&conn, &record.id).expect("capture should mark processed");

        let report = cleanup_orphaned_images(&base_dir, &conn).expect("cleanup should succeed");
        assert_eq!(report.orphaned_images_deleted, 0);
        assert_eq!(report.processed_images_deleted, 1);
        assert!(!image_path.exists(), "processed image should be removed");
        assert!(
            !metadata_path.exists(),
            "processed metadata should be removed"
        );

        let stored = get_capture_by_id(&conn, &record.id)
            .expect("stored record should load")
            .expect("stored record should exist");
        assert_eq!(stored.image_path, None);

        fs::remove_dir_all(&base_dir).expect("temporary capture directory should be removed");
    }
}
