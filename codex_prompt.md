# Codex 実装指示: 階層的要約・日次記録・ログ改善 (T50〜T53)

## 方針

- **T50 → T52 → T51 → T53 の順で実装すること**（依存関係あり）
- `cargo check` でエラーがないことを確認してから次のタスクへ進むこと
- 既存テスト（`cargo test`）が壊れないこと
- 各タスク完了後に `cargo test` を実行して既存テストが通ることを確認

## 背景

現在のアーキテクチャ:
- キャプチャ: 10秒ごとにスクリーンショット取得
- セッション: 5分（session_window_secs=300）ごとにコラージュを作成
- バッチ: 12:00 と 17:30 に未処理セッションの説明文を Copilot で生成

**追加する3層要約:**
- L1（既存）: セッション説明文（5分ごと、コラージュ画像ベース）
- L2（新規）: 時間帯要約（60分ごと、L1 説明文のテキスト要約）
- L3（新規）: 日次記録（バッチ時、L2 要約を基にした半日/1日のまとめ）

---

## T50: L2 時間帯要約の定期生成（60分ごと）

### 目的
60分ごとにその時間帯のセッション説明文（L1）を Copilot でテキスト要約し、中間要約（L2）として DB に保存する。これにより、日次記録（L3）生成時のプロンプト文字数を抑制する。

### 対象ファイル
- `src-tauri/src/db.rs`（マイグレーション + クエリ追加）
- `src-tauri/src/models.rs`（プロンプトテンプレート追加）
- `src-tauri/src/vlm/inference.rs`（テキストのみ要約関数追加）
- `src-tauri/src/scheduler.rs`（60分タイマー追加）

### 変更内容

#### 1. DB マイグレーション（db.rs）

`apply_migrations()` に version < 4 のマイグレーションを追加:

```rust
if version < 4 {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS hourly_summaries (
            id TEXT PRIMARY KEY,
            period_start TEXT NOT NULL,
            period_end TEXT NOT NULL,
            source_session_ids TEXT NOT NULL,
            summary TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_hourly_summaries_period
            ON hourly_summaries(period_start);

        CREATE TABLE IF NOT EXISTS daily_records (
            id TEXT PRIMARY KEY,
            record_date TEXT NOT NULL,
            period_type TEXT NOT NULL,
            source_summary_ids TEXT NOT NULL,
            record TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_daily_records_date
            ON daily_records(record_date);
        ",
    )?;
    conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
}
```

`DB_SCHEMA_VERSION` を 4 に更新すること。

**クエリ関数を追加:**

```rust
/// 指定時間帯のセッション説明文を取得（L2 要約の入力）
pub fn get_sessions_in_period(
    conn: &Connection,
    period_start: &str,
    period_end: &str,
) -> Result<Vec<SessionRecord>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, start_time, end_time, collage_path, description, processed, capture_count, frame_count, created_at
         FROM sessions
         WHERE processed = 1
           AND description IS NOT NULL
           AND start_time >= ?1
           AND start_time < ?2
         ORDER BY start_time ASC"
    )?;
    // ... map rows to SessionRecord
}

/// 既に L2 要約が生成済みかチェック
pub fn hourly_summary_exists(
    conn: &Connection,
    period_start: &str,
    period_end: &str,
) -> Result<bool, DbError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM hourly_summaries WHERE period_start = ?1 AND period_end = ?2",
        params![period_start, period_end],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// L2 要約を保存
pub fn insert_hourly_summary(
    conn: &Connection,
    id: &str,
    period_start: &str,
    period_end: &str,
    source_session_ids: &str,  // JSON array
    summary: &str,
) -> Result<(), DbError> { ... }

/// 指定日時以降の L2 要約を取得（L3 生成の入力）
pub fn get_hourly_summaries_since(
    conn: &Connection,
    since: &str,
) -> Result<Vec<HourlySummaryRecord>, DbError> { ... }

/// L3 日次記録を保存
pub fn insert_daily_record(
    conn: &Connection,
    id: &str,
    record_date: &str,
    period_type: &str,  // "morning" or "afternoon"
    source_summary_ids: &str,  // JSON array
    record: &str,
) -> Result<(), DbError> { ... }
```

