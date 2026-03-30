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
pub struct MaskRule {
    pub pattern: String,
    pub replacement: String,
    pub is_regex: bool,
}

impl Default for MaskRule {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            replacement: "[MASKED]".to_string(),
            is_regex: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub capture_interval_secs: u64,
    pub dhash_threshold: u32,
    pub auto_delete_images: bool,
    pub scheduler_enabled: bool,
    pub setup_complete: bool,
    pub batch_time: String,
    pub vlm_host: String,
    pub vlm_max_tokens: u32,
    pub data_dir: String,
    pub system_prompt: String,
    pub user_prompt: String,
    pub mask_rules: Vec<MaskRule>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            capture_interval_secs: 30,
            dhash_threshold: 10,
            auto_delete_images: true,
            scheduler_enabled: true,
            setup_complete: false,
            batch_time: "22:00".to_string(),
            vlm_host: "127.0.0.1:8080".to_string(),
            vlm_max_tokens: 256,
            data_dir: String::new(),
            system_prompt: default_system_prompt(),
            user_prompt: default_user_prompt(),
            mask_rules: Vec::new(),
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

pub fn default_system_prompt() -> String {
    concat!(
        "あなたは経理部門向けの業務記録アシスタントです。画面上で確認できる事実を優先し、",
        "日本語で簡潔に記述してください。SAP GUI、Excel、Outlook、Teams などの画面を対象とし、",
        "連結PKG、内部取引消去、UPI、月次決算、メール確認、会議参加などの業務文脈が明確な場合のみ用語を使ってください。",
        "推測は控えめにし、不確実な場合は一般的な表現に留めてください。"
    )
    .to_string()
}

pub fn default_user_prompt() -> String {
    concat!(
        "このスクリーンショットに写っている業務操作を1から3文で説明してください。",
        "必ず次の観点を含めてください: 使用中のアプリケーション、実行している操作、表示されているデータや対象。",
        "出力は自然な日本語の文章のみとし、箇条書きやJSONは使わないでください。"
    )
    .to_string()
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VlmBatchProgress {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub current_id: Option<String>,
    pub estimated_remaining_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::{default_system_prompt, default_user_prompt, AppConfig, CaptureRecord, MaskRule};

    #[test]
    fn app_config_roundtrip_json() {
        let config = AppConfig {
            capture_interval_secs: 45,
            dhash_threshold: 12,
            auto_delete_images: false,
            scheduler_enabled: true,
            setup_complete: true,
            batch_time: "23:15".to_string(),
            vlm_host: "127.0.0.1:8181".to_string(),
            vlm_max_tokens: 384,
            data_dir: "C:\\Users\\tester\\AppData\\Local\\Kiroku".to_string(),
            system_prompt: default_system_prompt(),
            user_prompt: default_user_prompt(),
            mask_rules: vec![MaskRule {
                pattern: "株式会社A".to_string(),
                replacement: "[取引先]".to_string(),
                is_regex: false,
            }],
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
