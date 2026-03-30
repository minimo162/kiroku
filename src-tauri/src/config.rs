use std::{
    fs, io,
    path::{Path, PathBuf},
};

use tauri::{Manager, Runtime};
use thiserror::Error;

use crate::models::AppConfig;

pub const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_path = data_dir.join(CONFIG_FILE_NAME);
        Self {
            data_dir,
            config_path,
        }
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
}

pub fn resolve_app_paths<R: Runtime, M: Manager<R>>(manager: &M) -> Result<AppPaths, ConfigError> {
    let data_dir = manager
        .path()
        .app_local_data_dir()
        .map_err(ConfigError::PathResolution)?;
    Ok(AppPaths::new(data_dir))
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

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{load_config, load_or_create_config, AppConfig, CONFIG_FILE_NAME};

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
}