#### 2. テキストのみ要約関数（inference.rs）

既存の `describe_screenshot()` は画像付きリクエストを送信する。
L2/L3 は画像なしのテキストのみなので、新しい関数を追加:

```rust
pub struct TextPromptContext<'a> {
    pub system_prompt: &'a str,
    pub user_prompt: &'a str,
}

/// テキストのみで Copilot に要約リクエストを送信する（画像なし）
pub async fn summarize_text(
    client: &Client,
    server_url: &str,
    max_tokens: u32,
    prompt_context: TextPromptContext<'_>,
) -> Result<String, VlmError> {
    let endpoint = format!("{}/v1/chat/completions", server_url.trim_end_matches('/'));

    let payload = json!({
        "model": "qwen",
        "messages": [{
            "role": "system",
            "content": prompt_context.system_prompt
        }, {
            "role": "user",
            "content": prompt_context.user_prompt
        }],
        "max_tokens": max_tokens,
        "temperature": 0.1
    });

    // リトライロジックは describe_screenshot と同様
    for attempt in 0..MAX_RETRIES {
        let response = client
            .post(&endpoint)
            .json(&payload)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await?;

        if !response.status().is_success() {
            // 既存のエラーハンドリングと同様（LoginRequired 検出含む）
            ...
        }

        let response = response.json::<serde_json::Value>().await?;
        match extract_description(&response) {
            Ok(text) => return Ok(text),
            Err(error) if attempt + 1 < MAX_RETRIES && should_retry(&error) => {
                let backoff = INITIAL_BACKOFF_MS * (1_u64 << attempt);
                sleep(Duration::from_millis(backoff)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(VlmError::InvalidResponse("Text summarization exhausted all retries".to_string()))
}
```

#### 3. L2 プロンプトテンプレート（models.rs）

```rust
pub fn default_hourly_summary_prompt() -> String {
    concat!(
        "以下は {start_time} から {end_time} の間の業務セッション記録です。\n\n",
        "{sessions}\n\n",
        "この時間帯の業務を2〜3文で要約してください。",
        "使用したアプリケーション、主な操作内容、対象データを含めてください。",
        "出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。"
    ).to_string()
}
```

`AppConfig` に `hourly_summary_prompt: String` フィールドを追加（デフォルト: `default_hourly_summary_prompt()`）。

#### 4. 60分タイマー（scheduler.rs）

`spawn_scheduler()` と並行して `spawn_hourly_summarizer()` を追加:

```rust
const HOURLY_SUMMARY_INTERVAL_SECS: u64 = 3600;

pub fn spawn_hourly_summarizer(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        hourly_summarizer_loop(app, state).await;
    });
}

async fn hourly_summarizer_loop(app: AppHandle, state: AppState) {
    // 次の正時まで待機（例: 現在 9:23 なら 10:00 まで待つ）
    let initial_wait = secs_until_next_hour();
    tokio::time::sleep(Duration::from_secs(initial_wait)).await;

    loop {
        // バッチ実行中はスキップ
        {
            let vlm_state = state.vlm_state.lock().await;
            if vlm_state.batch_running {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        }

        // Copilot エンジンが有効な場合のみ実行
        let config = state.config.lock().await.clone();
        if config.vlm_engine == "copilot" && config.setup_complete {
            if let Err(error) = generate_hourly_summary(&app, &state).await {
                eprintln!("[hourly-summary] failed: {error}");
            }
        }

        tokio::time::sleep(Duration::from_secs(HOURLY_SUMMARY_INTERVAL_SECS)).await;
    }
}

fn secs_until_next_hour() -> u64 {
    let now = chrono::Local::now();
    let next_hour = (now + chrono::Duration::hours(1))
        .with_minute(0).unwrap()
        .with_second(0).unwrap();
    (next_hour - now).num_seconds().max(0) as u64
}
```

