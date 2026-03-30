use std::{
    fs, io,
    path::{Path, PathBuf},
};

use tauri::{Emitter, Manager, Runtime};
use tauri_plugin_dialog::DialogExt;
use thiserror::Error;

use crate::{
    models::AppConfig,
    recorder::{start_recording_inner, stop_recording_inner},
    state::AppState,
    vlm::server::{parse_host_and_port, LlamaServer, VlmError},
};

pub const CONFIG_FILE_NAME: &str = "config.json";
pub const DB_FILE_NAME: &str = "kiroku.db";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub resource_dir: Option<PathBuf>,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_path = data_dir.join(CONFIG_FILE_NAME);
        let db_path = data_dir.join(DB_FILE_NAME);
        Self {
            data_dir,
            config_path,
            db_path,
            resource_dir: None,
        }
    }

    pub fn with_resource_dir(mut self, resource_dir: Option<PathBuf>) -> Self {
        self.resource_dir = resource_dir;
        self
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to resolve app local data directory")]
    PathResolution(#[source] tauri::Error),
    #[error("failed to read or write config file")]
    Io(#[from] io::Error),
    #[error("failed to parse config file")]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    Vlm(#[from] VlmError),
}

pub fn resolve_app_paths<R: Runtime, M: Manager<R>>(manager: &M) -> Result<AppPaths, ConfigError> {
    let data_dir = manager
        .path()
        .app_local_data_dir()
        .map_err(ConfigError::PathResolution)?;
    let resource_dir = manager.path().resource_dir().ok();
    Ok(AppPaths::new(data_dir).with_resource_dir(resource_dir))
}

pub fn load_config(path: &Path, default_data_dir: &Path) -> Result<AppConfig, ConfigError> {
    let contents = fs::read_to_string(path)?;
    let mut config: AppConfig = serde_json::from_str(&contents)?;

    config.ensure_data_dir(default_data_dir);

    Ok(config)
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(config)?;
    fs::write(path, contents)?;

    Ok(())
}

pub fn load_or_create_config(
    config_path: &Path,
    default_data_dir: &Path,
) -> Result<AppConfig, ConfigError> {
    let config = if config_path.exists() {
        load_config(config_path, default_data_dir)?
    } else {
        AppConfig::with_data_dir(default_data_dir.to_string_lossy().into_owned())
    };

    save_config(config_path, &config)?;

    Ok(config)
}

pub fn load_or_create_config_for_manager<R: Runtime, M: Manager<R>>(
    manager: &M,
) -> Result<(AppConfig, AppPaths), ConfigError> {
    let paths = resolve_app_paths(manager)?;
    let config = load_or_create_config(&paths.config_path, &paths.data_dir)?;
    Ok((config, paths))
}

#[tauri::command]
pub async fn get_config(state: tauri::State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().await.clone())
}

#[tauri::command]
pub async fn save_config_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    let mut next_config = config;
    if next_config.data_dir.trim().is_empty() {
        next_config.ensure_data_dir(&state.app_paths.data_dir);
    }

    let next_server = LlamaServer::from_config(&next_config, &state.app_paths)
        .map_err(|error| error.to_string())?;

    save_config(&state.app_paths.config_path, &next_config).map_err(|error| error.to_string())?;

    let was_recording = *state.is_recording.lock().await;
    if was_recording {
        stop_recording_inner(app.clone(), state.inner().clone()).await?;
    }

    {
        let mut current = state.config.lock().await;
        *current = next_config.clone();
    }
    let _ = state.config_tx.send(next_config.clone());

    {
        let mut server = state.vlm_server.lock().await;
        let _ = server.stop();
        *server = next_server;
    }
    {
        let mut vlm_state = state.vlm_state.lock().await;
        vlm_state.server_running = false;
        vlm_state.last_error = None;
    }

    if was_recording {
        start_recording_inner(app.clone(), state.inner().clone()).await?;
    }

    let _ = app.emit("config-updated", &next_config);
    Ok(next_config)
}

#[tauri::command]
pub async fn select_data_dir(app: tauri::AppHandle) -> Result<Option<String>, String> {
    match app
        .dialog()
        .file()
        .set_title("データ保存フォルダを選択")
        .blocking_pick_folder()
    {
        Some(path) => Ok(Some(
            path.into_path()
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .into_owned(),
        )),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn test_vlm_connection(vlm_host: String) -> Result<bool, String> {
    let (host, port) = parse_host_and_port(&vlm_host).map_err(|error| error.to_string())?;
    let health_url = format!("http://{host}:{port}/health");

    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())?
        .get(health_url)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    Ok(response.status().is_success())
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{load_config, load_or_create_config, AppConfig, CONFIG_FILE_NAME};
    use crate::{config::AppPaths, vlm::server::LlamaServer};

    fn test_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        env::temp_dir().join(format!("kiroku-{test_name}-{}-{unique}", process::id()))
    }

    #[test]
    fn load_or_create_config_writes_default_file() {
        let data_dir = test_dir("default-config");
        let config_path = data_dir.join(CONFIG_FILE_NAME);

        let config = load_or_create_config(&config_path, &data_dir).expect("config should load");

        assert_eq!(
            config,
            AppConfig::with_data_dir(data_dir.to_string_lossy().into_owned())
        );
        assert!(config_path.exists(), "config file should be created");

        fs::remove_dir_all(&data_dir).expect("temporary config directory should be removed");
    }

    #[test]
    fn load_config_backfills_empty_data_dir() {
        let data_dir = test_dir("backfill-data-dir");
        let config_path = data_dir.join(CONFIG_FILE_NAME);

        fs::create_dir_all(&data_dir).expect("temporary config directory should be created");
        fs::write(
            &config_path,
            r#"{
  "capture_interval_secs": 30,
  "dhash_threshold": 10,
  "auto_delete_images": true,
  "scheduler_enabled": true,
  "setup_complete": false,
  "batch_time": "22:00",
  "vlm_host": "127.0.0.1:8080",
  "vlm_max_tokens": 256,
  "data_dir": ""
}"#,
        )
        .expect("config fixture should be written");

        let config = load_config(&config_path, &data_dir).expect("config should load");

        assert_eq!(config.data_dir, data_dir.to_string_lossy());

        fs::remove_dir_all(&data_dir).expect("temporary config directory should be removed");
    }

    #[test]
    fn remote_vlm_host_is_rejected_before_persisting() {
        let data_dir = test_dir("reject-remote-vlm");
        fs::create_dir_all(&data_dir).expect("temporary config directory should be created");

        let config = AppConfig {
            vlm_host: "api.openai.com:443".to_string(),
            data_dir: data_dir.to_string_lossy().into_owned(),
            ..AppConfig::default()
        };

        let error = LlamaServer::from_config(&config, &AppPaths::new(data_dir.clone()))
            .expect_err("remote VLM host should fail");
        assert!(
            error.to_string().contains("localhost"),
            "error should explain the localhost-only restriction"
        );

        fs::remove_dir_all(&data_dir).expect("temporary config directory should be removed");
    }
}
