use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use reqwest::Client;
use tokio::time::sleep;

use crate::{config::AppPaths, models::AppConfig, vlm::server::VlmError};

const READY_TIMEOUT_SECS: u64 = 30;
const HEALTH_POLL_INTERVAL_MS: u64 = 500;

#[derive(Debug)]
pub struct CopilotServer {
    process: Option<Child>,
    port: u16,
    cdp_port: u16,
    client: Client,
    script_path: Option<PathBuf>,
}

impl CopilotServer {
    pub fn new(config: &AppConfig, app_paths: &AppPaths) -> Result<Self, VlmError> {
        Ok(Self {
            process: None,
            port: config.copilot_port,
            cdp_port: config.edge_cdp_port,
            client: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(VlmError::Http)?,
            script_path: resolve_script_path(app_paths),
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

        let child = match Command::new(node)
            .args([
                &script_path.to_string_lossy().into_owned(),
                "--port",
                &self.port.to_string(),
                "--cdp-port",
                &self.cdp_port.to_string(),
            ])
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
