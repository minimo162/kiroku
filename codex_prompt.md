# Codex 実装指示: Session Collage Processing (T12〜T16)

前回 T01〜T11 が完了した。以下の T12〜T16 を実装すること。

---

## 前提・方針

- Rust ファイルを変更する場合は必ず事前に `cargo check` を実行し、コンパイルエラーがないことを確認すること
- 既存コードのスタイル（`thiserror`、`Arc<Mutex<T>>`、`params![]` マクロ等）を踏襲すること
- llama エンジン + `session_enabled: false` の動作は一切変えないこと（後退禁止）
- 新しい Rust モジュールを作成したら `mod` 宣言を追加すること

---

## T12: `models.rs` — AppConfig にセッション設定フィールドを追加

### 対象ファイル

`src-tauri/src/models.rs`

### 変更内容

`AppConfig` 構造体に以下のフィールドを追加する:

```rust
pub session_enabled: bool,
pub session_gap_secs: u64,
pub session_window_secs: u64,
pub max_frames_per_collage: u32,
pub session_user_prompt: String,
```

`Default` impl に以下のデフォルト値を追加する:

```rust
session_enabled: true,
session_gap_secs: 600,       // 10分
session_window_secs: 300,    // 5分
max_frames_per_collage: 6,
session_user_prompt: default_session_user_prompt(),
```

`default_session_user_prompt()` 関数を追加する:

```rust
pub fn default_session_user_prompt() -> String {
    concat!(
        "これは {start_time} から {end_time} の間（{duration_min}分間）の",
        "業務画面の流れです。{frame_count} 枚のスクリーンショットを",
        "時系列順に並べたコラージュを見て、この間に行っていた業務操作を",
        "1〜3文で説明してください。必ず次の観点を含めてください: ",
        "使用中のアプリケーション、実行している操作の流れ、",
        "表示されているデータや対象。",
        "出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。"
    )
    .to_string()
}
```

`AppConfig` の既存 unit test `app_config_roundtrip_json` に、新フィールドを追加すること（コンパイルエラー回避）。

---

## T13: `db.rs` — スキーマ v3 マイグレーション + sessions CRUD

### 対象ファイル

`src-tauri/src/db.rs`

### 変更内容

#### 1. バージョン定数を更新

```rust
const DB_SCHEMA_VERSION: i32 = 3;
```

#### 2. `SessionRecord` 構造体を追加（ファイル先頭付近 `use` の後）

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub start_time: String,
    pub end_time: String,
    pub collage_path: Option<String>,
    pub description: Option<String>,
    pub processed: bool,
    pub capture_count: i64,
    pub frame_count: i64,
}
```

#### 3. `apply_migrations` に v3 ブロックを追加

既存の `if version < 2 { ... }` ブロックの後に追加:

```rust
if version < 3 {
    conn.execute_batch(
        "
        ALTER TABLE captures ADD COLUMN session_id TEXT REFERENCES sessions(id);
        CREATE INDEX IF NOT EXISTS idx_captures_session_id ON captures(session_id);

        CREATE TABLE IF NOT EXISTS sessions (
            id             TEXT PRIMARY KEY,
            start_time     TEXT NOT NULL,
            end_time       TEXT NOT NULL,
            collage_path   TEXT,
            description    TEXT,
            processed      INTEGER NOT NULL DEFAULT 0,
            capture_count  INTEGER NOT NULL DEFAULT 0,
            frame_count    INTEGER NOT NULL DEFAULT 0,
            created_at     TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_processed ON sessions(processed);
        CREATE INDEX IF NOT EXISTS idx_sessions_start_time ON sessions(start_time);
        ",
    )?;
    conn.pragma_update(None, "user_version", DB_SCHEMA_VERSION)?;
}
```

#### 4. sessions テーブルの CRUD 関数を追加

```rust
pub fn insert_session(conn: &Connection, record: &SessionRecord) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO sessions (id, start_time, end_time, collage_path, description, processed, capture_count, frame_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            record.id,
            record.start_time,
            record.end_time,
            record.collage_path,
            record.description,
            record.processed as i64,
            record.capture_count,
            record.frame_count,
        ],
    )?;
    Ok(())
}

