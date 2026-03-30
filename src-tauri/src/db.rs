use std::{fs, path::Path};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::CaptureRecord;

const DB_SCHEMA_VERSION: i32 = 2;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("failed to create database directory")]
    Io(#[from] std::io::Error),
    #[error("database operation failed")]
    Sql(#[from] rusqlite::Error),
    #[error("capture record was not found")]
    CaptureNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCaptureRecord {
    pub id: String,
    pub timestamp: String,
    pub app: String,
    pub window_title: String,
    pub image_path: Option<String>,
    pub description: Option<String>,
    pub dhash: Option<String>,
    pub vlm_processed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureDateGroup {
    pub date: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureAppGroup {
    pub app: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionHistoryRecord {
    pub capture_id: String,
    pub previous_description: Option<String>,
    pub new_description: Option<String>,
    pub edited_at: String,
}

pub fn initialize_db(db_path: &Path) -> Result<Connection, DbError> {
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(db_path)?;
    apply_migrations(&conn)?;
    Ok(conn)
}

pub fn insert_capture(conn: &Connection, record: &CaptureRecord) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO captures (id, timestamp, app, window_title, image_path, dhash, description, vlm_processed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0)",
        params![
            record.id,
            record.timestamp,
            record.app,
            record.window_title,
            record.image_path,
            record.dhash,
            record.description
        ],
    )?;
    Ok(())
}

pub fn update_description(conn: &Connection, id: &str, description: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE captures SET description = ?2 WHERE id = ?1",
        params![id, description],
    )?;
    Ok(())
}

pub fn update_description_with_history(
    conn: &Connection,
    id: &str,
    description: Option<&str>,
) -> Result<(), DbError> {
    let previous_description = conn
        .query_row(
            "SELECT description FROM captures WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .ok_or(DbError::CaptureNotFound)?;

    let next_description = description
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    if previous_description == next_description {
        return Ok(());
    }

    conn.execute(
        "INSERT INTO capture_description_history (capture_id, previous_description, new_description, edited_at)
         VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)",
        params![id, previous_description, next_description],
    )?;
    conn.execute(
        "UPDATE captures SET description = ?2 WHERE id = ?1",
        params![id, next_description],
    )?;

    Ok(())
}

pub fn mark_processed(conn: &Connection, id: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE captures SET vlm_processed = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn get_unprocessed(conn: &Connection) -> Result<Vec<CaptureRecord>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, app, window_title, image_path, description, dhash
         FROM captures
         WHERE vlm_processed = 0
         ORDER BY timestamp ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CaptureRecord {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            app: row.get(2)?,
            window_title: row.get(3)?,
            image_path: row.get(4)?,
            description: row.get(5)?,
            dhash: row.get(6)?,
        })
    })?;

    let mut captures = Vec::new();
    for row in rows {
        captures.push(row?);
    }

    Ok(captures)
}

pub fn query_captures_filtered(
    conn: &Connection,
    start_date: Option<&str>,
    end_date: Option<&str>,
    apps: Option<&[String]>,
    only_processed: bool,
) -> Result<Vec<StoredCaptureRecord>, DbError> {
    let mut sql = String::from(
        "SELECT id, timestamp, app, window_title, image_path, description, dhash, vlm_processed
         FROM captures
         WHERE 1 = 1",
    );
    let mut params = Vec::<String>::new();

    if only_processed {
        sql.push_str(" AND vlm_processed = 1");
    }
    if let Some(start_date) = start_date {
        sql.push_str(" AND substr(timestamp, 1, 10) >= ?");
        params.push(start_date.to_string());
    }
    if let Some(end_date) = end_date {
        sql.push_str(" AND substr(timestamp, 1, 10) <= ?");
        params.push(end_date.to_string());
    }
    if let Some(apps) = apps.filter(|apps| !apps.is_empty()) {
        sql.push_str(" AND app IN (");
        sql.push_str(&vec!["?"; apps.len()].join(","));
        sql.push(')');
        params.extend(apps.iter().cloned());
    }

    sql.push_str(" ORDER BY timestamp ASC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        params_from_iter(params.iter().map(|value| value as &dyn ToSql)),
        |row| {
            Ok(StoredCaptureRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                app: row.get(2)?,
                window_title: row.get(3)?,
                image_path: row.get(4)?,
                description: row.get(5)?,
                dhash: row.get(6)?,
                vlm_processed: row.get::<_, i64>(7)? != 0,
            })
        },
    )?;

    let mut captures = Vec::new();
    for row in rows {
        captures.push(row?);
    }

    Ok(captures)
}