`generate_hourly_summary()` の実装:
1. 前の60分間（例: 09:00〜10:00）の処理済みセッションを `get_sessions_in_period()` で取得
2. セッションがなければスキップ
3. `hourly_summary_exists()` で重複チェック
4. Copilot エンジンを起動（`ensure_engine_running` 相当）
5. セッション説明文を連結してプロンプトを構築
6. `summarize_text()` を呼び出し
7. `insert_hourly_summary()` で保存

**lib.rs への追加:**

`spawn_scheduler()` の直後に `spawn_hourly_summarizer()` を追加:
```rust
spawn_scheduler(app.handle().clone(), scheduler_state);
spawn_hourly_summarizer(app.handle().clone(), hourly_state);
```

---

## T52: Copilot リクエストキューによる競合防止

### 目的
L1（セッション説明文）、L2（時間帯要約）、L3（日次記録）の Copilot リクエストが同時に発生しないよう直列化する。

### 対象ファイル
- `src-tauri/src/state.rs`（セマフォ追加）
- `src-tauri/src/vlm/batch.rs`（セマフォ利用）
- `src-tauri/src/scheduler.rs`（セマフォ利用）

### 変更内容

#### 1. AppState にセマフォを追加（state.rs）

```rust
use tokio::sync::Semaphore;

pub struct AppState {
    // ... 既存フィールド ...
    pub copilot_semaphore: Arc<Semaphore>,
}
```

`AppState::new()` で初期化:
```rust
copilot_semaphore: Arc::new(Semaphore::new(1)),
```

#### 2. バッチ処理での利用（batch.rs）

`vlm_batch_loop()` と `run_session_batch_loop()` 内の `describe_screenshot()` 呼び出しの前後でセマフォを acquire/release:

```rust
let _permit = state.copilot_semaphore.acquire().await
    .map_err(|_| VlmError::InvalidResponse("semaphore closed".to_string()))?;
let result = describe_screenshot(...).await;
drop(_permit);
```

#### 3. 60分タイマーでの利用（scheduler.rs）

`generate_hourly_summary()` 内の `summarize_text()` 呼び出し前にセマフォを acquire:

```rust
let _permit = state.copilot_semaphore.acquire().await
    .map_err(|error| format!("semaphore: {error}"))?;
let summary = summarize_text(...).await?;
drop(_permit);
```

**注意:** `batch_running == true` の場合は60分タイマーをスキップするので、実際にはバッチとタイマーが同時にセマフォを取り合うことは少ない。セマフォは安全弁としての役割が主。

---

## T51: 日次記録の自動生成（L3: 12:00 / 17:45 バッチ）

### 目的
バッチ実行時に L1 → L2 → L3 の3段階処理を行い、日次記録を自動生成する。
DEFAULT_BATCH_TIMES を 17:30 → 17:45 に変更する。

### 対象ファイル
- `src-tauri/src/scheduler.rs`（DEFAULT_BATCH_TIMES 変更 + バッチフロー拡張）
- `src-tauri/src/config.rs`（マイグレーション追加）
- `src-tauri/src/models.rs`（L3 プロンプト追加）
- `src-tauri/src/vlm/batch.rs`（L2 + L3 処理を追加）

### 変更内容

#### 1. DEFAULT_BATCH_TIMES の変更（scheduler.rs）

```rust
// 変更前
const DEFAULT_BATCH_TIMES: &[&str] = &["12:00", "17:30"];
// 変更後
const DEFAULT_BATCH_TIMES: &[&str] = &["12:00", "17:45"];
```

#### 2. config.rs にマイグレーション追加

```rust
// batch_times の 17:30 → 17:45 マイグレーション
for time in &mut config.batch_times {
    if time == "17:30" {
        *time = "17:45".to_string();
    }
}
```

#### 3. L3 プロンプトテンプレート（models.rs）

```rust
pub fn default_daily_record_prompt() -> String {
    concat!(
        "以下は {date} の {period} の時間帯別業務要約です。\n\n",
        "{summaries}\n\n",
        "この期間の業務内容を3〜5文でまとめてください。",
        "主な業務カテゴリ、使用アプリケーション、作業の流れを含めてください。",
        "出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。"
    ).to_string()
}
```

