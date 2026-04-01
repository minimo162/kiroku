use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::Client;
use tauri::{AppHandle, Emitter};
use tokio::time::sleep;

use crate::{
    config::AppPaths,
    models::AppConfig,
    state::AppState,
    vlm::server::{update_vlm_state, VlmError},
};

const READY_TIMEOUT_SECS: u64 = 30;
const HEALTH_POLL_INTERVAL_MS: u64 = 500;
const HEALTH_MONITOR_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CopilotStatusResponse {
    connected: bool,
    login_required: bool,
}

#[derive(Debug)]
pub struct CopilotServer {
    process: Option<Child>,
    port: u16,
    cdp_port: u16,
    client: Client,
    script_path: Option<PathBuf>,
    edge_profile_dir: PathBuf,
}

impl CopilotServer {
    pub fn new(config: &AppConfig, app_paths: &AppPaths) -> Result<Self, VlmError> {
        let edge_profile_dir = app_paths.data_dir.join("edge-profile");
        Ok(Self {
            process: None,
            port: config.copilot_port,
            cdp_port: config.edge_cdp_port,
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(VlmError::Http)?,
            script_path: resolve_script_path(app_paths),
            edge_profile_dir,
        })
    }

    pub async fn start(&mut self, data_dir: &Path) -> Result<(), VlmError> {
        if self.health_check().await.is_ok() {
            return Ok(());
        }

        if self.process_running()? {
            self.stop()?;
        }

        let node = find_node().ok_or_else(|| {
            VlmError::InvalidResponse("Node.js runtime could not be found".to_string())
        })?;
        let script_path = self.script_path.clone().ok_or_else(|| {
            VlmError::InvalidResponse("copilot_server.js could not be found".to_string())
        })?;

        let log_path = copilot_server_log_path(data_dir);
        let stderr_file = open_copilot_server_log_file(&log_path)?;

        let args_owned = vec![
            script_path.to_string_lossy().into_owned(),
            "--port".to_string(),
            self.port.to_string(),
            "--cdp-port".to_string(),
            self.cdp_port.to_string(),
            "--user-data-dir".to_string(),
            self.edge_profile_dir.to_string_lossy().into_owned(),
        ];

        let child = match Command::new(node)
            .args(&args_owned)
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file))
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("copilot-server log: {}", log_path.display());
                return Err(error.into());
            }
        };

        self.process = Some(child);
        if let Err(error) = self.wait_for_ready(READY_TIMEOUT_SECS).await {
            let _ = self.stop();
            eprintln!("copilot-server log: {}", log_path.display());
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

    async fn status(&self) -> Result<CopilotStatusResponse, VlmError> {
        let response = self
            .client
            .get(format!("{}/status", self.server_url()))
            .send()
            .await?;

        if response.status().is_success() {
            response
                .json::<CopilotStatusResponse>()
                .await
                .map_err(VlmError::Http)
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
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn script_path(&self) -> Option<&Path> {
        self.script_path.as_deref()
    }
}

fn resolve_script_path(app_paths: &AppPaths) -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(manifest_dir) = option_env!("CARGO_MANIFEST_DIR") {
        candidates.push(PathBuf::from(manifest_dir).join("binaries/copilot_server.js"));
    }

    candidates.push(app_paths.data_dir.join("binaries/copilot_server.js"));

    if let Some(resource_dir) = &app_paths.resource_dir {
        candidates.push(resource_dir.join("binaries/copilot_server.js"));
    }

    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join("binaries/copilot_server.js"));
        }
    }

    candidates.into_iter().find(|path| path.exists())
}

fn find_node() -> Option<PathBuf> {
    for name in &["node", "node.exe"] {
        if Command::new(name)
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
        {
            return Some(PathBuf::from(name));
        }
    }

    None
}

fn copilot_server_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("logs").join("copilot-server.log")
}

fn open_copilot_server_log_file(log_path: &Path) -> Result<fs::File, io::Error> {
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match fs::File::create(log_path) {
        Ok(file) => Ok(file),
        Err(primary_error) => fs::File::create(null_device_path()).map_err(|fallback_error| {
            io::Error::new(
                fallback_error.kind(),
                format!(
                    "failed to open copilot-server log file at {}: {primary_error}; fallback failed: {fallback_error}",
                    log_path.display()
                ),
            )
        }),
    }
}

