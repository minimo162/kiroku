use std::{
    fs,
    path::{Path, PathBuf},
};

use chrono::{DateTime, FixedOffset, Utc};
use image::{imageops::FilterType, DynamicImage, GenericImage, ImageBuffer, Rgba};
use uuid::Uuid;

use crate::{
    db::{
        assign_session_to_captures, get_captures_for_session_assembly, insert_session, DbError,
        SessionRecord,
    },
    models::{AppConfig, CaptureRecord},
};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("image processing error: {0}")]
    Image(#[from] image::ImageError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn process_pending_sessions(
    conn: &rusqlite::Connection,
    config: &AppConfig,
) -> Result<Vec<SessionRecord>, SessionError> {
    let captures = get_captures_for_session_assembly(conn)?;
    let sessions = assemble_sessions(captures, config);
    let data_dir = Path::new(&config.data_dir);
    let mut created_sessions = Vec::new();

    for session_captures in sessions {
        let key_frames = select_key_frames(&session_captures, config.max_frames_per_collage);
        if key_frames.is_empty() {
            continue;
        }

        let collage_path = build_collage(&key_frames, data_dir)?;
        let session_id = collage_path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_prefix("collage_"))
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let Some(first_capture) = session_captures.first() else {
            continue;
        };
        let Some(last_capture) = session_captures.last() else {
            continue;
        };

        let record = SessionRecord {
            id: session_id.clone(),
            start_time: first_capture.timestamp.clone(),
            end_time: last_capture.timestamp.clone(),
            collage_path: Some(collage_path.to_string_lossy().into_owned()),
            description: None,
            processed: false,
            capture_count: session_captures.len() as i64,
            frame_count: key_frames.len() as i64,
        };

        insert_session(conn, &record)?;
        let capture_ids = session_captures
            .iter()
            .map(|capture| capture.id.clone())
            .collect::<Vec<_>>();
        assign_session_to_captures(conn, &capture_ids, &session_id)?;
        created_sessions.push(record);
    }

    Ok(created_sessions)
}

fn assemble_sessions(captures: Vec<CaptureRecord>, config: &AppConfig) -> Vec<Vec<CaptureRecord>> {
    let mut sessions = Vec::new();
    let mut current_session = Vec::new();
    let mut current_start = None::<DateTime<FixedOffset>>;
    let mut previous_timestamp = None::<DateTime<FixedOffset>>;

    for capture in captures {
        let Ok(timestamp) = DateTime::parse_from_rfc3339(&capture.timestamp) else {
            continue;
        };

        if current_session.is_empty() {
            current_start = Some(timestamp);
            previous_timestamp = Some(timestamp);
            current_session.push(capture);
            continue;
        }

        let start = current_start.expect("current session start should exist");
        let previous = previous_timestamp.expect("previous timestamp should exist");
        let gap_secs = (timestamp - previous).num_seconds();
        let window_secs = (timestamp - start).num_seconds();

        if gap_secs > config.session_gap_secs as i64
            || window_secs > config.session_window_secs as i64
        {
            sessions.push(current_session);
            current_session = vec![capture];
            current_start = Some(timestamp);
            previous_timestamp = Some(timestamp);
            continue;
        }

        previous_timestamp = Some(timestamp);
        current_session.push(capture);
    }

    if current_session.is_empty() {
        return sessions;
    }

    let now = Utc::now().fixed_offset();
    if let Some(last_timestamp) = previous_timestamp {
        let age_secs = (now - last_timestamp).num_seconds();
        if age_secs > config.session_gap_secs as i64 {
            sessions.push(current_session);
        }
    }

    sessions
}

fn select_key_frames(captures: &[CaptureRecord], max_frames: u32) -> Vec<&CaptureRecord> {
    if captures.is_empty() {
        return Vec::new();
    }

    let mut deduped = vec![&captures[0]];
    for capture in captures.iter().skip(1) {
        let should_skip = deduped
            .last()
            .and_then(|previous| {
                compare_dhashes(previous.dhash.as_deref(), capture.dhash.as_deref())
            })
            .map(|distance| distance <= 5)
            .unwrap_or(false);

        if !should_skip {
            deduped.push(capture);
        }
    }

    let max_frames = max_frames.max(1) as usize;
    if deduped.len() <= max_frames {
        return deduped;
    }

    let total = deduped.len();
    (0..max_frames)
        .map(|index| deduped[index * total / max_frames])
        .collect()
}