`AppConfig` に `daily_record_prompt: String` フィールドを追加。

#### 4. バッチフロー拡張（batch.rs + scheduler.rs）

`run_scheduled_batch()` を拡張して3段階にする:

```rust
async fn run_scheduled_batch(app: AppHandle, state: AppState) -> Result<(), String> {
    // Phase 1: 既存の L1 セッション説明文バッチ
    let unprocessed_count = {
        let db = state.db.lock().await;
        count_unprocessed_captures(&db).map_err(|e| e.to_string())?
    };
    if unprocessed_count > 0 {
        run_vlm_batch_inner(app.clone(), state.clone(), RunBatchRequest { ... }).await?;
    }

    // Phase 2: 未生成の L2 時間帯要約を回収
    generate_pending_hourly_summaries(&app, &state).await?;

    // Phase 3: L3 日次記録の生成
    let batch_time = determine_current_batch_type(&state).await;
    generate_daily_record(&app, &state, batch_time).await?;

    Ok(())
}
```

**L3 対象範囲の決定:**

```rust
enum BatchType {
    Morning,   // 12:00 バッチ → 当日 00:00〜現在の全 L2 要約
    Afternoon, // 17:45 バッチ → 当日 12:00〜現在の L2 要約
}

fn determine_current_batch_type(state: &AppState) -> BatchType {
    let now = chrono::Local::now();
    if now.hour() < 13 {
        BatchType::Morning
    } else {
        BatchType::Afternoon
    }
}
```

`generate_daily_record()` の実装:
1. `BatchType` に応じて L2 要約の対象期間を決定
   - Morning: `{today}T00:00:00` 〜 現在
   - Afternoon: `{today}T12:00:00` 〜 現在
2. `get_hourly_summaries_since()` で L2 要約を取得
3. L2 要約を連結してプロンプトを構築
4. `summarize_text()` で L3 を生成
5. `insert_daily_record()` で保存

**テスト（scheduler.rs の既存テストを更新）:**

```rust
#[test]
fn next_run_at_uses_evening_batch_time_after_noon() {
    let config = AppConfig {
        batch_times: vec!["12:00".to_string(), "17:45".to_string()],
        ..AppConfig::default()
    };
    // 17:30 → 17:45 に変更
    ...
}
```

---

## T53: セッション説明文へのアプリ名付与とログ出力の説明文単位化

### 目的
1. セッションバッチ時にセッション内キャプチャのアプリ名上位をプロンプトに含める
2. CSV エクスポートにセッション単位出力モードを追加する

### 対象ファイル
- `src-tauri/src/db.rs`（アプリ集計クエリ追加）
- `src-tauri/src/vlm/batch.rs`（PromptContext にアプリ名を渡す）
- `src-tauri/src/export.rs`（セッション単位出力追加）
- `src/lib/settings/ExportCard.svelte`（UIオプション追加）

### 変更内容

#### 1. アプリ名集計クエリ（db.rs）

```rust
/// セッション内のキャプチャからアプリ名上位 N 件を取得
pub fn get_top_apps_for_session(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT app, COUNT(*) as cnt
         FROM captures
         WHERE session_id = ?1 AND app != 'Unknown'
         GROUP BY app
         ORDER BY cnt DESC
         LIMIT ?2"
    )?;
    let rows = stmt.query_map(
        params![session_id, limit as i64],
        |row| row.get::<_, String>(0),
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}

/// セッション内の主要ウィンドウタイトル上位 N 件を取得
pub fn get_top_window_titles_for_session(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT window_title, COUNT(*) as cnt
         FROM captures
         WHERE session_id = ?1 AND window_title != 'Unknown'
         GROUP BY window_title
         ORDER BY cnt DESC
         LIMIT ?2"
    )?;
    let rows = stmt.query_map(
        params![session_id, limit as i64],
        |row| row.get::<_, String>(0),
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
}
```

