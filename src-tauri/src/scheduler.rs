use chrono::{DateTime, Local, LocalResult, NaiveTime, TimeDelta, TimeZone, Timelike};
use tauri::{AppHandle, Emitter};
use tokio::{
    sync::{OwnedSemaphorePermit, watch},
    time::Duration,
};
use uuid::Uuid;

use crate::{
    db::{
        count_unprocessed_captures, daily_record_exists, get_hourly_summaries_since,
        get_sessions_in_period, hourly_summary_exists, insert_daily_record, insert_hourly_summary,
    },
    models::AppConfig,
    state::AppState,
    vlm::{
        batch::{run_vlm_batch_inner, RunBatchRequest},
        inference::{summarize_text, TextPromptContext},
        server::{update_vlm_state, VlmError},
    },
};

const DEFAULT_BATCH_TIMES: &[&str] = &["12:00", "17:45"];
const HOURLY_SUMMARY_INTERVAL_SECS: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatchType {
    Morning,
    Afternoon,
}

pub fn spawn_scheduler(app: AppHandle, state: AppState) {
    let config_rx = state.config_tx.subscribe();
    tauri::async_runtime::spawn(async move {
        batch_scheduler_loop(app, state, config_rx).await;
    });
}

pub fn spawn_hourly_summarizer(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        hourly_summarizer_loop(app, state).await;
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

async fn hourly_summarizer_loop(app: AppHandle, state: AppState) {
    tokio::time::sleep(Duration::from_secs(secs_until_next_hour())).await;

    loop {
        {
            let vlm_state = state.vlm_state.lock().await;
            if vlm_state.batch_running {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        }

        let config = state.config.lock().await.clone();
        if config.vlm_engine == "copilot" && config.setup_complete {
            if let Err(error) = generate_hourly_summary(&app, &state).await {
                eprintln!("[hourly-summary] failed: {error}");
            }
        }

        tokio::time::sleep(Duration::from_secs(HOURLY_SUMMARY_INTERVAL_SECS)).await;
    }
}

async fn run_scheduled_batch(app: AppHandle, state: AppState) -> Result<(), String> {
    let unprocessed_count = {
        let db = state.db.lock().await;
        count_unprocessed_captures(&db).map_err(|error| error.to_string())?
    };
    if unprocessed_count > 0 {
        let started = run_vlm_batch_inner(
            app.clone(),
            state.clone(),
            RunBatchRequest {
                auto_delete: None,
                model_path: None,
                mmproj_path: None,
                n_threads: None,
                max_concurrency: Some(1),
                stop_server_when_done: true,
                notify_on_completion: true,
                include_active_session: false,
            },
        )
        .await?;

        if !started {
            return Err("VLM バッチはすでに実行中です".to_string());
        }
    }

    generate_pending_hourly_summaries(&app, &state).await?;
    generate_daily_record(&app, &state, determine_current_batch_type()).await?;

    Ok(())
}

async fn update_next_run(app: &AppHandle, state: &AppState, next_run_at: Option<String>) {
    state.set_next_batch_run_at(next_run_at.clone()).await;
    let _ = app.emit("scheduler-status", &next_run_at);
}

pub fn next_run_at(now: DateTime<Local>, config: &AppConfig) -> Option<DateTime<Local>> {
    if !config.scheduler_enabled {
        return None;
    }

    let parsed_times = if config.batch_times.is_empty() {
        DEFAULT_BATCH_TIMES
            .iter()
            .filter_map(|time| NaiveTime::parse_from_str(time, "%H:%M").ok())
            .collect::<Vec<_>>()
    } else {
        let configured = config
            .batch_times
            .iter()
            .filter_map(|time| NaiveTime::parse_from_str(time, "%H:%M").ok())
            .collect::<Vec<_>>();

        if configured.is_empty() {
            DEFAULT_BATCH_TIMES
                .iter()
                .filter_map(|time| NaiveTime::parse_from_str(time, "%H:%M").ok())
                .collect::<Vec<_>>()
        } else {
            configured
        }
    };

    parsed_times
        .iter()
        .flat_map(|time| {
            let today = now.date_naive().and_time(*time);
            let tomorrow = today + TimeDelta::days(1);
            [today, tomorrow]
        })
        .filter_map(|candidate| match Local.from_local_datetime(&candidate) {
            LocalResult::Single(value) => Some(value),
            LocalResult::Ambiguous(earliest, _) => Some(earliest),
            LocalResult::None => None,
        })
        .filter(|candidate| *candidate > now)
        .min()
}

async fn generate_hourly_summary(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let period_end = truncate_to_hour(Local::now());
    let period_start = period_end - TimeDelta::hours(1);
    generate_hourly_summary_for_period(app, state, period_start, period_end).await
}

async fn generate_pending_hourly_summaries(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let now = Local::now();
    let mut period_start = start_of_day(now);
    let current_hour = truncate_to_hour(now);

    while period_start < current_hour {
        let period_end = period_start + TimeDelta::hours(1);
        generate_hourly_summary_for_period(app, state, period_start, period_end).await?;
        period_start = period_end;
    }

    if now > current_hour {
        generate_hourly_summary_for_period(app, state, current_hour, now).await?;
    }

    Ok(())
}

async fn generate_hourly_summary_for_period(
    app: &AppHandle,
    state: &AppState,
    period_start: DateTime<Local>,
    period_end: DateTime<Local>,
) -> Result<(), String> {
    if period_end <= period_start {
        return Ok(());
    }

    let period_start_rfc3339 = period_start.to_rfc3339();
    let period_end_rfc3339 = period_end.to_rfc3339();
    let sessions = {
        let db = state.db.lock().await;
        if hourly_summary_exists(&db, &period_start_rfc3339, &period_end_rfc3339)
            .map_err(|error| error.to_string())?
        {
            return Ok(());
        }

        get_sessions_in_period(&db, &period_start_rfc3339, &period_end_rfc3339)
            .map_err(|error| error.to_string())?
    };

    if sessions.is_empty() {
        return Ok(());
    }

    let session_lines = sessions
        .iter()
        .map(|session| {
            format!(
                "- {}〜{}: {}",
                format_time_hhmm(&session.start_time),
                format_time_hhmm(&session.end_time),
                session.description.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let config = state.config.lock().await.clone();
    let user_prompt = config
        .hourly_summary_prompt
        .replace("{start_time}", &period_start.format("%H:%M").to_string())
        .replace("{end_time}", &period_end.format("%H:%M").to_string())
        .replace("{sessions}", &session_lines);
    let server_url = ensure_copilot_running(app, state).await?;
    let permit = acquire_copilot_permit(state).await.map_err(|error| error.to_string())?;
    let summary_result = summarize_text(
        &reqwest::Client::new(),
        &server_url,
        config.vlm_max_tokens,
        TextPromptContext {
            system_prompt: &config.system_prompt,
            user_prompt: &user_prompt,
        },
    )
    .await;
    drop(permit);

    let summary = match summary_result {
        Ok(summary) => summary,
        Err(VlmError::LoginRequired(message)) => {
            let _ = app.emit("copilot-login-required", message.clone());
            return Err(message);
        }
        Err(error) => return Err(error.to_string()),
    };

    let source_session_ids =
        serde_json::to_string(&sessions.iter().map(|session| &session.id).collect::<Vec<_>>())
            .map_err(|error| error.to_string())?;

    let db = state.db.lock().await;
    insert_hourly_summary(
        &db,
        &Uuid::new_v4().to_string(),
        &period_start_rfc3339,
        &period_end_rfc3339,
        &source_session_ids,
        &summary,
    )
    .map_err(|error| error.to_string())
}

async fn generate_daily_record(
    app: &AppHandle,
    state: &AppState,
    batch_type: BatchType,
) -> Result<(), String> {
    let now = Local::now();
    let since = match batch_type {
        BatchType::Morning => start_of_day(now),
        BatchType::Afternoon => start_of_day(now) + TimeDelta::hours(12),
    };
    let record_date = now.format("%Y-%m-%d").to_string();
    let period_type = match batch_type {
        BatchType::Morning => "morning",
        BatchType::Afternoon => "afternoon",
    };
    let period_label = match batch_type {
        BatchType::Morning => "午前",
        BatchType::Afternoon => "午後",
    };

    let since_rfc3339 = since.to_rfc3339();
    let summaries = {
        let db = state.db.lock().await;
        if daily_record_exists(&db, &record_date, period_type).map_err(|error| error.to_string())? {
            return Ok(());
        }

        get_hourly_summaries_since(&db, &since_rfc3339).map_err(|error| error.to_string())?
    };

    if summaries.is_empty() {
        return Ok(());
    }

    let summary_lines = summaries
        .iter()
        .map(|summary| {
            format!(
                "- {}〜{}: {}",
                format_time_hhmm(&summary.period_start),
                format_time_hhmm(&summary.period_end),
                summary.summary
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let config = state.config.lock().await.clone();
    let user_prompt = config
        .daily_record_prompt
        .replace("{date}", &record_date)
        .replace("{period}", period_label)
        .replace("{summaries}", &summary_lines);
    let server_url = ensure_copilot_running(app, state).await?;
    let permit = acquire_copilot_permit(state).await.map_err(|error| error.to_string())?;
    let record_result = summarize_text(
        &reqwest::Client::new(),
        &server_url,
        config.vlm_max_tokens,
        TextPromptContext {
            system_prompt: &config.system_prompt,
            user_prompt: &user_prompt,
        },
    )
    .await;
    drop(permit);

    let record = match record_result {
        Ok(record) => record,
        Err(VlmError::LoginRequired(message)) => {
            let _ = app.emit("copilot-login-required", message.clone());
            return Err(message);
        }
        Err(error) => return Err(error.to_string()),
    };

    let source_summary_ids =
        serde_json::to_string(&summaries.iter().map(|summary| &summary.id).collect::<Vec<_>>())
            .map_err(|error| error.to_string())?;

    let db = state.db.lock().await;
    insert_daily_record(
        &db,
        &Uuid::new_v4().to_string(),
        &record_date,
        period_type,
        &source_summary_ids,
        &record,
    )
    .map_err(|error| error.to_string())
}

fn determine_current_batch_type() -> BatchType {
    if Local::now().hour() < 13 {
        BatchType::Morning
    } else {
        BatchType::Afternoon
    }
}

async fn ensure_copilot_running(app: &AppHandle, state: &AppState) -> Result<String, String> {
    let data_dir = state.app_paths.data_dir.clone();
    let mut started = false;
    let server_url = {
        let mut server = state.copilot_server.lock().await;
        if server.health_check().await.is_err() {
            server
                .start(&data_dir)
                .await
                .map_err(|error| error.to_string())?;
            started = true;
        }
        server.server_url()
    };

    if started {
        let snapshot = update_vlm_state(state, Some(true), None, None).await;
        let _ = app.emit("vlm-status", &snapshot);
    }

    Ok(server_url)
}

async fn acquire_copilot_permit(state: &AppState) -> Result<OwnedSemaphorePermit, VlmError> {
    state
        .copilot_semaphore
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| VlmError::InvalidResponse("copilot semaphore closed".to_string()))
}

fn secs_until_next_hour() -> u64 {
    let now = Local::now();
    let next_hour = truncate_to_hour(now + TimeDelta::hours(1));
    next_hour.signed_duration_since(now).num_seconds().max(0) as u64
}

fn truncate_to_hour(value: DateTime<Local>) -> DateTime<Local> {
    value
        .with_minute(0)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(value)
}

fn start_of_day(value: DateTime<Local>) -> DateTime<Local> {
    value
        .with_hour(0)
        .and_then(|value| value.with_minute(0))
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(value)
}

fn format_time_hhmm(ts: &str) -> String {
    use chrono::DateTime as FixedDateTime;

    let Ok(timestamp) = FixedDateTime::parse_from_rfc3339(ts) else {
        return ts.to_string();
    };
    timestamp.format("%H:%M").to_string()
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
    fn next_run_at_rolls_to_next_day_after_last_batch_time() {
        let config = AppConfig {
            batch_times: vec!["17:45".to_string()],
            ..AppConfig::default()
        };

        let now = Local
            .with_ymd_and_hms(2026, 4, 2, 23, 15, 0)
            .single()
            .expect("time should resolve");

        let next = next_run_at(now, &config).expect("next run should be calculated");

        assert_eq!(next.date_naive().to_string(), "2026-04-03");
        assert_eq!(next.format("%H:%M").to_string(), "17:45");
    }

    #[test]
    fn next_run_at_uses_nearest_future_batch_time_before_noon() {
        let config = AppConfig {
            batch_times: vec!["12:00".to_string(), "17:45".to_string()],
            ..AppConfig::default()
        };

        let now = Local
            .with_ymd_and_hms(2026, 4, 2, 11, 0, 0)
            .single()
            .expect("time should resolve");

        let next = next_run_at(now, &config).expect("next run should be calculated");

        assert_eq!(next.date_naive().to_string(), "2026-04-02");
        assert_eq!(next.format("%H:%M").to_string(), "12:00");
    }

    #[test]
    fn next_run_at_uses_evening_batch_time_after_noon() {
        let config = AppConfig {
            batch_times: vec!["12:00".to_string(), "17:45".to_string()],
            ..AppConfig::default()
        };

        let now = Local
            .with_ymd_and_hms(2026, 4, 2, 13, 0, 0)
            .single()
            .expect("time should resolve");

        let next = next_run_at(now, &config).expect("next run should be calculated");

        assert_eq!(next.date_naive().to_string(), "2026-04-02");
        assert_eq!(next.format("%H:%M").to_string(), "17:45");
    }

    #[test]
    fn next_run_at_rolls_to_next_day_noon_after_all_batch_times() {
        let config = AppConfig {
            batch_times: vec!["12:00".to_string(), "17:45".to_string()],
            ..AppConfig::default()
        };

        let now = Local
            .with_ymd_and_hms(2026, 4, 2, 18, 0, 0)
            .single()
            .expect("time should resolve");

        let next = next_run_at(now, &config).expect("next run should be calculated");

        assert_eq!(next.date_naive().to_string(), "2026-04-03");
        assert_eq!(next.format("%H:%M").to_string(), "12:00");
    }
}
