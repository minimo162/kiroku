use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRecord {
    pub id: String,
    pub timestamp: String,
    pub app: String,
    pub window_title: String,
    pub image_path: Option<String>,
    pub description: Option<String>,
    pub dhash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub capture_interval_secs: u64,
    pub dhash_threshold: u32,
    pub auto_delete_images: bool,
    pub batch_time: String,
    pub vlm_host: String,
    pub vlm_max_tokens: u32,
    pub data_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            capture_interval_secs: 30,
            dhash_threshold: 10,
            auto_delete_images: true,
            batch_time: "22:00".to_string(),
            vlm_host: "127.0.0.1:8080".to_string(),
            vlm_max_tokens: 256,
            data_dir: String::new(),
        }
    }
}

impl AppConfig {
    pub fn with_data_dir(data_dir: impl Into<String>) -> Self {
        Self {
            data_dir: data_dir.into(),
            ..Self::default()
        }
    }

    pub fn ensure_data_dir(&mut self, data_dir: &Path) -> bool {
        if self.data_dir.trim().is_empty() {
            self.data_dir = data_dir.to_string_lossy().into_owned();
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureStats {
    pub total_captures: u64,
    pub effective_captures: u64,
    pub skipped_captures: u64,
    pub last_capture_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VlmState {
    pub server_running: bool,
    pub batch_running: bool,
    pub last_error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, CaptureRecord};

    #[test]
    fn app_config_roundtrip_json() {
        let config = AppConfig {
            capture_interval_secs: 45,
            dhash_threshold: 12,
            auto_delete_images: false,
            batch_time: "23:15".to_string(),
            vlm_host: "127.0.0.1:8181".to_string(),
            vlm_max_tokens: 384,
            data_dir: "C:\\Users\\tester\\AppData\\Local\\Kiroku".to_string(),
        };

        let json = serde_json::to_string(&config).expect("config should serialize");
        let restored: AppConfig = serde_json::from_str(&json).expect("config should deserialize");

        assert_eq!(restored, config);
    }

    #[test]
    fn capture_record_roundtrip_json() {
        let record = CaptureRecord {
            id: "a4c6d52d-7c49-4773-93eb-bb7740c0f590".to_string(),
            timestamp: "2026-03-30T22:00:00+09:00".to_string(),
            app: "excel.exe".to_string(),
            window_title: "月次決算.xlsx".to_string(),
            image_path: Some("captures/20260330_220000.png".to_string()),
            description: Some("Excel で月次決算シートを確認している。".to_string()),
            dhash: Some("00ff12ab".to_string()),
        };

        let json = serde_json::to_string(&record).expect("record should serialize");
        let restored: CaptureRecord =
            serde_json::from_str(&json).expect("record should deserialize");

        assert_eq!(restored, record);
    }
}
