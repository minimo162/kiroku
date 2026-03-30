use chrono::{DateTime, Local, LocalResult, NaiveTime, TimeZone};
use tauri::{AppHandle, Emitter};
use tokio::{sync::watch, time::Duration};

use crate::{
    db::count_unprocessed_captures,
    models::AppConfig,
    state::AppState,
    vlm::batch::{run_vlm_batch_inner, RunBatchRequest},
    vlm::server::update_vlm_state,
};

const DEFAULT_BATCH_TIME: &str = "22:00";

pub fn spawn_scheduler(app: AppHandle, state: AppState) {
    let config_rx = state.config_tx.subscribe();
    tauri::async_runtime::spawn(async move {
        batch_scheduler_loop(app, state, config_rx).await;
    });
}

pub async fn batch_scheduler_loop(
    app: AppHandle,
    state: AppState,
    mut config_rx: watch::Receiver<AppConfig>,
) {
    loop {
        let config = config_rx.borrow().clone();
        let Some(next_run) = next_run_at(Local::now(), &config) else {
            update_next_run(&app, &state, None).await;
            if config_rx.changed().await.is_err() {
                break;
            }
            continue;
        };

        update_next_run(&app, &state, Some(next_run.to_rfc3339())).await;
        let now = Local::now();
        let sleep_for = next_run
            .signed_duration_since(now)
            .to_std()
            .unwrap_or_else(|_| Duration::from_secs(0));

        tokio::select! {
            _ = tokio::time::sleep(sleep_for) => {
                if let Err(error) = run_scheduled_batch(app.clone(), state.clone()).await {
                    let snapshot = update_vlm_state(&state, None, None, Some(error.clone())).await;
                    let _ = app.emit("vlm-status", &snapshot);
                    let next_run_at = state.next_batch_run_at.lock().await.clone();
                    let _ = app.emit("scheduler-status", &next_run_at);
                }
            }
            changed = config_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
        }
    }
}

async fn run_scheduled_batch(app: AppHandle, state: AppState) -> Result<(), String> {
    let unprocessed_count = {
        let db = state.db.lock().await;
        count_unprocessed_captures(&db).map_err(|error| error.to_string())?
    };
    if unprocessed_count == 0 {
        return Ok(());
    }

    let started = run_vlm_batch_inner(
        app,
        state,
        RunBatchRequest {
            auto_delete: None,
            model_path: None,
            mmproj_path: None,
            n_threads: None,
            max_concurrency: Some(1),
            stop_server_when_done: true,
            notify_on_completion: true,
        },
    )
    .await?;

    if started {
        Ok(())
    } else {
        Err("VLM バッチはすでに実行中です".to_string())
    }
}

async fn update_next_run(app: &AppHandle, state: &AppState, next_run_at: Option<String>) {
    state.set_next_batch_run_at(next_run_at.clone()).await;
    let _ = app.emit("scheduler-status", &next_run_at);
}

pub fn next_run_at(now: DateTime<Local>, config: &AppConfig) -> Option<DateTime<Local>> {
    if !config.scheduler_enabled {
        return None;
    }

    let batch_time = NaiveTime::parse_from_str(&config.batch_time, "%H:%M").unwrap_or_else(|_| {
        NaiveTime::parse_from_str(DEFAULT_BATCH_TIME, "%H:%M")
            .expect("default batch time should parse")
    });

    let mut next_run = now.date_naive().and_time(batch_time);
    if next_run <= now.naive_local() {
        next_run += chrono::Duration::days(1);
    }

    match Local.from_local_datetime(&next_run) {
        LocalResult::Single(value) => Some(value),
        LocalResult::Ambiguous(earliest, _) => Some(earliest),
        LocalResult::None => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use crate::models::AppConfig;

    use super::next_run_at;

    #[test]
    fn next_run_at_returns_none_when_scheduler_is_disabled() {
        let config = AppConfig {
            scheduler_enabled: false,
            ..AppConfig::default()
        };

        let now = Local
            .with_ymd_and_hms(2026, 4, 2, 12, 0, 0)
            .single()
            .expect("time should resolve");

        assert!(next_run_at(now, &config).is_none());
    }

    #[test]
    fn next_run_at_rolls_to_next_day_after_batch_time() {
        let config = AppConfig {
            batch_time: "22:00".to_string(),
            ..AppConfig::default()
        };

        let now = Local
            .with_ymd_and_hms(2026, 4, 2, 23, 15, 0)
            .single()
            .expect("time should resolve");

        let next = next_run_at(now, &config).expect("next run should be calculated");

        assert_eq!(next.date_naive().to_string(), "2026-04-03");
        assert_eq!(next.format("%H:%M").to_string(), "22:00");
    }
}