pub fn get_unprocessed_sessions(conn: &Connection) -> Result<Vec<SessionRecord>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, start_time, end_time, collage_path, description, processed, capture_count, frame_count
         FROM sessions
         WHERE processed = 0
         ORDER BY start_time ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SessionRecord {
            id: row.get(0)?,
            start_time: row.get(1)?,
            end_time: row.get(2)?,
            collage_path: row.get(3)?,
            description: row.get(4)?,
            processed: row.get::<_, i64>(5)? != 0,
            capture_count: row.get(6)?,
            frame_count: row.get(7)?,
        })
    })?;
    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row?);
    }
    Ok(sessions)
}

pub fn mark_session_processed(
    conn: &Connection,
    id: &str,
    description: &str,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE sessions SET processed = 1, description = ?2 WHERE id = ?1",
        params![id, description],
    )?;
    Ok(())
}

pub fn assign_session_to_captures(
    conn: &Connection,
    capture_ids: &[String],
    session_id: &str,
) -> Result<(), DbError> {
    for id in capture_ids {
        conn.execute(
            "UPDATE captures SET session_id = ?2 WHERE id = ?1",
            params![id, session_id],
        )?;
    }
    Ok(())
}

pub fn set_session_collage_path(
    conn: &Connection,
    id: &str,
    collage_path: Option<&str>,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE sessions SET collage_path = ?2 WHERE id = ?1",
        params![id, collage_path],
    )?;
    Ok(())
}

/// セッションに属する未処理キャプチャのうち、image_path が存在するものを返す
pub fn get_captures_for_session_assembly(
    conn: &Connection,
) -> Result<Vec<crate::models::CaptureRecord>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, app, window_title, image_path, description, dhash
         FROM captures
         WHERE session_id IS NULL
           AND image_path IS NOT NULL
         ORDER BY timestamp ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(crate::models::CaptureRecord {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            app: row.get(2)?,
            window_title: row.get(3)?,
            image_path: row.get(4)?,
            description: row.get(5)?,
            dhash: row.get(6)?,
        })
    })?;
    let mut captures = Vec::new();
    for row in rows {
        captures.push(row?);
    }
    Ok(captures)
}
```

---

## T14: `src-tauri/src/session.rs` — 新規モジュール

### 新規作成ファイル

`src-tauri/src/session.rs`

### 依存クレート確認

`src-tauri/Cargo.toml` に以下が存在することを確認し、なければ追加すること:

- `image = "0.25"` (既存の場合はバージョンを確認)
- `chrono = { version = "0.4", features = ["serde"] }` (既存の場合はスキップ)
- `uuid = { version = "1", features = ["v4"] }` (既存の場合はスキップ)

### モジュール登録

`src-tauri/src/main.rs` または `src-tauri/src/lib.rs` に `mod session;` を追加すること。

### 実装内容

以下の構造で `session.rs` を実装すること:

```rust
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgba};
use uuid::Uuid;