pub fn get_recent_captures(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<StoredCaptureRecord>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, app, window_title, image_path, description, dhash, vlm_processed
         FROM captures
         ORDER BY timestamp DESC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(StoredCaptureRecord {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            app: row.get(2)?,
            window_title: row.get(3)?,
            image_path: row.get(4)?,
            description: row.get(5)?,
            dhash: row.get(6)?,
            vlm_processed: row.get::<_, i64>(7)? != 0,
        })
    })?;

    let mut captures = Vec::new();
    for row in rows {
        captures.push(row?);
    }

    Ok(captures)
}

pub fn get_capture_by_id(
    conn: &Connection,
    id: &str,
) -> Result<Option<StoredCaptureRecord>, DbError> {
    conn.query_row(
        "SELECT id, timestamp, app, window_title, image_path, description, dhash, vlm_processed
         FROM captures
         WHERE id = ?1",
        params![id],
        |row| {
            Ok(StoredCaptureRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                app: row.get(2)?,
                window_title: row.get(3)?,
                image_path: row.get(4)?,
                description: row.get(5)?,
                dhash: row.get(6)?,
                vlm_processed: row.get::<_, i64>(7)? != 0,
            })
        },
    )
    .optional()
    .map_err(DbError::from)
}

pub fn list_capture_dates(conn: &Connection) -> Result<Vec<CaptureDateGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT substr(timestamp, 1, 10) AS capture_date, COUNT(*)
         FROM captures
         GROUP BY capture_date
         ORDER BY capture_date DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CaptureDateGroup {
            date: row.get(0)?,
            count: row.get(1)?,
        })
    })?;

    let mut dates = Vec::new();
    for row in rows {
        dates.push(row?);
    }

    Ok(dates)
}

pub fn list_capture_apps(conn: &Connection) -> Result<Vec<CaptureAppGroup>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT app, COUNT(*)
         FROM captures
         GROUP BY app
         ORDER BY app ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CaptureAppGroup {
            app: row.get(0)?,
            count: row.get(1)?,
        })
    })?;

    let mut apps = Vec::new();
    for row in rows {
        apps.push(row?);
    }

    Ok(apps)
}

pub fn count_captures_for_date(conn: &Connection, date: &str) -> Result<u64, DbError> {
    let count = conn.query_row(
        "SELECT COUNT(*) FROM captures WHERE substr(timestamp, 1, 10) = ?1",
        params![date],
        |row| row.get::<_, u64>(0),
    )?;
    Ok(count)
}

pub fn get_captures_by_date(
    conn: &Connection,
    date: &str,
    page: u64,
    page_size: u64,
) -> Result<Vec<StoredCaptureRecord>, DbError> {
    let offset = page.saturating_sub(1) * page_size;
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, app, window_title, image_path, description, dhash, vlm_processed
         FROM captures
         WHERE substr(timestamp, 1, 10) = ?1
         ORDER BY timestamp DESC
         LIMIT ?2 OFFSET ?3",
    )?;

    let rows = stmt.query_map(params![date, page_size as i64, offset as i64], |row| {
        Ok(StoredCaptureRecord {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            app: row.get(2)?,
            window_title: row.get(3)?,
            image_path: row.get(4)?,
            description: row.get(5)?,
            dhash: row.get(6)?,
            vlm_processed: row.get::<_, i64>(7)? != 0,
        })
    })?;

    let mut captures = Vec::new();
    for row in rows {
        captures.push(row?);
    }

    Ok(captures)
}

