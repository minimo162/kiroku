use std::process::Command;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{config::save_config, models::AppConfig, state::AppState};

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatus {
    pub setup_complete: bool,
    pub engine_ready: bool,
    pub node_available: bool,
    pub copilot_server_available: bool,
    pub edge_debugging_ready: bool,
    pub edge_debugging_url: String,
}

#[tauri::command]
pub async fn get_setup_status(state: State<'_, AppState>) -> Result<SetupStatus, String> {
    Ok(build_setup_status(state.inner()).await)
}

#[tauri::command]
pub async fn complete_setup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppConfig, String> {
    let mut config = state.config.lock().await.clone();
    config.setup_complete = true;
    config.vlm_engine = "copilot".to_string();
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
    let node_available = Command::new("node")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    let copilot_server_available = {
        let server = state.copilot_server.lock().await;
        server.script_path().is_some()
    };
    let edge_debugging_url = format!("http://127.0.0.1:{}/json/version", config.edge_cdp_port);
    let edge_debugging_ready = if let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    {
        client
            .get(&edge_debugging_url)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    } else {
        false
    };

    SetupStatus {
        setup_complete: config.setup_complete,
        engine_ready: node_available && copilot_server_available,
        node_available,
        copilot_server_available,
        edge_debugging_ready,
        edge_debugging_url,
    }
}