use crate::{
    db::{
        assign_session_to_captures, get_captures_for_session_assembly, insert_session,
        set_session_collage_path, DbError, SessionRecord,
    },
    models::{AppConfig, CaptureRecord},
};
```

#### 公開関数: `process_pending_sessions`

```rust
/// session_id が未割り当てのキャプチャをセッションにグループ化し、
/// コラージュを生成してDBに保存する。
/// 返り値: 生成されたセッションのリスト
pub fn process_pending_sessions(
    conn: &rusqlite::Connection,
    config: &AppConfig,
) -> Result<Vec<SessionRecord>, SessionError>
```

処理手順:

1. `get_captures_for_session_assembly(conn)` でセッション未割り当てキャプチャを取得
2. `assemble_sessions(captures, config)` でセッションにグループ化
3. 各セッションに対して:
   a. `select_key_frames(&session_captures, config.max_frames_per_collage)` でキーフレーム選択
   b. `build_collage(&key_frames, &data_dir)` でコラージュ画像生成
   c. `insert_session(conn, &record)` でセッションをDBに挿入
   d. `assign_session_to_captures(conn, &capture_ids, &session_id)` でキャプチャにsession_idを設定
4. 生成されたセッションのリストを返す

**重要**: セッション境界の判定で「最後のセッションの end_time から `session_gap_secs` 秒以内の場合はまだ継続中と見なし、スキップする」こと。これにより録画中のセッションを中途半端に確定しない。

#### 内部関数: `assemble_sessions`

```rust
fn assemble_sessions(
    captures: Vec<CaptureRecord>,
    config: &AppConfig,
) -> Vec<Vec<CaptureRecord>>
```

アルゴリズム:
1. captures は `timestamp` でソート済みと仮定
2. キャプチャを順番に処理し、以下の条件でセッションを区切る:
   - 前のキャプチャからの時間差 > `session_gap_secs` 秒
   - または、現在のセッション開始から > `session_window_secs` 秒
3. どちらか短い方の条件が先に満たされたらセッションを区切る
4. **現在時刻から `session_gap_secs` 秒以内に終わるセッション（最後のセッション）は除外する**
   - これは録画中の未完了セッションを除くため

timestamp のパース: `DateTime::parse_from_rfc3339(&capture.timestamp)`。パース失敗のキャプチャはスキップ。

#### 内部関数: `select_key_frames`

```rust
fn select_key_frames(
    captures: &[CaptureRecord],
    max_frames: u32,
) -> Vec<&CaptureRecord>
```

アルゴリズム:
1. dHash でほぼ同じフレームを除去（閾値: 5、通常の半分）
   - `captures[i].dhash` と `captures[i+1].dhash` を比較
   - dHash 文字列を16進数でパース → `u64` → `hamming_distance = (a ^ b).count_ones()`
   - distance ≤ 5 なら後のフレームをスキップ
2. 残ったフレームから均等に `max_frames` 枚をサンプリング
   - 残りフレーム数 ≤ max_frames なら全部返す
   - それ以上なら `(0..max_frames).map(|i| i * n / max_frames)` のインデックスで選択

#### 内部関数: `build_collage`

```rust
fn build_collage(
    key_frames: &[&CaptureRecord],
    data_dir: &Path,
) -> Result<PathBuf, SessionError>
```

アルゴリズム:

1. フレーム数に応じたグリッドレイアウトを決定:
   - 1枚: 1×1
   - 2枚: 1×2（横並び）
   - 3〜4枚: 2×2
   - 5〜6枚: 2×3

2. 各セルのサイズ: 全体 1280×960 をグリッドで割った値
   - cols=1: cell_w=1280 / cols=2: cell_w=640
   - rows=1: cell_h=960 / rows=2: cell_h=480 / rows=3: cell_h=320

3. 各フレームの画像を読み込み、セルサイズに Lanczos3 でリサイズ
   - `image::open(path)?.resize(cell_w, cell_h, image::imageops::FilterType::Lanczos3)`
   - 読み込み失敗のフレームは空白セル（黒）で埋める

4. コラージュ画像（RGBA8）を生成し、各セルに貼り付け

5. 各セルの右下に時刻ラベルを描画:
   - `capture.timestamp` から `HH:MM` 形式で抽出
   - フォントレンダリングは `imageproc` クレートが使えるが、重い場合は以下の代替案:
     **代替案（推奨）**: 時刻テキストをピクセル単位で描くのは複雑なので、
     代わりに `image` クレートの draw_text は使わず、
     セル右下に小さい半透明の黒背景矩形を描いて、その上に白ピクセルで
     HH:MM の各文字を3×5ビットマップフォントで描く。
     **ただし、時刻ラベル描画が複雑になりすぎるなら省略して構わない。**
     コラージュ生成の主目的はフレームの結合であり、ラベルは補助的なもの。

6. コラージュを PNG として保存:
   - 保存先: `data_dir/sessions/collage_{session_id}.png`
   - ディレクトリが存在しない場合は `fs::create_dir_all` で作成

#### エラー型

```rust
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("image processing error: {0}")]
    Image(#[from] image::ImageError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## T15: `vlm/batch.rs` — セッションバッチループの追加

### 対象ファイル

`src-tauri/src/vlm/batch.rs`

### 変更内容

#### 1. `vlm_batch_loop` の分岐を追加

既存の `vlm_batch_loop` 関数の冒頭（`ensure_engine_running` の直後、`get_unprocessed` の前）に分岐を追加する:

```rust
// セッションモードの場合は別ループへ
let config = state.config.lock().await.clone();
if options.vlm_engine == "copilot" && config.session_enabled {
    run_session_batch_loop(app, state, cancel_rx, pause_rx, options, config).await;
    return;
}
```

#### 2. `run_session_batch_loop` 関数を追加

```rust
async fn run_session_batch_loop(
    app: AppHandle,
    state: AppState,
    mut cancel_rx: watch::Receiver<bool>,
    mut pause_rx: watch::Receiver<bool>,
    options: BatchOptions,
    config: crate::models::AppConfig,
) {
    use crate::session::process_pending_sessions;

    // Step 1: セッションをアセンブル
    let sessions = {
        let db = state.db.lock().await;
        match process_pending_sessions(&db, &config) {
            Ok(s) => s,
            Err(error) => {
                finish_batch(
                    &app,
                    &state,
                    BatchResult { total: 0, completed: 0, failed: 0, cancelled: false, error: None },
                    Some(error.to_string()),
                    &options,
                )
                .await;
                return;
            }
        }
    };

    // 未処理セッション（process_pending_sessions が返すのは新規セッションのみ）と
    // 既存の未処理セッションをDBから取得してマージ
    let unprocessed = {
        let db = state.db.lock().await;
        match crate::db::get_unprocessed_sessions(&db) {
            Ok(s) => s,
            Err(error) => {
                finish_batch(
                    &app,
                    &state,
                    BatchResult { total: 0, completed: 0, failed: 0, cancelled: false, error: None },
                    Some(error.to_string()),
                    &options,
                )
                .await;
                return;
            }
        }
    };

    let total = unprocessed.len();
    emit_progress(&app, &state, VlmBatchProgress {
        total,
        completed: 0,
        failed: 0,
        current_id: None,
        estimated_remaining_secs: if total == 0 { Some(0) } else { None },
    })
    .await;

    if total == 0 {
        finish_batch(
            &app,
            &state,
            BatchResult { total: 0, completed: 0, failed: 0, cancelled: false, error: None },
            None,
            &options,
        )
        .await;
        return;
    }

    let client = reqwest::Client::new();
    let server_url = state.copilot_server.lock().await.server_url();
    let mut completed = 0;
    let mut failed = 0;
    let mut elapsed_times = Vec::new();

    for (index, session) in unprocessed.iter().enumerate() {
        if should_stop(&cancel_rx) {
            break;
        }
        if let Err(e) = wait_if_paused(&mut cancel_rx, &mut pause_rx).await {
            finish_batch(
                &app,
                &state,
                BatchResult { total, completed, failed, cancelled: true, error: None },
                Some(e.to_string()),
                &options,
            )
            .await;
            return;
        }

        emit_progress(&app, &state, VlmBatchProgress {
            total,
            completed,
            failed,
            current_id: Some(session.id.clone()),
            estimated_remaining_secs: estimate_remaining_secs(&elapsed_times, total, completed),
        })
        .await;

        let collage_path = match session.collage_path.as_deref() {
            Some(p) => p.to_string(),
            None => {
                failed += 1;
                continue;
            }
        };

        // セッション用プロンプトのプレースホルダを置換
        let start_dt = &session.start_time;
        let end_dt = &session.end_time;
        let duration_min = calc_duration_min(start_dt, end_dt);
        let frame_count = session.frame_count;
        let user_prompt = config.session_user_prompt
            .replace("{start_time}", &format_time_hhmm(start_dt))
            .replace("{end_time}", &format_time_hhmm(end_dt))
            .replace("{duration_min}", &duration_min.to_string())
            .replace("{frame_count}", &frame_count.to_string());

        let started_at = std::time::Instant::now();
        let result = describe_screenshot(
            &client,
            std::path::Path::new(&collage_path),
            &server_url,
            config.vlm_max_tokens,
            PromptContext {
                app: None,
                window_title: None,
                system_prompt: Some(&config.system_prompt),
                user_prompt: Some(&user_prompt),
            },
        )
        .await;

        match result {
            Ok(description) => {
                let db = state.db.lock().await;
                // セッションに説明を保存
                let _ = crate::db::mark_session_processed(&db, &session.id, &description);
                // セッション内の全キャプチャに同じ説明を設定し、処理済みにする
                let capture_ids_result = get_capture_ids_for_session(&db, &session.id);
                if let Ok(ids) = capture_ids_result {
                    for cid in &ids {
                        let _ = crate::db::update_description(&db, cid, &description);
                        let _ = crate::db::mark_processed(&db, cid);
                    }
                }
                // auto_delete_images の場合はコラージュ以外の元画像を削除
                if config.auto_delete_images {
                    if let Ok(ids) = get_capture_ids_for_session(&db, &session.id) {
                        for cid in &ids {
                            // 個別キャプチャ画像を削除
                            if let Ok(Some(img_path)) = get_capture_image_path(&db, cid) {
                                let _ = std::fs::remove_file(&img_path);
                                let _ = crate::db::clear_image_path(&db, cid);
                            }
                        }
                    }
                }
                elapsed_times.push(started_at.elapsed());
                completed += 1;
            }
            Err(e) => {
                tracing::warn!("session {} description failed: {}", session.id, e);
                failed += 1;
            }
        }
    }

    finish_batch(
        &app,
        &state,
        BatchResult { total, completed, failed, cancelled: false, error: None },
        None,
        &options,
    )
    .await;
}
```

#### 3. ヘルパー関数を追加

```rust
fn get_capture_ids_for_session(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id FROM captures WHERE session_id = ?1 ORDER BY timestamp ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![session_id], |row| row.get(0))?;
    rows.collect()
}

fn get_capture_image_path(
    conn: &rusqlite::Connection,
    capture_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT image_path FROM captures WHERE id = ?1",
        rusqlite::params![capture_id],
        |row| row.get(0),
    )
}

fn calc_duration_min(start: &str, end: &str) -> i64 {
    use chrono::DateTime;
    let Ok(s) = DateTime::parse_from_rfc3339(start) else { return 0 };
    let Ok(e) = DateTime::parse_from_rfc3339(end) else { return 0 };
    (e - s).num_minutes().max(0)
}

fn format_time_hhmm(ts: &str) -> String {
    use chrono::DateTime;
    let Ok(dt) = DateTime::parse_from_rfc3339(ts) else { return ts.to_string() };
    dt.format("%H:%M").to_string()
}
```

---

## T16: `settings/+page.svelte` — セッション設定 UI の追加

### 対象ファイル

`src/routes/settings/+page.svelte`

### 変更内容

#### 1. `AppConfig` 型にフィールドを追加

```typescript
type AppConfig = {
  // ... 既存フィールド ...
  session_enabled: boolean;
  session_gap_secs: number;
  session_window_secs: number;
  max_frames_per_collage: number;
  session_user_prompt: string;
};
```

#### 2. `defaultConfig` にデフォルト値を追加

```typescript
const defaultConfig: AppConfig = {
  // ... 既存フィールド ...
  session_enabled: true,
  session_gap_secs: 600,
  session_window_secs: 300,
  max_frames_per_collage: 6,
  session_user_prompt:
    "これは {start_time} から {end_time} の間（{duration_min}分間）の業務画面の流れです。{frame_count} 枚のスクリーンショットを時系列順に並べたコラージュを見て、この間に行っていた業務操作を1〜3文で説明してください。必ず次の観点を含めてください: 使用中のアプリケーション、実行している操作の流れ、表示されているデータや対象。出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。"
};
```

#### 3. キャプチャ間隔スライダーの最小値を変更

既存の以下の行:
```html
min="10"
```
を以下に変更:
```html
min="3"
```

#### 4. copilot エンジン選択時のセッション設定セクションを追加

copilot エンジンの設定セクション（Edge CDP ポートの下）に以下のセクションを追加する:

```html
{#if config.vlm_engine === "copilot"}
  <!-- セッション処理 -->
  <div class="space-y-4 rounded-lg border border-gray-200 p-4">
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-semibold text-gray-900">セッション処理</p>
        <p class="text-xs text-gray-500 mt-0.5">
          複数フレームを結合して Copilot に送信（レートリミット対策）
        </p>
      </div>
      <input
        type="checkbox"
        bind:checked={config.session_enabled}
        class="h-4 w-4 rounded border-gray-300 text-brass-600"
      />
    </div>

    {#if config.session_enabled}
      <div class="space-y-3 border-t border-gray-100 pt-3">
        <!-- セッションギャップ -->
        <div>
          <div class="flex justify-between mb-1">
            <label class="text-xs font-medium text-gray-700">
              セッション区切り（無操作）
            </label>
            <span class="text-xs text-gray-500">{Math.round(config.session_gap_secs / 60)} 分</span>
          </div>
          <input
            type="range"
            min="120"
            max="1800"
            step="60"
            bind:value={config.session_gap_secs}
            class="w-full"
          />
          <div class="flex justify-between text-xs text-gray-400 mt-0.5">
            <span>2分</span><span>30分</span>
          </div>
        </div>

        <!-- セッション最大時間窓 -->
        <div>
          <div class="flex justify-between mb-1">
            <label class="text-xs font-medium text-gray-700">
              セッション最大長
            </label>
            <span class="text-xs text-gray-500">{Math.round(config.session_window_secs / 60)} 分</span>
          </div>
          <input
            type="range"
            min="60"
            max="900"
            step="60"
            bind:value={config.session_window_secs}
            class="w-full"
          />
          <div class="flex justify-between text-xs text-gray-400 mt-0.5">
            <span>1分</span><span>15分</span>
          </div>
        </div>

        <!-- コラージュ最大フレーム数 -->
        <div>
          <div class="flex justify-between mb-1">
            <label class="text-xs font-medium text-gray-700">
              コラージュ最大フレーム数
            </label>
            <span class="text-xs text-gray-500">{config.max_frames_per_collage} 枚</span>
          </div>
          <input
            type="range"
            min="2"
            max="6"
            step="1"
            bind:value={config.max_frames_per_collage}
            class="w-full"
          />
          <div class="flex justify-between text-xs text-gray-400 mt-0.5">
            <span>2</span><span>6</span>
          </div>
        </div>

        <!-- セッションプロンプト -->
        <div>
          <label class="text-xs font-medium text-gray-700 block mb-1">
            セッション用プロンプト
          </label>
          <textarea
            bind:value={config.session_user_prompt}
            rows="4"
            class="w-full rounded border border-gray-300 px-2 py-1.5 text-xs font-mono focus:outline-none focus:ring-1 focus:ring-brass-400"
          ></textarea>
          <p class="text-xs text-gray-400 mt-0.5">
            プレースホルダ: {"{start_time}"} {"{end_time}"} {"{duration_min}"} {"{frame_count}"}
          </p>
        </div>
      </div>
    {/if}
  </div>
{/if}
```

---

## 完了条件チェックリスト

- [ ] `cargo check` がエラーなしで通過する
- [ ] `pnpm build:copilot` が成功する（TypeScript 変更がない場合はスキップ可）
- [ ] `DB_SCHEMA_VERSION` が `3` になっている
- [ ] `sessions` テーブルと `captures.session_id` 列が v3 マイグレーションで追加される
- [ ] `session.rs` が `process_pending_sessions` を公開している
- [ ] copilot + `session_enabled: true` 時に `run_session_batch_loop` が呼ばれる
- [ ] llama エンジンのコードパスに変更がない
- [ ] キャプチャ間隔スライダーの最小値が `3` になっている
- [ ] セッション設定 UI が copilot エンジン選択時のみ表示される

---

## 注意事項

- `image` クレートのバージョンが 0.24 系の場合はAPIが異なる可能性がある。
  `Cargo.toml` で確認し、`resize` の引数型（`u32` vs `NonZeroU32`）を合わせること。
- `imageproc` クレートを追加しない場合、時刻ラベル描画は省略して構わない。
  コラージュ生成が主目的のため。
- Windows の path separator (`\`) を考慮し、path 操作は `std::path::Path` / `PathBuf` を使うこと。
- `chrono` が `Cargo.toml` にない場合は `time` クレート（既存）で代替してもよい。
  `time::OffsetDateTime::parse(&ts, &time::format_description::well_known::Rfc3339)` を使う。