#### 2. PromptContext にアプリ名を渡す（batch.rs）

`run_session_batch_loop()` 内のセッション処理ループで:

```rust
// 変更前
PromptContext {
    app: None,
    window_title: None,
    system_prompt: Some(&config.system_prompt),
    user_prompt: Some(&user_prompt),
}

// 変更後
let top_apps = {
    let db = state.db.lock().await;
    get_top_apps_for_session(&db, &session.id, 3)
        .unwrap_or_default()
        .join(", ")
};
let top_titles = {
    let db = state.db.lock().await;
    get_top_window_titles_for_session(&db, &session.id, 3)
        .unwrap_or_default()
        .join(", ")
};

PromptContext {
    app: if top_apps.is_empty() { None } else { Some(&top_apps) },
    window_title: if top_titles.is_empty() { None } else { Some(&top_titles) },
    system_prompt: Some(&config.system_prompt),
    user_prompt: Some(&user_prompt),
}
```

#### 3. セッション単位 CSV エクスポート（export.rs）

`ExportFilter` に追加:
```rust
pub struct ExportFilter {
    // ... 既存フィールド ...
    pub group_by_session: bool,
}
```

セッション単位エクスポート関数を追加:
```rust
fn export_sessions_csv(
    conn: &Connection,
    filter: &ExportFilter,
    writer: &mut Writer<File>,
) -> Result<usize, ExportError> {
    // ヘッダー: session_id, start_time, end_time, duration_min, primary_app, description
    writer.write_record(&[
        "session_id", "start_time", "end_time", "duration_min", "primary_app", "description",
    ])?;

    let sessions = query_sessions_filtered(conn, filter)?;
    for session in &sessions {
        let primary_app = get_top_apps_for_session(conn, &session.id, 1)
            .unwrap_or_default()
            .first()
            .cloned()
            .unwrap_or_default();
        let duration = calc_duration_min(&session.start_time, &session.end_time);
        writer.write_record(&[
            &session.id,
            &session.start_time,
            &session.end_time,
            &duration.to_string(),
            &primary_app,
            session.description.as_deref().unwrap_or(""),
        ])?;
    }

    Ok(sessions.len())
}
```

#### 4. フロントエンド（ExportCard.svelte）

エクスポート画面に「出力単位」の切り替えオプションを追加:
- ラジオボタン: 「キャプチャ単位」（既存） / 「セッション単位」（新規）
- デフォルト: 「セッション単位」

---

## 完了条件チェックリスト

### T50: L2 時間帯要約
- [ ] DB マイグレーション（hourly_summaries, daily_records テーブル）が正常に実行される
- [ ] `summarize_text()` が画像なしテキストのみリクエストを送信する
- [ ] 60分ごとにタイマーが発火し、L2 要約が生成・保存される
- [ ] `batch_running == true` の場合はタイマーがスキップされる
- [ ] セッション説明文がない時間帯はスキップされる

### T52: Copilot リクエストキュー
- [ ] `copilot_semaphore` が AppState に追加されている
- [ ] Copilot へのリクエストがセマフォで直列化されている
- [ ] L1 と L2 が同時に Copilot に送信されない

### T51: L3 日次記録
- [ ] DEFAULT_BATCH_TIMES が `["12:00", "17:45"]` になっている
- [ ] config.rs で "17:30" → "17:45" のマイグレーションが実行される
- [ ] 12:00 バッチで当日全体の L2 要約を基に L3 が生成される
- [ ] 17:45 バッチで 12:00 以降の L2 要約を基に L3 が生成される
- [ ] バッチが L1 → L2 → L3 の順で実行される
- [ ] 既存テストが 17:45 に更新されている

### T53: アプリ名付与とログ単位化
- [ ] セッション説明文のプロンプトにアプリ名上位3件が含まれる
- [ ] CSV エクスポートでセッション単位出力が動作する
- [ ] 既存のキャプチャ単位出力に影響がない
- [ ] `cargo check` がエラーなしで通過する
- [ ] `cargo test` が全テスト通過する
