use std::sync::Arc;

use tauri::{Manager, Runtime};
use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
};

use crate::{
    config::{load_or_create_config_for_manager, AppPaths, ConfigError},
    models::{AppConfig, CaptureStats, VlmState},
};

#[derive(Clone)]
pub struct AppState {
    pub is_recording: Arc<Mutex<bool>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub capture_stats: Arc<Mutex<CaptureStats>>,
    pub vlm_state: Arc<Mutex<VlmState>>,
    pub previous_dhash: Arc<Mutex<Option<u64>>>,
    pub stop_signal: Arc<Mutex<Option<watch::Sender<bool>>>>,
    pub recording_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub app_paths: Arc<AppPaths>,
}

impl AppState {
    pub fn new<R: Runtime, M: Manager<R>>(manager: &M) -> Result<Self, ConfigError> {
        let (config, app_paths) = load_or_create_config_for_manager(manager)?;

        Ok(Self {
            is_recording: Arc::new(Mutex::new(false)),
            config: Arc::new(Mutex::new(config)),
            capture_stats: Arc::new(Mutex::new(CaptureStats::default())),
            vlm_state: Arc::new(Mutex::new(VlmState::default())),
            previous_dhash: Arc::new(Mutex::new(None)),
            stop_signal: Arc::new(Mutex::new(None)),
            recording_task: Arc::new(Mutex::new(None)),
            app_paths: Arc::new(app_paths),
        })
    }
}
