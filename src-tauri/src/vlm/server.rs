use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::{Client, Url};
use tauri::{AppHandle, Emitter, State};
use thiserror::Error;
use tokio::time::sleep;

use crate::{
    config::AppPaths,
    models::{AppConfig, VlmState},
    state::AppState,
};

const DEFAULT_VLM_HOST: &str = "127.0.0.1:8080";
const DEFAULT_READY_TIMEOUT_SECS: u64 = 30;
const HEALTH_POLL_INTERVAL_MS: u64 = 500;
const ENV_LLAMA_SERVER_PATH: &str = "KIROKU_LLAMA_SERVER_PATH";

#[derive(Debug, Error)]
pub enum VlmError {
    #[error("invalid VLM host: {0}")]
    InvalidHost(String),
    #[error("llama-server binary could not be found")]
    BinaryNotFound,
    #[error("failed to start or stop llama-server")]
    Io(#[from] io::Error),
    #[error("VLM health check failed")]
    Http(#[from] reqwest::Error),
    #[error("VLM endpoint returned status {0}")]
    UnexpectedStatus(u16),
    #[error("llama-server startup timed out")]
    StartupTimeout,
    #[error("llama-server exited before becoming ready (code: {0:?})")]
    ProcessExited(Option<i32>),
    #[error("image processing failed")]
    Image(#[from] image::ImageError),
    #[error("failed to parse VLM response")]
    Serde(#[from] serde_json::Error),
    #[error("background task failed to join")]
    Join(#[from] tokio::task::JoinError),
    #[error("{0}")]
    InvalidResponse(String),
}

#[derive(Debug)]
pub struct LlamaServer {
    process: Option<Child>,
    host: String,
    port: u16,
    client: Client,
    binary_path: Option<PathBuf>,
}

impl LlamaServer {
    pub fn from_config(config: &AppConfig, app_paths: &AppPaths) -> Result<Self, VlmError> {
        let target = if config.vlm_host.trim().is_empty() {
            DEFAULT_VLM_HOST
        } else {
            config.vlm_host.as_str()
        };
        let (host, port) = parse_host_and_port(target)?;

        Ok(Self {
            process: None,
            host,
            port,
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(VlmError::Http)?,
            binary_path: resolve_binary_path(app_paths),
        })
    }

    pub async fn start(
        &mut self,
        model_path: &Path,
        mmproj_path: &Path,
        n_threads: usize,
    ) -> Result<(), VlmError> {
        if self.health_check().await.is_ok() {
            return Ok(());
        }

        if self.process_running()? {
            self.stop()?;
        }

        let binary_path = self.binary_path.clone().ok_or(VlmError::BinaryNotFound)?;

        let child = Command::new(binary_path)
            .args([
                "-m",
                &model_path.to_string_lossy(),
                "--mmproj",
                &mmproj_path.to_string_lossy(),
                "--host",
                &self.host,
                "--port",
                &self.port.to_string(),
                "-t",
                &n_threads.to_string(),
                "--ctx-size",
                "4096",
                "--log-disable",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        self.process = Some(child);
        if let Err(error) = self.wait_for_ready(DEFAULT_READY_TIMEOUT_SECS).await {
            let _ = self.stop();
            return Err(error);
        }

        Ok(())
    }

    pub async fn health_check(&self) -> Result<(), VlmError> {
        let response = self
            .client
            .get(format!("{}/health", self.server_url()))
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(VlmError::UnexpectedStatus(response.status().as_u16()))
        }
    }

    pub async fn wait_for_ready(&mut self, timeout_secs: u64) -> Result<(), VlmError> {
        let deadline = Instant::now() + Duration::from_secs(timeout_secs);

        loop {
            if Instant::now() > deadline {
                return Err(VlmError::StartupTimeout);
            }

            if let Some(child) = self.process.as_mut() {
                match child.try_wait()? {
                    Some(status) => {
                        self.process = None;
                        return Err(VlmError::ProcessExited(status.code()));
                    }
                    None => {}
                }
            }

            if self.health_check().await.is_ok() {
                return Ok(());
            }

            sleep(Duration::from_millis(HEALTH_POLL_INTERVAL_MS)).await;
        }
    }

    pub fn stop(&mut self) -> Result<(), VlmError> {
        if let Some(mut child) = self.process.take() {
            match child.try_wait()? {
                Some(_) => {}
                None => {
                    child.kill()?;
                    let _ = child.wait();
                }
            }
        }

        Ok(())
    }

    pub fn process_running(&mut self) -> Result<bool, VlmError> {
        if let Some(child) = self.process.as_mut() {
            match child.try_wait()? {
                Some(_) => {
                    self.process = None;
                    Ok(false)
                }
                None => Ok(true),
            }
        } else {
            Ok(false)
        }
    }

    pub fn server_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    pub fn binary_path(&self) -> Option<&Path> {
        self.binary_path.as_deref()
    }
}

#[tauri::command]
pub async fn check_vlm_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<VlmState, String> {
    let snapshot = refresh_vlm_state(state.inner()).await;
    let _ = app.emit("vlm-status", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn start_vlm_server(
    app: AppHandle,
    state: State<'_, AppState>,
    model_path: String,
    mmproj_path: String,
    n_threads: Option<usize>,
) -> Result<VlmState, String> {
    let n_threads = n_threads.unwrap_or_else(default_thread_count);

    let result = {
        let mut server = state.vlm_server.lock().await;
        server
            .start(Path::new(&model_path), Path::new(&mmproj_path), n_threads)
            .await
    };

    let snapshot = match result {
        Ok(()) => update_vlm_state(&state, Some(true), None, None).await,
        Err(error) => {
            let message = error.to_string();
            let snapshot = update_vlm_state(&state, Some(false), None, Some(message.clone())).await;
            let _ = app.emit("vlm-status", &snapshot);
            return Err(message);
        }
    };

    let _ = app.emit("vlm-status", &snapshot);
    Ok(snapshot)
}

#[tauri::command]
pub async fn stop_vlm_server(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<VlmState, String> {
    let result = {
        let mut server = state.vlm_server.lock().await;
        server.stop()
    };

    let snapshot = match result {
        Ok(()) => update_vlm_state(&state, Some(false), None, None).await,
        Err(error) => {
            let message = error.to_string();
            let snapshot = update_vlm_state(&state, Some(false), None, Some(message.clone())).await;
            let _ = app.emit("vlm-status", &snapshot);
            return Err(message);
        }
    };

    let _ = app.emit("vlm-status", &snapshot);
    Ok(snapshot)
}

pub async fn refresh_vlm_state(state: &AppState) -> VlmState {
    let status = {
        let mut server = state.vlm_server.lock().await;
        let process_running = server.process_running();
        let healthy = server.health_check().await;

        match (process_running, healthy) {
            (Ok(_), Ok(())) => (true, None),
            (Ok(true), Err(error)) => (true, Some(error.to_string())),
            (Ok(false), Err(error)) => (false, Some(error.to_string())),
            (Err(error), _) => (false, Some(error.to_string())),
        }
    };

    update_vlm_state(state, Some(status.0), None, status.1).await
}

pub fn default_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4)
}

pub async fn update_vlm_state(
    state: &AppState,
    server_running: Option<bool>,
    batch_running: Option<bool>,
    last_error: Option<String>,
) -> VlmState {
    let mut vlm_state = state.vlm_state.lock().await;
    if let Some(server_running) = server_running {
        vlm_state.server_running = server_running;
    }
    if let Some(batch_running) = batch_running {
        vlm_state.batch_running = batch_running;
    }
    vlm_state.last_error = last_error;
    vlm_state.clone()
}

fn parse_host_and_port(input: &str) -> Result<(String, u16), VlmError> {
    let normalized = if input.contains("://") {
        input.to_string()
    } else {
        format!("http://{input}")
    };

    let url = Url::parse(&normalized).map_err(|_| VlmError::InvalidHost(input.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| VlmError::InvalidHost(input.to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| VlmError::InvalidHost(input.to_string()))?;

    Ok((host.to_string(), port))
}

fn resolve_binary_path(app_paths: &AppPaths) -> Option<PathBuf> {
    if let Some(path) = env::var_os(ENV_LLAMA_SERVER_PATH).map(PathBuf::from) {
        if path.exists() {
            return Some(path);
        }
    }

    let mut search_roots = vec![
        app_paths.data_dir.join("binaries"),
        app_paths.data_dir.clone(),
    ];

    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            search_roots.push(parent.to_path_buf());
            search_roots.push(parent.join("binaries"));
        }
    }

    for root in search_roots {
        if let Some(path) = find_binary_in_dir(&root) {
            return Some(path);
        }
    }

    env::var_os("PATH").and_then(|value| {
        env::split_paths(&value).find_map(|dir| {
            let candidate = binary_names()
                .into_iter()
                .map(|name| dir.join(name))
                .find(|path| path.exists());

            candidate.or_else(|| find_binary_in_dir(&dir))
        })
    })
}

pub fn resolve_model_paths(app_paths: &AppPaths) -> Option<(PathBuf, PathBuf)> {
    let models_dir = app_paths.data_dir.join("models");
    let entries = fs::read_dir(models_dir).ok()?;

    let mut model_path = None;
    let mut mmproj_path = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().map(|value| value.to_string_lossy()) else {
            continue;
        };
        let file_name = file_name.to_ascii_lowercase();

        if !file_name.ends_with(".gguf") {
            continue;
        }

        if file_name.contains("mmproj") {
            if mmproj_path.is_none() {
                mmproj_path = Some(path);
            }
        } else if model_path.is_none() {
            model_path = Some(path);
        }
    }

    Some((model_path?, mmproj_path?))
}

fn binary_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["llama-server.exe", "llama-server"]
    } else {
        vec!["llama-server"]
    }
}

fn find_binary_in_dir(dir: &Path) -> Option<PathBuf> {
    for name in binary_names() {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if file_name.starts_with("llama-server") {
            return Some(path);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        net::SocketAddr,
        path::PathBuf,
        process,
        time::{SystemTime, UNIX_EPOCH},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::{
        find_binary_in_dir, parse_host_and_port, resolve_model_paths, LlamaServer, VlmError,
    };
    use crate::{config::AppPaths, models::AppConfig};

    fn test_dir(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos();
        env::temp_dir().join(format!("kiroku-vlm-{test_name}-{}-{unique}", process::id()))
    }

    async fn spawn_health_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("address should resolve");

        tokio::spawn(async move {
            loop {
                let accepted = listener.accept().await;
                let Ok((mut stream, _)) = accepted else {
                    break;
                };

                tokio::spawn(async move {
                    let mut buffer = [0_u8; 1024];
                    let _ = stream.read(&mut buffer).await;
                    let response =
                        b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";
                    let _ = stream.write_all(response).await;
                });
            }
        });

        addr
    }

    #[test]
    fn parse_host_and_port_accepts_plain_host() {
        let parsed = parse_host_and_port("127.0.0.1:8080").expect("host should parse");
        assert_eq!(parsed, ("127.0.0.1".to_string(), 8080));
    }

    #[test]
    fn find_binary_in_dir_matches_sidecar_prefix() {
        let dir = test_dir("binary-discovery");
        fs::create_dir_all(&dir).expect("test directory should be created");

        let binary_path = dir.join("llama-server-x86_64-pc-windows-msvc.exe");
        fs::write(&binary_path, b"binary").expect("binary fixture should be written");

        let discovered = find_binary_in_dir(&dir).expect("binary should be discovered");
        assert_eq!(discovered, binary_path);

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }

    #[test]
    fn resolve_model_paths_picks_model_and_mmproj() {
        let dir = test_dir("model-discovery");
        let app_paths = AppPaths::new(dir.clone());
        let models_dir = dir.join("models");
        fs::create_dir_all(&models_dir).expect("models directory should exist");

        let model_path = models_dir.join("qwen2.5-vl-0.5b-instruct.gguf");
        let mmproj_path = models_dir.join("qwen2.5-vl-mmproj-f16.gguf");
        fs::write(&model_path, b"model").expect("model fixture should be written");
        fs::write(&mmproj_path, b"mmproj").expect("mmproj fixture should be written");

        let discovered = resolve_model_paths(&app_paths).expect("model paths should resolve");
        assert_eq!(discovered.0, model_path);
        assert_eq!(discovered.1, mmproj_path);

        fs::remove_dir_all(&dir).expect("test directory should be removed");
    }

    #[tokio::test]
    async fn wait_for_ready_returns_when_health_endpoint_is_available() {
        let addr = spawn_health_server().await;
        let app_paths = AppPaths::new(test_dir("ready-server"));
        let config = AppConfig {
            vlm_host: format!("{}:{}", addr.ip(), addr.port()),
            ..AppConfig::default()
        };

        let mut server =
            LlamaServer::from_config(&config, &app_paths).expect("server should initialize");

        server
            .wait_for_ready(1)
            .await
            .expect("health endpoint should become ready");
    }

    #[tokio::test]
    async fn wait_for_ready_times_out_when_no_server_is_available() {
        let app_paths = AppPaths::new(test_dir("timeout-server"));
        let config = AppConfig {
            vlm_host: "127.0.0.1:9".to_string(),
            ..AppConfig::default()
        };

        let mut server =
            LlamaServer::from_config(&config, &app_paths).expect("server should initialize");

        let error = server
            .wait_for_ready(0)
            .await
            .expect_err("missing health endpoint should time out");
        assert!(matches!(error, VlmError::StartupTimeout));
    }
}