fn null_device_path() -> &'static str {
    if cfg!(windows) {
        "NUL"
    } else {
        "/dev/null"
    }
}

/// アプリ起動時に Copilot サーバーと Edge を自動接続する。
/// setup_complete かつ vlm_engine == "copilot" の場合のみ動作する。
/// 失敗してもアプリの動作には影響しない（バッチ時に再試行される）。
pub fn spawn_copilot_auto_connect(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = copilot_auto_connect(&app, &state).await {
            eprintln!("[copilot] auto-connect failed (will retry at batch time): {error}");
        }

        let (setup_complete, vlm_engine) = {
            let config = state.config.lock().await;
            (config.setup_complete, config.vlm_engine.clone())
        };
        if setup_complete && vlm_engine == "copilot" {
            copilot_health_monitor_loop(&app, &state).await;
        }
    });
}

async fn copilot_auto_connect(
    app: &AppHandle,
    state: &AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    let (setup_complete, vlm_engine) = {
        let config = state.config.lock().await;
        (config.setup_complete, config.vlm_engine.clone())
    };

    if !setup_complete || vlm_engine != "copilot" {
        return Ok(());
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    let data_dir = state.app_paths.data_dir.clone();
    {
        let mut server = state.copilot_server.lock().await;
        server.start(&data_dir).await?;
    }

    let snapshot = update_vlm_state(state, Some(true), None, None).await;
    let _ = app.emit("vlm-status", &snapshot);

    let status = {
        let server = state.copilot_server.lock().await;
        server.status().await
    };

    if let Ok(status) = status {
        if status.login_required {
            let _ = app.emit(
                "copilot-login-required",
                "Copilot にログインしてください。Edge の画面を確認してください。",
            );
        }
    }

    Ok(())
}

async fn copilot_health_monitor_loop(app: &AppHandle, state: &AppState) {
    #[derive(PartialEq, Clone)]
    enum ConnectionState {
        Connected,
        LoginRequired,
        Disconnected,
    }

    let mut last_state = ConnectionState::Disconnected;
    let client = match Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("[copilot] health monitor client init failed: {error}");
            return;
        }
    };

    loop {
        tokio::time::sleep(Duration::from_secs(HEALTH_MONITOR_INTERVAL_SECS)).await;

        {
            let vlm_state = state.vlm_state.lock().await;
            if vlm_state.batch_running {
                continue;
            }
        }

        {
            let config = state.config.lock().await;
            if config.vlm_engine != "copilot" {
                break;
            }
        }

        let healthy = {
            let server = state.copilot_server.lock().await;
            server.health_check().await.is_ok()
        };

        if !healthy {
            let data_dir = state.app_paths.data_dir.clone();
            let restart_result = {
                let mut server = state.copilot_server.lock().await;
                server.start(&data_dir).await
            };

            match restart_result {
                Ok(()) => {
                    let snapshot = update_vlm_state(state, Some(true), None, None).await;
                    let _ = app.emit("vlm-status", &snapshot);
                    eprintln!("[copilot] health monitor: reconnected after disconnect");
                }
                Err(error) => {
                    if last_state != ConnectionState::Disconnected {
                        let snapshot =
                            update_vlm_state(state, Some(false), None, Some(error.to_string()))
                                .await;
                        let _ = app.emit("vlm-status", &snapshot);
                        last_state = ConnectionState::Disconnected;
                    }
                    continue;
                }
            }
        }

        let server_url = {
            let server = state.copilot_server.lock().await;
            server.server_url()
        };

        let current_state = match client.get(format!("{server_url}/status")).send().await {
            Ok(response) => match response.json::<CopilotStatusResponse>().await {
                Ok(status) if status.login_required => ConnectionState::LoginRequired,
                Ok(status) if status.connected => ConnectionState::Connected,
                _ => ConnectionState::Disconnected,
            },
            Err(_) => ConnectionState::Disconnected,
        };

        if current_state != last_state {
            match &current_state {
                ConnectionState::LoginRequired => {
                    let _ = app.emit(
                        "copilot-login-required",
                        "Copilot にログインしてください。Edge の画面を確認してください。",
                    );
                }
                ConnectionState::Connected => {
                    let snapshot = update_vlm_state(state, Some(true), None, None).await;
                    let _ = app.emit("vlm-status", &snapshot);
                }
                ConnectionState::Disconnected => {}
            }
            last_state = current_state;
        }
    }
}
