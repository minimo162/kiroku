use std::{env, fs, io, path::Path, time::Instant};

use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_LENGTH, ETAG};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use thiserror::Error;

use crate::{
    config::save_config, models::AppConfig, state::AppState, vlm::server::resolve_model_paths,
};

const MODEL_REPO_BASE: &str =
    "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main";
const DEFAULT_MODEL_FILE_NAME: &str = "Qwen3.5-0.8B-UD-Q6_K_XL.gguf";
const DEFAULT_MMPROJ_FILE_NAME: &str = "mmproj-BF16.gguf";
const MODEL_PROGRESS_EVENT: &str = "model-download-progress";
const ENV_MODEL_URL: &str = "KIROKU_MODEL_URL";
const ENV_MMPROJ_URL: &str = "KIROKU_MMPROJ_URL";

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatus {
    pub setup_complete: bool,
    pub model_ready: bool,
    pub llama_server_available: bool,
    pub models_dir: String,
    pub model_path: Option<String>,
    pub mmproj_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadProgress {
    pub step: String,
    pub file_name: String,
    pub percent: f64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed: String,
    pub remaining: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadResult {
    pub model_path: String,
    pub mmproj_path: String,
}

#[derive(Debug, Error)]
pub enum ModelManagerError {
    #[error("failed to create models directory")]
    Io(#[from] io::Error),
    #[error("failed to download model file")]
    Http(#[from] reqwest::Error),
    #[error("checksum verification failed for {file_name}")]
    ChecksumMismatch { file_name: String },
    #[error("setup cannot complete until both model files are ready")]
    SetupIncomplete,
    #[error("failed to persist config")]
    Config(#[from] crate::config::ConfigError),
}

#[derive(Debug, Clone)]
struct ArtifactSpec {
    file_name: &'static str,
    env_key: &'static str,
}

const MODEL_ARTIFACTS: [ArtifactSpec; 2] = [
    ArtifactSpec {
        file_name: DEFAULT_MODEL_FILE_NAME,
        env_key: ENV_MODEL_URL,
    },
    ArtifactSpec {
        file_name: DEFAULT_MMPROJ_FILE_NAME,
        env_key: ENV_MMPROJ_URL,
    },
];

#[tauri::command]
pub async fn get_setup_status(state: State<'_, AppState>) -> Result<SetupStatus, String> {
    Ok(build_setup_status(state.inner()).await)
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ModelDownloadResult, String> {
    let models_dir = state.app_paths.data_dir.join("models");
    fs::create_dir_all(&models_dir).map_err(|error| error.to_string())?;
    let client = build_download_client().map_err(|error| error.to_string())?;

    for artifact in MODEL_ARTIFACTS {
        let destination = models_dir.join(artifact.file_name);
        if destination.exists() {
            emit_progress(
                &app,
                ModelDownloadProgress {
                    step: "cached".to_string(),
                    file_name: artifact.file_name.to_string(),
                    percent: 100.0,
                    downloaded_bytes: 0,
                    total_bytes: None,
                    speed: "既存ファイルを使用".to_string(),
                    remaining: "0秒".to_string(),
                },
            );
            continue;
        }

        let url = model_url_for(&artifact);
        download_artifact(&client, &app, artifact.file_name, &url, &destination)
            .await
            .map_err(|error| error.to_string())?;
    }

    let (model_path, mmproj_path) = resolve_model_paths(&state.app_paths)
        .ok_or_else(|| "downloaded model files could not be discovered".to_string())?;

    Ok(ModelDownloadResult {
        model_path: model_path.to_string_lossy().into_owned(),
        mmproj_path: mmproj_path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn complete_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppConfig, String> {
    if resolve_model_paths(&state.app_paths).is_none() {
        return Err(ModelManagerError::SetupIncomplete.to_string());
    }

    let mut config = state.config.lock().await.clone();
    config.setup_complete = true;
    save_config(&state.app_paths.config_path, &config).map_err(|error| error.to_string())?;

    {
        let mut current = state.config.lock().await;
        *current = config.clone();
    }
    let _ = state.config_tx.send(config.clone());
    let _ = app.emit("config-updated", &config);

    Ok(config)
}

async fn build_setup_status(state: &AppState) -> SetupStatus {
    let config = state.config.lock().await.clone();
    let model_paths = resolve_model_paths(&state.app_paths);
    let llama_server_available = {
        let server = state.vlm_server.lock().await;
        server.binary_path().is_some()
    };

    SetupStatus {
        setup_complete: config.setup_complete,
        model_ready: model_paths.is_some(),
        llama_server_available,
        models_dir: state
            .app_paths
            .data_dir
            .join("models")
            .to_string_lossy()
            .into_owned(),
        model_path: model_paths
            .as_ref()
            .map(|value| value.0.to_string_lossy().into_owned()),
        mmproj_path: model_paths.map(|value| value.1.to_string_lossy().into_owned()),
    }
}

async fn download_artifact(
    client: &reqwest::Client,
    app: &AppHandle,
    file_name: &str,
    url: &str,
    destination: &Path,
) -> Result<(), ModelManagerError> {
    let response = client.get(url).send().await?.error_for_status()?;
    let total_bytes = content_length(response.headers());
    let expected_hash = expected_hash(response.headers());
    let mut stream = response.bytes_stream();
    let started_at = Instant::now();
    let tmp_path = destination.with_extension("download");
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let mut downloaded_bytes = 0_u64;
    let mut hasher = Sha256::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        hasher.update(&chunk);
        downloaded_bytes += chunk.len() as u64;

        emit_progress(
            app,
            ModelDownloadProgress {
                step: "downloading".to_string(),
                file_name: file_name.to_string(),
                percent: percentage(downloaded_bytes, total_bytes),
                downloaded_bytes,
                total_bytes,
                speed: format_speed(downloaded_bytes, started_at.elapsed().as_secs_f64()),
                remaining: format_remaining(
                    downloaded_bytes,
                    total_bytes,
                    started_at.elapsed().as_secs_f64(),
                ),
            },
        );
    }

    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    let actual_hash = format!("{:x}", hasher.finalize());
    if let Some(expected_hash) = expected_hash {
        if actual_hash != expected_hash {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(ModelManagerError::ChecksumMismatch {
                file_name: file_name.to_string(),
            });
        }
    }

    tokio::fs::rename(&tmp_path, destination).await?;
    emit_progress(
        app,
        ModelDownloadProgress {
            step: "complete".to_string(),
            file_name: file_name.to_string(),
            percent: 100.0,
            downloaded_bytes,
            total_bytes,
            speed: format_speed(downloaded_bytes, started_at.elapsed().as_secs_f64()),
            remaining: "0秒".to_string(),
        },
    );

    Ok(())
}

fn model_url_for(spec: &ArtifactSpec) -> String {
    env::var(spec.env_key).unwrap_or_else(|_| format!("{MODEL_REPO_BASE}/{}", spec.file_name))
}

fn build_download_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder().use_native_tls().build()
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

fn expected_hash(headers: &HeaderMap) -> Option<String> {
    extract_hash(headers.get("x-linked-etag")).or_else(|| extract_hash(headers.get(ETAG)))
}

fn extract_hash(value: Option<&HeaderValue>) -> Option<String> {
    let value = value?.to_str().ok()?.trim_matches('"').to_ascii_lowercase();
    if value.len() == 64 && value.chars().all(|char| char.is_ascii_hexdigit()) {
        Some(value)
    } else {
        None
    }
}

fn percentage(downloaded_bytes: u64, total_bytes: Option<u64>) -> f64 {
    match total_bytes {
        Some(total_bytes) if total_bytes > 0 => {
            (downloaded_bytes as f64 / total_bytes as f64) * 100.0
        }
        _ => 0.0,
    }
}

fn format_speed(downloaded_bytes: u64, elapsed_secs: f64) -> String {
    if elapsed_secs <= 0.0 {
        return "-".to_string();
    }

    format!(
        "{}/秒",
        format_bytes((downloaded_bytes as f64 / elapsed_secs) as u64)
    )
}

fn format_remaining(downloaded_bytes: u64, total_bytes: Option<u64>, elapsed_secs: f64) -> String {
    let Some(total_bytes) = total_bytes else {
        return "計算中".to_string();
    };
    if downloaded_bytes == 0 || elapsed_secs <= 0.0 {
        return "計算中".to_string();
    }

    let bytes_per_sec = downloaded_bytes as f64 / elapsed_secs;
    let remaining_secs = ((total_bytes.saturating_sub(downloaded_bytes)) as f64 / bytes_per_sec)
        .max(0.0)
        .round() as u64;

    if remaining_secs < 60 {
        format!("{remaining_secs}秒")
    } else {
        format!("{}分", remaining_secs / 60)
    }
}

fn format_bytes(value: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let value = value as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.1} KB", value / KB)
    } else {
        format!("{} B", value as u64)
    }
}

fn emit_progress(app: &AppHandle, progress: ModelDownloadProgress) {
    let _ = app.emit(MODEL_PROGRESS_EVENT, &progress);
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;

    use super::{
        build_download_client, extract_hash, format_remaining, percentage,
        DEFAULT_MMPROJ_FILE_NAME, DEFAULT_MODEL_FILE_NAME, MODEL_REPO_BASE,
    };

    #[test]
    fn extract_hash_accepts_sha256_etag() {
        let hash = extract_hash(Some(&HeaderValue::from_static(
            "\"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"",
        )))
        .expect("hash should parse");

        assert_eq!(
            hash,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn percentage_returns_zero_without_total() {
        assert_eq!(percentage(10, None), 0.0);
    }

    #[test]
    fn format_remaining_uses_seconds_or_minutes() {
        assert_eq!(format_remaining(50, Some(100), 10.0), "10秒");
        assert_eq!(format_remaining(100, Some(1000), 10.0), "1分");
    }

    #[test]
    fn default_model_artifacts_point_to_qwen35_unsloth_repo() {
        assert_eq!(
            MODEL_REPO_BASE,
            "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main"
        );
        assert_eq!(DEFAULT_MODEL_FILE_NAME, "Qwen3.5-0.8B-UD-Q6_K_XL.gguf");
        assert_eq!(DEFAULT_MMPROJ_FILE_NAME, "mmproj-BF16.gguf");
    }

    #[test]
    fn download_client_builds_with_native_tls_enabled() {
        build_download_client().expect("download client should build");
    }
}
