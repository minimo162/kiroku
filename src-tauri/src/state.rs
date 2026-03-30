use std::sync::Arc;

use tauri::{Manager, Runtime};
use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
};

use crate::{
    config::{load_or_create_config_for_manager, AppPaths, ConfigError},
    db::{initialize_db, DbError},
    models::{AppConfig, CaptureStats, VlmState},
    vlm::server::{LlamaServer, VlmError},
};

#[derive(Debug, thiserror::Error)]
pub enum AppStateError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Vlm(#[from] VlmError),
}

#[derive(Clone)]
pub struct AppState {
    pub is_recording: Arc<Mutex<bool>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub capture_stats: Arc<Mutex<CaptureStats>>,
    pub vlm_state: Arc<Mutex<VlmState>>,
    pub vlm_server: Arc<Mutex<LlamaServer>>,
    pub vlm_batch_stop_signal: Arc<Mutex<Option<watch::Sender<bool>>>>,
    pub vlm_batch_pause_signal: Arc<Mutex<Option<watch::Sender<bool>>>>,
    pub vlm_batch_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub previous_dhash: Arc<Mutex<Option<u64>>>,
    pub stop_signal: Arc<Mutex<Option<watch::Sender<bool>>>>,
    pub recording_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub db: Arc<Mutex<rusqlite::Connection>>,
    pub app_paths: Arc<AppPaths>,
}

impl AppState {
    pub fn new<R: Runtime, M: Manager<R>>(manager: &M) -> Result<Self, AppStateError> {
        let (config, app_paths) = load_or_create_config_for_manager(manager)?;
        let db = initialize_db(&app_paths.db_path)?;
        let vlm_server = LlamaServer::from_config(&config, &app_paths)?;

        Ok(Self {
            is_recording: Arc::new(Mutex::new(false)),
            config: Arc::new(Mutex::new(config)),
            capture_stats: Arc::new(Mutex::new(CaptureStats::default())),
            vlm_state: Arc::new(Mutex::new(VlmState::default())),
            vlm_server: Arc::new(Mutex::new(vlm_server)),
            vlm_batch_stop_signal: Arc::new(Mutex::new(None)),
            vlm_batch_pause_signal: Arc::new(Mutex::new(None)),
            vlm_batch_task: Arc::new(Mutex::new(None)),
            previous_dhash: Arc::new(Mutex::new(None)),
            stop_signal: Arc::new(Mutex::new(None)),
            recording_task: Arc::new(Mutex::new(None)),
            db: Arc::new(Mutex::new(db)),
            app_paths: Arc::new(app_paths),
        })
    }

    pub async fn shutdown_vlm_server(&self) {
        let stop_result = {
            let mut server = self.vlm_server.lock().await;
            server.stop()
        };

        let mut vlm_state = self.vlm_state.lock().await;
        vlm_state.server_running = false;
        vlm_state.last_error = stop_result.err().map(|error| error.to_string());
    }

    pub fn shutdown_vlm_server_blocking(&self) {
        tauri::async_runtime::block_on(self.shutdown_vlm_server());
    }
}