pub fn get_description_history(
    conn: &Connection,
    capture_id: &str,
) -> Result<Vec<DescriptionHistoryRecord>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT capture_id, previous_description, new_description, edited_at
         FROM capture_description_history
         WHERE capture_id = ?1
         ORDER BY edited_at DESC, rowid DESC",
    )?;

    let rows = stmt.query_map(params![capture_id], |row| {
        Ok(DescriptionHistoryRecord {
            capture_id: row.get(0)?,
            previous_description: row.get(1)?,
            new_description: row.get(2)?,
            edited_at: row.get(3)?,
        })
    })?;

    let mut history = Vec::new();
    for row in rows {
        history.push(row?);
    }

    Ok(history)
}

pub fn count_processed_captures(conn: &Connection) -> Result<u64, DbError> {
    let count = conn.query_row(
        "SELECT COUNT(*) FROM captures WHERE vlm_processed = 1",
        [],
        |row| row.get::<_, u64>(0),
    )?;
    Ok(count)
}

pub fn count_unprocessed_captures(conn: &Connection) -> Result<u64, DbError> {
    let count = conn.query_row(
        "SELECT COUNT(*) FROM captures WHERE vlm_processed = 0",
        [],
        |row| row.get::<_, u64>(0),
    )?;
    Ok(count)
}