fn build_collage(key_frames: &[&CaptureRecord], data_dir: &Path) -> Result<PathBuf, SessionError> {
    let (rows, cols) = match key_frames.len() {
        0 | 1 => (1, 1),
        2 => (1, 2),
        3 | 4 => (2, 2),
        _ => (2, 3),
    };

    let width = 1280u32;
    let height = 960u32;
    let cell_width = width / cols;
    let cell_height = height / rows;
    let mut canvas = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));

    for (index, capture) in key_frames.iter().enumerate() {
        let row = (index as u32) / cols;
        let col = (index as u32) % cols;
        let origin_x = col * cell_width;
        let origin_y = row * cell_height;

        let Some(image_path) = capture.image_path.as_deref() else {
            continue;
        };
        let Ok(image) = image::open(image_path) else {
            continue;
        };

        let resized = image
            .resize(cell_width, cell_height, FilterType::Lanczos3)
            .to_rgba8();
        let offset_x = origin_x + (cell_width.saturating_sub(resized.width())) / 2;
        let offset_y = origin_y + (cell_height.saturating_sub(resized.height())) / 2;
        canvas.copy_from(&resized, offset_x, offset_y)?;
    }

    let collage_dir = data_dir.join("sessions");
    fs::create_dir_all(&collage_dir)?;
    let session_id = Uuid::new_v4().to_string();
    let output_path = collage_dir.join(format!("collage_{session_id}.png"));
    DynamicImage::ImageRgba8(canvas).save(&output_path)?;

    Ok(output_path)
}

fn compare_dhashes(left: Option<&str>, right: Option<&str>) -> Option<u32> {
    let left = left?;
    let right = right?;
    let left = u64::from_str_radix(left, 16).ok()?;
    let right = u64::from_str_radix(right, 16).ok()?;
    Some((left ^ right).count_ones())
}

#[cfg(test)]
mod tests {
    use super::{assemble_sessions, select_key_frames};
    use crate::models::{AppConfig, CaptureRecord};

    fn capture(id: &str, timestamp: &str, dhash: Option<&str>) -> CaptureRecord {
        CaptureRecord {
            id: id.to_string(),
            timestamp: timestamp.to_string(),
            app: "excel.exe".to_string(),
            window_title: "sheet".to_string(),
            image_path: Some(format!("captures/{id}.png")),
            description: None,
            dhash: dhash.map(ToOwned::to_owned),
        }
    }

    #[test]
    fn assemble_sessions_splits_by_gap_and_window() {
        let config = AppConfig {
            session_gap_secs: 600,
            session_window_secs: 300,
            ..AppConfig::default()
        };
        let captures = vec![
            capture("a", "2024-01-01T10:00:00+09:00", None),
            capture("b", "2024-01-01T10:04:00+09:00", None),
            capture("c", "2024-01-01T10:12:00+09:00", None),
            capture("d", "2024-01-01T10:30:00+09:00", None),
        ];

        let sessions = assemble_sessions(captures, &config);

        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].len(), 2);
        assert_eq!(sessions[1].len(), 1);
        assert_eq!(sessions[2].len(), 1);
    }

    #[test]
    fn select_key_frames_deduplicates_near_identical_hashes() {
        let captures = vec![
            capture("a", "2024-01-01T10:00:00+09:00", Some("0000000000000000")),
            capture("b", "2024-01-01T10:01:00+09:00", Some("0000000000000001")),
            capture("c", "2024-01-01T10:02:00+09:00", Some("ffffffffffffffff")),
        ];

        let frames = select_key_frames(&captures, 6);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].id, "a");
        assert_eq!(frames[1].id, "c");
    }
}
