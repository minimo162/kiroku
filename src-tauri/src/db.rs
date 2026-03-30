use std::{fs, path::Path};

use rusqlite::{params, params_from_iter, Connection, ToSql};
use thiserror::Error;

use crate::models::CaptureRecord;

const DB_SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("failed to create database directory")]
    Io(#[from] std::io::Error),
    #[error("database operation failed")]
    Sql(#[from] rusqlite::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        get_unprocessed, initialize_db, insert_capture, mark_processed, query_captures_filtered,
        update_description,
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
}