fn apply_migrations(conn: &Connection) -> Result<(), DbError> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

    if version < 1 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS captures (
                id TEXT PRIMARY KEY,
                timestamp TEXT NOT NULL,
                app TEXT NOT NULL,
                window_title TEXT NOT NULL,
                image_path TEXT,
                dhash TEXT,
                description TEXT,
                vlm_processed INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_captures_timestamp ON captures(timestamp);
            CREATE INDEX IF NOT EXISTS idx_captures_app ON captures(app);
            CREATE INDEX IF NOT EXISTS idx_captures_vlm_processed ON captures(vlm_processed);
            ",
        )?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    if version < 2 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS capture_description_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capture_id TEXT NOT NULL,
                previous_description TEXT,
                new_description TEXT,
                edited_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (capture_id) REFERENCES captures(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_capture_description_history_capture_id
                ON capture_description_history(capture_id, edited_at DESC);
            ",
        )?;
        conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        count_captures_for_date, get_captures_by_date, get_description_history, get_unprocessed,
        initialize_db, insert_capture, list_capture_apps, list_capture_dates, mark_processed,
        query_captures_filtered, update_description, update_description_with_history,
    };
    use crate::models::CaptureRecord;

    fn test_db_path(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        env::temp_dir().join(format!(
            "kiroku-db-{test_name}-{}-{unique}.sqlite",
            process::id()
        ))
    }

    fn sample_record() -> CaptureRecord {
        CaptureRecord {
            id: "capture-1".to_string(),
            timestamp: "2026-03-30T22:00:00+09:00".to_string(),
            app: "excel.exe".to_string(),
            window_title: "月次決算.xlsx".to_string(),
            image_path: Some("captures/sample.png".to_string()),
            description: None,
            dhash: Some("0011aa22bb33cc44".to_string()),
        }
    }

    #[test]
    fn insert_update_and_mark_processed_roundtrip() {
        let db_path = test_db_path("crud");
        let conn = initialize_db(&db_path).expect("database should initialize");
        let record = sample_record();

        insert_capture(&conn, &record).expect("record should insert");
        let unprocessed = get_unprocessed(&conn).expect("unprocessed captures should load");
        assert_eq!(unprocessed.len(), 1);
        assert_eq!(unprocessed[0], record);

        update_description(&conn, &record.id, "Excel で決算シートを更新")
            .expect("description should update");
        mark_processed(&conn, &record.id).expect("record should mark processed");

        let unprocessed = get_unprocessed(&conn).expect("unprocessed captures should reload");
        assert!(
            unprocessed.is_empty(),
            "processed record should be filtered out"
        );

        fs::remove_file(&db_path).expect("temporary database should be removed");
    }

    #[test]
    fn query_captures_filtered_applies_date_app_and_processed_filters() {
        let db_path = test_db_path("filter");
        let conn = initialize_db(&db_path).expect("database should initialize");

        let excel_record = sample_record();
        let outlook_record = CaptureRecord {
            id: "capture-2".to_string(),
            timestamp: "2026-03-31T09:00:00+09:00".to_string(),
            app: "outlook.exe".to_string(),
            window_title: "受信トレイ".to_string(),
            image_path: Some("captures/mail.png".to_string()),
            description: None,
            dhash: Some("1234".to_string()),
        };

        insert_capture(&conn, &excel_record).expect("excel record should insert");
        insert_capture(&conn, &outlook_record).expect("outlook record should insert");
        mark_processed(&conn, &excel_record.id).expect("excel record should mark processed");

        let filtered = query_captures_filtered(
            &conn,
            Some("2026-03-30"),
            Some("2026-03-30"),
            Some(&["excel.exe".to_string()]),
            true,
        )
        .expect("filtered captures should load");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, excel_record.id);

        fs::remove_file(&db_path).expect("temporary database should be removed");
    }

    #[test]
    fn update_description_with_history_tracks_before_and_after_values() {
        let db_path = test_db_path("history");
        let conn = initialize_db(&db_path).expect("database should initialize");
        let record = sample_record();

        insert_capture(&conn, &record).expect("record should insert");
        update_description_with_history(&conn, &record.id, Some("Excel で月次報告を修正"))
            .expect("manual update should succeed");
        update_description_with_history(&conn, &record.id, Some("Excel で月次報告を提出前確認"))
            .expect("second manual update should succeed");

        let history = get_description_history(&conn, &record.id).expect("history should load");

        assert_eq!(history.len(), 2);
        assert_eq!(
            history[0].previous_description.as_deref(),
            Some("Excel で月次報告を修正")
        );
        assert_eq!(
            history[0].new_description.as_deref(),
            Some("Excel で月次報告を提出前確認")
        );
        assert_eq!(history[1].previous_description, None);
        assert_eq!(
            history[1].new_description.as_deref(),
            Some("Excel で月次報告を修正")
        );

        fs::remove_file(&db_path).expect("temporary database should be removed");
    }

    #[test]
    fn get_captures_by_date_supports_pagination() {
        let db_path = test_db_path("preview-page");
        let conn = initialize_db(&db_path).expect("database should initialize");

        for index in 0..55 {
            let record = CaptureRecord {
                id: format!("capture-{index}"),
                timestamp: format!("2026-03-30T09:{:02}:00+09:00", index % 60),
                app: "excel.exe".to_string(),
                window_title: format!("sheet-{index}"),
                image_path: Some(format!("captures/{index}.png")),
                description: None,
                dhash: None,
            };
            insert_capture(&conn, &record).expect("record should insert");
        }

        let total = count_captures_for_date(&conn, "2026-03-30").expect("count should load");
        let first_page =
            get_captures_by_date(&conn, "2026-03-30", 1, 50).expect("page one should load");
        let second_page =
            get_captures_by_date(&conn, "2026-03-30", 2, 50).expect("page two should load");
        let groups = list_capture_dates(&conn).expect("capture dates should load");

        assert_eq!(total, 55);
        assert_eq!(first_page.len(), 50);
        assert_eq!(second_page.len(), 5);
        assert_eq!(groups[0].date, "2026-03-30");
        assert_eq!(groups[0].count, 55);

        fs::remove_file(&db_path).expect("temporary database should be removed");
    }

    #[test]
    fn list_capture_apps_returns_distinct_app_counts() {
        let db_path = test_db_path("app-groups");
        let conn = initialize_db(&db_path).expect("database should initialize");

        insert_capture(&conn, &sample_record()).expect("excel record should insert");
        insert_capture(
            &conn,
            &CaptureRecord {
                id: "capture-2".to_string(),
                timestamp: "2026-03-31T10:00:00+09:00".to_string(),
                app: "outlook.exe".to_string(),
                window_title: "受信トレイ".to_string(),
                image_path: None,
                description: None,
                dhash: None,
            },
        )
        .expect("outlook record should insert");

        let apps = list_capture_apps(&conn).expect("app groups should load");

        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].app, "excel.exe");
        assert_eq!(apps[0].count, 1);
        assert_eq!(apps[1].app, "outlook.exe");
        assert_eq!(apps[1].count, 1);

        fs::remove_file(&db_path).expect("temporary database should be removed");
    }
}
