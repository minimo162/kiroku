# Kiroku 再設計 — 技術設計書 (design.md)

作成日: 2026-04-17
対象: `minimo162/kiroku`
状態: requirements.md (2026-04-17 改訂版) を元にした実装設計

---

## 0. この文書の位置付け

本書は `requirements.md` で確定した要件・制約・ゴールを、
実装可能な粒度のモジュール・型・関数・フロー・スレッドモデルに落とし込む。

- **上位層**: requirements.md (What / Why)
- **本書**: design.md (How — 型・モジュール・境界・同期モデル)
- **下位層**: 実ソースコード (Rust / Svelte)

実装途中で設計に矛盾や穴が出た場合は、
まず本書を更新し、合意した上で実装に反映する。

---

## 1. 設計原則

本設計を通して守るべき 5 原則。

1. **1 セッション = 1 オブジェクト**
   `RecordingSession` だけがセッション状態を持ち、サービスはセッションに従属する。
2. **長寿命サービスはセッションで再生成しない**
   `rdev::listen` / UIA STA スレッドは、アプリ生存中 1 本だけ走る。
3. **I/O は writer に閉じ込める**
   NDJSON append・counts・UIA 再結合は 1 スレッド・1 関数で完結させる。
4. **自動送信しない / 推測しない**
   bundle は「Copilot に渡す材料」であり、Copilot の出力を前提にしない。
5. **失敗しても raw は残る**
   bundle 生成に失敗しても、raw session (events.ndjson + screens/) は保全され、後から再 bundle 可能。

---

## 2. 全体アーキテクチャ

```text
                 ┌─────────────────────────┐
                 │   Svelte (Dashboard)    │
                 └────────────┬────────────┘
              invoke/emit    │
                 ┌────────────▼────────────┐
                 │ Tauri command layer     │
                 │  start_recording        │
                 │  stop_recording         │
                 │  get_recording_state    │
                 │  list_bundles / ...     │
                 └────────────┬────────────┘
                              │
                 ┌────────────▼────────────┐
                 │  RecordingController    │   ← AppState が保持
                 │  (state machine)        │
                 └───┬──────────────┬──────┘
                     │              │
      RecordingSession              │ (idle 時は None)
                     │              │
     ┌───────────────┼──────────────┴────────────┬──────────────┐
     │               │                           │              │
     ▼               ▼                           ▼              ▼
InputListener  ScreenSampler                 UiaService     Writer
(rdev)         (xcap + heartbeat)            (STA thread)   (tokio task)
     │               │                           │              │
     └───────────────┴───────────────────────────┴──────────────┘
                              │
                  events.ndjson + screens/*.png
                              │
                              ▼
                   ┌────────────────────┐
                   │  Bundle builder     │
                   │ normalize/keyframe/ │
                   │ annotate/prompt     │
                   └─────────┬──────────┘
                             │
                  explorer open + clipboard
```

- **InputListener / UiaService** はプロセス常駐 (長寿命サービス)。
- **ScreenSampler / Writer** は `RecordingSession` の生存に連動する短寿命サービス。
- すべてのサービスは `crossbeam-channel` による mpsc/spsc で接続する。

---

## 3. モジュール構成

requirements.md §7 に従いつつ、既存コードからの移植パスを明示する。

```text
src-tauri/src/
  lib.rs                         # 大改造: recording/bundle 配線のみ残す
  main.rs                        # 変更なし
  state.rs                       # AppState を縮小、recording/bundle 用フィールドを追加
  config.rs                      # 設定 source of truth、プロンプト周りを削減
  models.rs                      # AppConfig の項目を一新
  db.rs                          # bundles テーブル追加、legacy は read-only 保持
  tray.rs                        # Recording/Bundling/Idle 表示に対応
  capture.rs                     # 下位プリミティブ (capture_primary_monitor) を流用

  recording/
    mod.rs                       # pub use 再エクスポート、公開 API 集約
    service.rs                   # RecordingController + Tauri command 実装
    session.rs                   # RecordingSession ライフサイクル
    event.rs                     # Event / EventKind / serde
    input_listener.rs            # rdev listener (プロセス常駐)
    screen_sampler.rs            # heartbeat + event-trigger capture
    uia_worker.rs                # STA thread + UIA query
    writer.rs                    # NDJSON append / counts / UIA 再結合
    window_focus.rs              # FocusChange 監視 (既存 window_meta 流用)

  bundle/
    mod.rs
    normalize.rs                 # TextRun 化・ノイズ削減・digest 整形
    keyframe.rs                  # キーフレーム選定
    annotate.rs                  # 画像注釈 (赤丸 + ラベル + caption)
    prompt.rs                    # prompt.md 生成
    writer.rs                    # manifest.json / ファイル配置

  bin/
    kiroku_input_hook.rs         # helper process fallback (Phase 0 で必要と判断された場合のみ)

  # 削除候補 (Phase 1 でコードごと削除)
  scheduler.rs                   # 削除
  session.rs                     # 削除
  vlm/batch.rs                   # 自動ループを削除 (ファイル自体は縮小)

src/
  routes/
    dashboard/+page.svelte       # RecordingState ベースに再実装
    history/+page.svelte         # Bundle / Legacy の 2 タブ化
    settings/+page.svelte        # 項目を削減 + source of truth を backend に
  lib/
    types/recording.ts           # 新規: RecordingState / Bundle payload 型
    types/dashboard.ts           # 縮小 (VLM 系を削除)
    components/dashboard/
      RecordingStatusCard.svelte # 新規: 現在状態 + 警告
      BundleReadyCard.svelte     # 新規: Stop 後の案内
```

### 3.1 既存 → 新規への移植表

| 既存 | 移植先 | 備考 |
|---|---|---|
| `recorder.rs` の `start_recording` / `stop_recording` | `recording/service.rs` | Tauri コマンド名は維持 |
| `capture::capture_primary_monitor` | そのまま流用 | `screen_sampler.rs` から呼ぶ |
| `window_meta::get_active_window_metadata` | `recording/window_focus.rs` から利用 | UIA 取得失敗時のフォールバック |
| `tray::update_recording_tray_state` | `service.rs` が state 変更時に通知 | 引数を `RecordingState` に拡張 |
| `scheduler.rs` / 旧 `session.rs` / `vlm/batch.rs` 自動ループ | 削除 | Phase 1 のクリーンアップ対象 |

---

## 4. 状態モデル (RecordingState)

requirements.md §8 を起点に、Tauri / Svelte 間でやりとりする payload と
内部オブジェクトを分離する。

### 4.1 内部状態 (Rust)

```rust
// recording/service.rs
pub struct RecordingController {
    state: Mutex<RecordingState>,
    session: Mutex<Option<Arc<RecordingSession>>>,
    input_listener: Arc<InputListenerService>,   // 長寿命
    uia_service:    Arc<UiaService>,              // 長寿命 (lazy init)
    app_handle:     AppHandle,
    paths:          Arc<AppPaths>,
    db:             Arc<Mutex<rusqlite::Connection>>,
    config_tx:      Arc<watch::Sender<AppConfig>>,
}

pub enum RecordingState {
    Idle,
    Starting { session_id: SessionId },
    Recording {
        session_id: SessionId,
        started_at: DateTime<Local>,
        started_mono: Instant,
        stats:      SessionStats,
        warning:    Option<String>,
    },
    Stopping { session_id: SessionId, reason: StopReason },
    Bundling { session_id: SessionId, progress: BundleProgress },
    Failed   { session_id: Option<SessionId>, message: String },
}

pub struct SessionStats {
    pub event_count:      u64,
    pub screenshot_count: u64,
    pub text_input_count: u64,
}

pub struct BundleProgress {
    pub phase: BundlePhase,           // Normalizing / SelectingKeyframes / Annotating / Writing
    pub current: u32,
    pub total:   u32,
}
```

`SessionId` は `Arc<String>` (UUID v4)。生成は `Starting` 遷移時に 1 回だけ。

### 4.2 外部 payload (Tauri → Svelte)

```rust
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordingStatePayload {
    Idle,
    Starting  { session_id: String },
    Recording {
        session_id: String,
        started_at: String,         // RFC3339
        elapsed_ms: u64,
        event_count: u64,
        screenshot_count: u64,
        text_input_count: u64,
        warning: Option<String>,
    },
    Stopping  { session_id: String, reason: String },
    Bundling  { session_id: String, phase: String, current: u32, total: u32 },
    Failed    { session_id: Option<String>, message: String },
}
```

- `RecordingState` → `RecordingStatePayload` の変換は `service.rs` 内の単一関数で行う。
- `elapsed_ms` は `Instant::elapsed()` を payload 生成時に毎回計算して付ける (状態には保持しない)。

### 4.3 遷移

```text
Idle ──start_recording──> Starting ──(prepare ok)──> Recording
                              │
                              └─(prepare err)─> Failed ──reset──> Idle

Recording ──stop_recording / max_duration──> Stopping
Stopping  ──drain ok──> Bundling ──build ok──> Idle (bundle-ready 発火)
Bundling  ──build err──> Failed ──reset──> Idle (bundle-failed 発火)
```

- 遷移は `RecordingController::transition()` を一本化した async 関数にまとめ、
  Mutex を保持したままの遷移を禁止する (I/O と状態更新を分離)。
- 各遷移で `recording-state` イベントを emit する。
- `recording-status: bool` は移行期間のみ emit (`Idle` 以外を `true` と見なす)。

---

## 5. イベントとタイムラインデータモデル

### 5.1 型定義 (recording/event.rs)

requirements.md §10 を Rust コードに起こす。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq:       u64,                 // 0 から単調増加
    pub t_mono_ms: u64,                 // session 開始からの monotonic ms
    pub wallclock: DateTime<Local>,     // RFC3339
    #[serde(flatten)]
    pub kind:      EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    SessionStarted,
    SessionStopped { reason: StopReason },

    FocusChange { hwnd: Option<i64>, title: String, process: String },

    MouseDown { button: MouseButton, x: i32, y: i32 },
    MouseUp   { button: MouseButton, x: i32, y: i32 },
    MouseMove { x: i32, y: i32 },
    Wheel     { dx: i64, dy: i64 },

    PhysicalKeyDown { key: String, shift: bool, ctrl: bool, alt: bool, win: bool },
    PhysicalKeyUp   { key: String },

    TextInput { text: String, masked: bool },

    Screenshot {
        path:       String,                // session 相対 (例 "screens/000042.png")
        dhash:      Option<String>,
        width:      u32,
        height:     u32,
        cause_seq:  Option<u64>,           // 発火源イベントの seq
        redactions: Vec<Rect>,
    },

    UiaContext {
        for_seq:       u64,
        role:          Option<String>,
        name:          Option<String>,
        value:         Option<String>,     // password field は書かない
        cell_address:  Option<String>,
        bounding_rect: Option<Rect>,
        is_password:   bool,
    },

    Warning { code: WarningCode, message: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason { UserRequested, MaxDurationReached, AppShutdown, FatalError }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton { Left, Right, Middle, Other }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    OffPrimaryMonitor,
    UiaUnavailable,
    UiaTimeout,
    ImeCompositionBestEffort,
    ScreenshotFailed,
    MaxDurationReached,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }
```

### 5.2 NDJSON フォーマット

1 行 = 1 `Event` (JSON)。改行は LF。
writer は `BufWriter<File>` + `write_all` + 明示的 `flush` で append する。

- 改行区切りなので、中断後も行単位のリカバリが可能。
- parser 側は `serde_json::from_str` で 1 行ずつ読む。

### 5.3 seq の採番

- `AtomicU64` を `RecordingSession` が保持する。
- 採番は入力 callback / sampler / uia 返答を問わず、writer 入口で `fetch_add(1, Relaxed)` する。
- 採番と NDJSON 書き込みは **writer スレッド内で逐次化** される。
  input listener callback は「未採番の EventDraft」を投げるだけ。

```rust
pub struct EventDraft {
    pub t_mono_ms: u64,            // listener callback 内で取得
    pub wallclock: DateTime<Local>,
    pub kind: EventKind,            // seq を持たない変種も許容
}
```

---

## 6. 長寿命サービス設計

### 6.1 InputListenerService (recording/input_listener.rs)

#### 6.1.1 責務

- アプリ起動直後に `spawn_blocking` で 1 本だけ `rdev::listen` を張る。
- `recording_active: AtomicBool` が `true` のときだけ、
  現在の session channel に EventDraft を送る。
- `recording_active=false` の間は callback 内で即 return する。

#### 6.1.2 構造

```rust
pub struct InputListenerService {
    recording_active: Arc<AtomicBool>,
    session_tx: Arc<ArcSwap<Option<SessionChannels>>>, // Option<Arc<Session>> 相当
    pressed: Arc<Mutex<HashSet<rdev::Key>>>,           // modifier 判定
}

pub struct SessionChannels {
    pub event_tx:  crossbeam_channel::Sender<EventDraft>,
    pub uia_tx:    crossbeam_channel::Sender<UiaRequest>,
    pub capture_tx:crossbeam_channel::Sender<CaptureRequest>,
}
```

- `ArcSwap<Option<SessionChannels>>` により、callback 側は lock-free に現在の session を参照する。
- セッション開始時は `store(Some(channels))`、終了時は `store(None)` → callback が自然に no-op になる。

#### 6.1.3 callback 内の禁止事項

- **I/O は一切しない**: ファイル・DB・println は禁止。
- **allocate を最小化**: `EventDraft` は stack に近い size。ただし `TextInput.text` は `String` で alloc は許容。
- **lock を取らない** ことを原則とする。modifier flag は `AtomicU8` bitfield で保持。

#### 6.1.4 イベント分類

| rdev event | 変換先 EventKind | 備考 |
|---|---|---|
| `KeyPress`(ASCII/printable) | `PhysicalKeyDown` + `TextInput` (masked 判定付き) | IME は後述 |
| `KeyPress`(special: Enter/Tab/Esc/Arrow/Alt/...) | `PhysicalKeyDown` のみ | caption には出ない |
| `KeyRelease` | `PhysicalKeyUp` | |
| `ButtonPress/Release` | `MouseDown/Up` | UIA / capture トリガー |
| `MouseMove` | `MouseMove` | 50Hz 絞りを writer 側で実施 |
| `Wheel` | `Wheel` | |

#### 6.1.5 IME の扱い

v1 は「printable キー押下時に `TextInput { text }` を同時発火する」
最低版のみ実装する。

- `ctrl/alt/win` の組合せは TextInput を出さない。
- Shift のみなら大文字化して `TextInput` を出す。
- 日本語 IME の変換中文字列は取れないため、`ImeCompositionBestEffort` を
  **セッション開始時に 1 度だけ** `Warning` として記録する。
- 確定文字列の取得は v1 では試みない (requirements §6.2)。
- password field focus 中は `TextInput.text = "•"` * (len)、`masked = true`。
  ただし writer が UIA の `is_password` を確認してから確定するため、
  詳細は §6.3 参照。

#### 6.1.6 rdev の Windows 制約

- `rdev::listen` は blocking。必ず `spawn_blocking` で別スレッド。
- Windows では低レベルフック (`WH_KEYBOARD_LL`) を使うため、
  メッセージループが別途必要。rdev 内部で処理される前提で追加実装はしない。
- **Phase 0 で実測**: Tauri 同一プロセスでの安定性を確認する。
  欠落が出る PC では `src-tauri/src/bin/kiroku_input_hook.rs` にフォールバック。
  helper 採用時の IPC は stdin に NDJSON を流す最小構成 (stdio pipe + line-delimited JSON)。

### 6.2 UiaService (recording/uia_worker.rs)

#### 6.2.1 スレッドモデル

- アプリ起動時には起動しない。**初回の `Starting` で lazy 起動**。
- 専用 OS スレッド (`std::thread::spawn`) で STA を構成:
  - `CoInitializeEx(COINIT_APARTMENTTHREADED)`
  - `UIAutomation::new()`
- `Receiver<UiaRequest>` から要求を受け取り、`Sender<UiaResponse>` で返す。
- このスレッドは **アプリ生存中 1 本**。idle 中は `recv` でブロック。
- アプリ終了時に `channel drop` で抜ける。

#### 6.2.2 リクエスト / レスポンス

```rust
pub struct UiaRequest {
    pub for_seq: u64,           // 紐付け先イベント
    pub trigger: UiaTrigger,    // FocusChange / MouseUp / Enter / Tab
    pub cursor:  Option<(i32, i32)>,
}

pub struct UiaResponse {
    pub for_seq: u64,
    pub payload: UiaContextPayload,   // requirements §9.3
    pub elapsed_ms: u32,
}
```

#### 6.2.3 タイムアウトとフォールバック

- 1 要求あたり 150ms hard timeout。
- timeout 時は `Warning { code: UiaTimeout }` を記録し、
  `window_focus` から取った `app / title` を `UiaContext` の `name/role` に代用。
- channel が詰まったら **古い要求を drop**。
  rationale: UIA 結果は bundle 正規化で「その周辺の時間帯」を補助するだけなので、
  1:1 必須ではない。

#### 6.2.4 password 判定の副作用

UIA 応答で `is_password = true` を得たら、UiaService は:

- `Arc<AtomicBool> password_focus` を `true` にする。
- `Arc<Mutex<Option<Rect>>> password_rect` を更新する。

フォーカスが抜けたら `focus change` が来るので、そこで `false` に戻す。
この 2 つは input_listener と screen_sampler が **読むだけ**。

### 6.3 WindowFocus 監視 (recording/window_focus.rs)

WinEvent `EVENT_SYSTEM_FOREGROUND` を使うのが理想だが、v1 は
**heartbeat ポーリング (500ms)** + `window_meta::get_active_window_metadata`
で済ます (コスト低・安定)。

- hwnd / title / process が変化したら `FocusChange` を writer に送る。
- 同時に UIA request (`FocusChange` trigger) を投げる。
- sampler には「次の 150ms 後に capture」のリクエストを送る。

---

## 7. セッション単位オブジェクト

### 7.1 RecordingSession (recording/session.rs)

```rust
pub struct RecordingSession {
    pub id:           SessionId,
    pub started_at:   DateTime<Local>,
    pub started_mono: Instant,
    pub paths:        SessionPaths,            // dir / events.ndjson / screens/
    pub stats:        Arc<Mutex<SessionStats>>,
    pub seq:          Arc<AtomicU64>,
    pub stop_reason:  Mutex<Option<StopReason>>,

    // 短寿命サービスへの送受信口
    event_tx:   Sender<EventDraft>,
    capture_tx: Sender<CaptureRequest>,
    uia_tx:     Sender<UiaRequest>,

    // join handles
    writer_task:  JoinHandle<WriterSummary>,
    sampler_task: JoinHandle<()>,
    focus_task:   JoinHandle<()>,
}
```

### 7.2 ライフサイクル

`start()`:
1. `SessionId` 生成、`SessionPaths` 確保 (`recordings/<id>/{events.ndjson, screens/}`)。
2. `session.json` を書き出す (config snapshot + primary monitor + app version)。
3. channel 群を構築 (event / capture / uia)。
4. writer task を起動 (tokio::task、but 書き込みは `spawn_blocking`)。
5. sampler task / focus task を起動。
6. UIA worker に session を紐付け (request channel を渡す)。
7. InputListener に `SessionChannels` を `ArcSwap::store(Some(...))`。
8. `recording_active = true`。
9. `Event { SessionStarted }` を emit。

`stop(reason)`:
1. `recording_active = false`。
2. InputListener の session を `ArcSwap::store(None)`。
3. capture channel / uia request channel を `drop` (送信側終了)。
4. sampler task / focus task の `join`。
5. `Event { SessionStopped { reason } }` を event_tx に送る。
6. event_tx を drop → writer task が EOF を検知して flush。
7. writer task の `join` で `WriterSummary` を受け取る。
8. final screenshot を 1 枚撮り、直接 writer に渡す (writer 終了前)。
   実装上は: final screenshot も通常 capture と同じ経路を通し、
   「final_capture_barrier」を insert して sampler 経由で取る方が race が少ない。
9. Bundle builder へ `SessionPaths` と `WriterSummary` を引き渡す。

### 7.3 SessionPaths とディレクトリ

```text
<data_dir>/
  recordings/
    <session_id>/
      session.json
      events.ndjson
      screens/
        000001.png
        000002.png
```

- `session.json` スキーマ:
  ```json
  {
    "schema_version": 1,
    "session_id": "uuid",
    "started_at": "RFC3339",
    "app_version": "0.2.0",
    "bundle_version": 1,
    "primary_monitor": { "width": 2560, "height": 1440, "scale": 1.5 },
    "config_snapshot": { "max_recording_minutes": 60, "record_keystrokes": true, ... }
  }
  ```
- screens のファイル名は `{seq:06}.png`。seq と一致させることで
  bundle 側が `cause_seq` だけで画像を特定できる。

---

## 8. ScreenSampler (recording/screen_sampler.rs)

### 8.1 責務

requirements §9.2 のルールを実装する:

1. **Heartbeat** 2s 周期。
2. **Event trigger**: `MouseUp / Enter / Tab / Esc / Alt+Tab / FocusChange` 後 150ms。
3. **Debounce**: 直近 400ms の capture は統合。
4. **Stop final**: 停止時 1 枚。

### 8.2 実装

```rust
pub enum CaptureRequest {
    Heartbeat,
    Triggered { cause_seq: u64, after_ms: u32 },
    Final,
}
```

- sampler は tokio task で走る。
- `tokio::select!` で:
  - heartbeat ticker (2s)
  - `capture_rx.recv()` (trigger)
- 400ms debounce は `Instant last_capture` を保持し、
  `now - last_capture < 400ms` なら skip。
- capture は `tokio::task::spawn_blocking` で `capture::capture_primary_monitor` を呼ぶ。
- 結果に対して:
  - password_rect が Some なら黒塗り (image crate で `draw_filled_rect`)。
  - dhash 計算。
  - writer に `EventDraft { Screenshot }` を送る。

### 8.3 primary monitor 監視

- `Monitor::all()` の結果で primary 以外が存在したら、
  session 開始時に `Warning { OffPrimaryMonitor }` を 1 回記録。
- マウスが primary の範囲外に出たら (50ms ポーリングで判定)、
  `Warning { OffPrimaryMonitor, message: "mouse left primary monitor" }`。
  同じ状態が続く間は 10s に 1 回の間引きで warn。

### 8.4 性能目標

- heartbeat (2s) + trigger 合算で、1 分あたり最大 ~60 枚、
  30 分で最大 ~1800 枚。bundle では §12 のキーフレーム選定で最大 60 枚に落とす。
- capture 1 回の所要時間は capture.rs で既に 500ms を閾値にしている。
- 画像は PNG 無圧縮寄り (xcap のデフォルト) でまず書く。
  bundle で使わない画像の削減は **v1 では実施しない** (raw 保全を優先)。

---

## 9. Writer (recording/writer.rs)

### 9.1 責務

- seq の最終採番
- UIA 応答の for_seq 突合せ と NDJSON 再結合
- `events.ndjson` への append (行単位)
- `SessionStats` の更新
- shutdown 時の flush
- `WriterSummary` の返却

### 9.2 チャネル構成

```text
InputListener ──event_tx──┐
ScreenSampler ──event_tx──┤
WindowFocus    ──event_tx──┤──> writer task ──append──> events.ndjson
UiaService    ──uia_rx────┘
```

- UIA 応答だけは別チャネル。writer 内で `(for_seq → EventDraft)` マップに解決する。
- UIA 応答は到着が遅いので、writer は `UiaContext` を専用の
  **順序維持バッファ** に入れ、それ以外のイベントとマージして書き出す。

### 9.3 書き込みポリシー

- **逐次書き**: 各 EventDraft を受け取ったら即採番 → NDJSON 1 行追記 → flush。
  - ただし `flush` はバッチ化: 50ms coalesce window または 256 イベント単位。
- **MouseMove 絞り**: writer 入口で 50Hz (20ms) に絞る。
  - 直近 MouseMove の wallclock から 20ms 未満なら **破棄**。
  - 200ms idle 後の最初の move は必ず書く (idle timer を別途保持)。
- **UIA マージ**: UIA 応答を受信したら即別行として書く。
  bundle 側で `for_seq` を元に紐付ける。
  → writer では "遅れて来たが時系列順に追加される" ことだけ許容すればよい。

### 9.4 耐障害性

- write エラーは `WriterSummary.errors` に積むが writer task は死なない。
- OOM / disk full は `Event::Warning { ScreenshotFailed }` 等で残す。
- アプリクラッシュ時は最後の flush までが保全される。
  events.ndjson は append-only なので、行の途中で切れても
  parser 側で「最後の不完全行を捨てる」で復旧可能。

---

## 10. Bundle 生成 (bundle/*.rs)

### 10.1 パイプライン

```text
RecordingSession stop
     │
     ▼
bundle/mod.rs: build_bundle(session_paths, config) -> BundleResult
     │
     ├─ normalize.rs::normalize()  # TextRun 化、MouseMove 間引き、step 候補
     ├─ keyframe.rs::select()      # events + dhash から 最大60 枚
     ├─ annotate.rs::render()      # PNG に赤丸 + caption + ラベル (password 先黒塗り)
     ├─ prompt.rs::generate()      # prompt.md 生成
     └─ writer.rs::finalize()      # manifest.json, events.ndjson コピー, 配置
```

### 10.2 normalize.rs

1. events.ndjson を parse して `Vec<Event>` にロード。
2. 連続する `TextInput` を 1 つの `TextRun { text, start_seq, end_seq, masked }` に畳む。
   境界は `Enter / Tab / Escape / Alt+Tab / FocusChange / MouseUp` のいずれか。
3. MouseMove は間引き: annotate 用に cursor trail を残すだけで digest には出さない。
4. ステップ候補: `MouseDown/MouseUp + (UiaContext 同 for_seq) + 直後 Screenshot` を束ねる。
5. `Warning(OffPrimaryMonitor)` 区間は step に "画面保証外" フラグを付与する。

出力型:

```rust
pub struct NormalizedTimeline {
    pub steps:      Vec<Step>,
    pub text_runs:  Vec<TextRun>,
    pub warnings:   Vec<EventWarning>,
    pub raw_digest: Vec<DigestLine>,   // prompt.md の <event_digest> 用
}

pub struct Step {
    pub seq_range:    (u64, u64),
    pub anchor_seq:   u64,             // この step の中心 (MouseUp や Enter)
    pub screenshot:   Option<ScreenshotRef>,
    pub uia:          Option<UiaContextPayload>,
    pub app:          Option<String>,
    pub title:        Option<String>,
    pub caption:      Option<String>,  // "入力: \"売上\"" など
    pub off_primary:  bool,
}
```

### 10.3 keyframe.rs

採択条件 (requirements §12.3):

- `FocusChange` 直後
- `MouseUp` 直前 / 直後
- `TextRun` 完了直後
- `Enter / Tab / Escape / Alt+Tab` 直後
- `dhash` 差分が閾値超過 (既存 `diff::has_significant_change` を流用)
- 2s heartbeat 画像

選定アルゴリズム:

1. 候補スコアリング: step 境界に近い / dhash 差 / heartbeat の重み付け。
2. `frame_cap = min(60, ceil(duration_min * 2))` で枝刈り。
3. 削除時はステップ優先 + 均等サンプリングで保つ。

### 10.4 annotate.rs

1. 元画像を読み込み。
2. `redactions` があれば **先に** 黒塗り (image crate の `draw_filled_rect_mut`)。
3. ステップ番号 (右上テキスト) + 赤丸 (クリック座標) を描画。
4. UI ラベル (右上): `{process} / {cell_address or role+name}`。
5. 下部 caption: `TextRun` があれば `入力: "〜"`、`Enter/Tab/Esc` なら `キー: Enter`。
6. `off_primary` 区間には画像下端に赤ストライプ帯 + `primary monitor 外の操作`。
7. 出力は bundle フォルダへ `{step_index:02}_{label}.png`。

フォント: 埋め込み済みの日本語対応 TTF (例: Noto Sans JP Regular) を `include_bytes!` で同梱。
既存プロジェクトにフォントが無い場合は asset を `src-tauri/assets/fonts/` に追加。

### 10.5 prompt.rs

requirements §12.6 のテンプレートをベースに、`<event_digest>` だけを動的に差し込む:

```rust
pub fn generate_prompt_md(
    template: &str,
    timeline: &NormalizedTimeline,
) -> String
```

- テンプレートは `AppConfig.procedure_prompt_template` から取得 (source of truth は backend)。
- `<event_digest>` の行数は上限 200 (超過時は先頭 / 末尾を優先し中略 `...` を入れる)。
- digest 1 行のフォーマット:
  ```
  HH:MM:SS  focus -> <app> <title>
  HH:MM:SS  click -> cell A1
  HH:MM:SS  type  -> "売上"
  HH:MM:SS  key   -> Enter
  HH:MM:SS  warn  -> off_primary_monitor
  ```

### 10.6 writer.rs (bundle writer)

- bundle フォルダ: `<data_dir>/bundles/<yyyyMMdd_HHmmss>_<primary_app_slug>/`。
- 画像配置、`events.ndjson` コピー、`prompt.md`、`manifest.json` 出力。
- DB `bundles` の status を `bundling` → `ready` に更新。
- 失敗時は `status = failed` + `error_message` 記録。

### 10.7 BundleProgress の発行

各フェーズ開始時に `Bundling` state を更新し、`recording-state` event を emit。

---

## 11. ユーザー引き渡し (Stop 後)

### 11.1 BundleOpenService (recording/service.rs 内)

- 1 スレッドで扱う。`arboard` クリップボードはここ以外から触らない。
- 処理順:
  1. `open::that(bundle_dir)` で explorer。
  2. `clipboard.set_text(prompt_md_content)`。
  3. `bundle-ready { bundle_id, folder_path, frame_count }` を emit。
- エラーは `bundle-failed` で emit するが、raw session と bundle ファイルは残す。

### 11.2 フロント表示

- Stop 完了後、`BundleReadyCard` を dashboard に出す。
- 再操作:
  - `recopy_bundle_prompt(bundle_id)` → clipboard 再コピー
  - `open_bundle(bundle_id)` → explorer で再オープン

---

## 12. 永続化 / DB 変更

### 12.1 bundles テーブル (新規)

requirements §17 の DDL をそのまま採用。migration は **起動時に 1 回** 実行:

```rust
pub fn migrate_bundles_schema(conn: &Connection) -> Result<(), DbError>
```

- idempotent (IF NOT EXISTS)。
- 既存 `captures / sessions / hourly_summaries / daily_records` は **スキーマ維持**。
- insert は bundle writer 経由でのみ。

### 12.2 起動時整合性回復

```rust
UPDATE bundles
   SET status = 'failed',
       error_message = 'interrupted before completion'
 WHERE status IN ('recording', 'bundling');
```

- app 起動時に 1 回実行。
- 対応する raw session ディレクトリは残す。
- フロント `History > Bundle` で `failed` 行に「再ビルド」ボタンを出す (Phase 2)。

### 12.3 legacy 制御

- `recorder.rs`・`scheduler.rs`・旧 `session.rs`・`vlm/batch.rs` を経由する insert 経路は Phase 1 で全削除。
- History 画面 (Legacy タブ) は `db::list_captures / list_sessions / ...` を read-only で利用。

---

## 13. 設定 (AppConfig) 変更

### 13.1 残す項目

```rust
pub struct AppConfig {
    pub data_dir: String,
    pub setup_complete: bool,
    pub max_recording_minutes: u32,            // 既定 60
    pub record_keystrokes: bool,                // 既定 true
    pub record_uia_context: bool,               // 既定 true
    pub procedure_prompt_template: String,      // §12.6 を既定値に
    pub mask_rules: Vec<MaskRule>,              // 既存 (TextInput のポスト置換に使用)
    pub copilot_port: u16,                      // 当面残す (v2 で検討)
    pub edge_cdp_port: u16,                     // 当面残す
}
```

- `data_dir` / `setup_complete` / `mask_rules` は既存から移植。
- プロンプト関連は `procedure_prompt_template` のみ。
- 旧項目 (`capture_interval_secs`, `dhash_threshold`, `session_*`, `scheduler_enabled`,
  `batch_times`, `vlm_*`, `system_prompt`, `user_prompt`, `session_user_prompt`,
  `hourly_summary_prompt`, `daily_record_prompt`) は `config.rs` の migration で
  **黙って捨てる** (serde `#[serde(default)]` + カスタムデシリアライズ)。

### 13.2 source of truth を Rust 側に

- Svelte `settings/+page.svelte` は起動時に `get_config` を呼んで値を取る。
- デフォルト値を frontend で持たない。未取得ならスピナ。
- 保存は `save_config_command` のみ。差分 patch ではなく full object を送る。

---

## 14. Tauri コマンド / イベント契約

### 14.1 コマンド (invoke)

| name | args | returns | 備考 |
|---|---|---|---|
| `start_recording` | — | `StartRecordingResult { session_id: String }` | 既存名維持 |
| `stop_recording`  | — | `StopRecordingResult { session_id: String, stopping: true }` | |
| `get_recording_state` | — | `RecordingStatePayload` | 新設 |
| `list_bundles` | `{ limit: u32, offset: u32 }` | `Vec<BundleSummary>` | |
| `open_bundle` | `{ bundle_id: String }` | `()` | explorer.exe |
| `recopy_bundle_prompt` | `{ bundle_id: String }` | `()` | clipboard |
| `get_config` / `save_config_command` | 既存踏襲 | | source of truth は backend |

### 14.2 イベント (emit)

| event | payload | 用途 |
|---|---|---|
| `recording-state` | `RecordingStatePayload` | 主要な状態通知 |
| `recording-status` | `bool` | 移行期間互換 (tray 等旧購読者用) |
| `bundle-ready` | `{ bundle_id, folder_path, prompt_copied: bool, frame_count }` | stop 後 |
| `bundle-failed` | `{ bundle_id?, message }` | |

### 14.3 削除するコマンド / イベント

- `run_vlm_batch / cancel_vlm_batch / pause_vlm_batch / resume_vlm_batch`
- `start_vlm_server / stop_vlm_server / check_vlm_status`
- `check_copilot_connection`
- `capture_now` (将来必要なら再導入)
- `capture-added` event
- VLM batch 系イベント全般

---

## 15. フロントエンド設計 (Svelte)

### 15.1 型定義 (`src/lib/types/recording.ts`)

```ts
export type RecordingStatePayload =
  | { kind: "idle" }
  | { kind: "starting"; session_id: string }
  | { kind: "recording";
      session_id: string;
      started_at: string;
      elapsed_ms: number;
      event_count: number;
      screenshot_count: number;
      text_input_count: number;
      warning: string | null; }
  | { kind: "stopping"; session_id: string; reason: string }
  | { kind: "bundling"; session_id: string; phase: string; current: number; total: number }
  | { kind: "failed"; session_id: string | null; message: string };

export interface BundleReadyPayload {
  bundle_id: string;
  folder_path: string;
  prompt_copied: boolean;
  frame_count: number;
}
```

### 15.2 Dashboard

- `RecordingStatusCard`: 現在状態 (色分け) + 経過時間 + カウンタ + 注意文。
  - Recording: 赤パルス + 経過時間 + イベント数 + 画像枚数。
  - Bundling: 黄 + フェーズ + progress bar。
  - Failed: 赤 + message + 再試行ボタン (raw から再 bundle)。
- `BundleReadyCard`: stop 完了後表示。
  - `フォルダを再オープン` / `プロンプトを再コピー` / `閉じる`。
- 長時間記録 60 分到達通知は `recording-state` が `stopping(MaxDurationReached)` に
  遷移するので、それを検知してトーストで通知する。

### 15.3 History

- タブ 2 本:
  - **Bundle History**: `list_bundles()` から。サムネ (1 枚目) + 時刻 + frame_count + primary_app。
  - **Legacy Capture History**: 既存 `history/+page.svelte` のロジックを流用。
- Bundle の行を開くと:
  - フォルダを開く / プロンプトを再コピー / (Phase 2) 再ビルド。

### 15.4 Settings

- フォーム項目を §13 の新 AppConfig に合わせて削減。
- プロンプトは 1 項目だけ (procedure template)。
- max_recording_minutes は 5〜240 のスライダー or 数値入力。
- バックエンドから取得できるまでフォームを disabled にする (source of truth)。

---

## 16. プライバシー / 安全設計

### 16.1 記録中インジケータ

- Tray: `Idle=gray / Recording=red / Bundling=yellow` の 3 状態アイコンを用意。
- Dashboard: `RecordingStatusCard` が赤パルス + 経過時間。
- 通知: start 時 / stop 時 / bundle-ready 時に OS 通知 (`tauri-plugin-notification`)。

### 16.2 パスワードマスク実装

- `password_focus: AtomicBool` は UiaService が更新、InputListener と ScreenSampler が読み取る。
- InputListener: `password_focus=true` 時に `TextInput.text = "•".repeat(len)` + `masked=true`。
  実文字列は writer には渡らない。
- ScreenSampler: capture 直前に `password_rect` を参照して黒塗り。
  rect が無い場合は黒塗りせず、`Warning(ImeCompositionBestEffort)` ではなく
  `Warning { UiaUnavailable, message: "password rect unknown" }` を残す。
- UIA 判定不能な独自描画の UI は v1 では保証しない。
  bundle の prompt.md 冒頭で `"このバンドルには入力テキストが含まれる場合があります"` を必ず表示。

### 16.3 記録最大長

- Phase 1: `max_recording_minutes` で強制 Stop。
- 実装は sampler 近傍ではなく、`RecordingController` が別 tokio task で
  `tokio::time::sleep_until(started_mono + max_duration)` を張り、
  発火時に `stop(MaxDurationReached)` を呼ぶ。

### 16.4 ネットワーク送信

- v1 は `arboard` clipboard 書き込みのみが外部 I/O。
- `reqwest` / `vlm::server` / `copilot_server` の起動 / コールは v1 では発生しない。
  コードは残すが起動経路から外す。

---

## 17. フェーズ別タスク分解

requirements §19 を実装タスクに落とす。各タスクは 1〜2 日粒度。

### Phase 0 — 技術スパイク (3〜5 日)

- `crates/spike/` or `src-tauri/examples/input_spike.rs` で standalone。
- 課題:
  - `rdev::listen` + Tauri 同一プロセスで Enter 欠落が出ないか
  - UIA (Excel A1 / Notepad / Web password field) の取得可否
  - Windows Defender 実行下でのフック継続
- 成果物: Phase 0 レポート (`docs/phase0_report.md`) + helper fallback 採否。

### Phase 1 — 記録基盤 (~1.5 週)

1. 型 / モジュール雛形: `recording/{mod, event, session, service}.rs`。
2. `RecordingState` + `RecordingController` + Tauri command。
3. `InputListenerService` (rdev callback + recording_active)。
4. `writer.rs` (NDJSON + seq + stats)。
5. `screen_sampler.rs` (heartbeat のみで OK、trigger は最小)。
6. `get_recording_state` と `recording-state` event。
7. `session.json` / `events.ndjson` の書き出し。
8. `max_recording_minutes` タイムアウト。
9. DB `bundles` テーブル migration + 起動時整合性回復。
10. legacy scheduler / hourly summarizer / batch loop の起動経路削除。

**Exit criteria**: Excel で start → 入力 → stop で raw session が取れる。
Dashboard で Recording 状態と経過時間が見える。

### Phase 2 — Bundle 最小版 (~1.5 週)

1. `bundle/normalize.rs` (TextRun + step 束ね)。
2. `bundle/keyframe.rs` (最大 60 枚)。
3. `bundle/annotate.rs` (日本語フォント同梱、赤丸 + caption)。
4. `bundle/prompt.rs` + テンプレート。
5. `bundle/writer.rs` + manifest.json。
6. `open::that` + `arboard` clipboard。
7. Dashboard `BundleReadyCard` / History `Bundle History` タブ。
8. `bundles` status 更新 + error リカバリ UI。

**Exit criteria**: Excel A1=売上, B1=100, B2 クリック, メモ帳で done のテストが
E2E で通り、bundle フォルダが explorer で開き、prompt.md が clipboard に入る。

### Phase 3 — UIA / privacy (~1 週)

1. `uia_worker.rs` (STA thread + timeout)。
2. FocusChange と MouseUp で UIA request。
3. `password_focus` + `password_rect` 連携。
4. TextInput mask + screenshot redaction。
5. Warning event 類型の整備。
6. primary monitor 外検知。

**Exit criteria**: パスワード欄で TextInput が masked、screenshot が黒塗り、
UIA で cell_address が取れるケースが bundle で "cell A1" として残る。

### Phase 4 — UI polish / 配布 (~1 週)

1. Settings の刷新 + source of truth 徹底。
2. Legacy History との分離。
3. helper process packaging (Phase 0 で採用した場合)。
4. installer / Defender 確認 / 署名。
5. ドキュメント (`docs/user-manual.md` 更新)。

**リリース条件**: Phase 3 完了後のみ外部配布可 (requirements §19)。

---

## 18. 動作検証計画

### 18.1 自動テスト

- 単体テスト:
  - `recording/event.rs`: Event / EventKind の JSON roundtrip。
  - `bundle/normalize.rs`: 既知の events.ndjson フィクスチャ → 期待 Step。
  - `bundle/keyframe.rs`: 合成タイムラインで最大 60 枚に収まるか。
  - `bundle/prompt.rs`: digest のフォーマット。
- 統合テスト (Windows CI が理想だが v1 はローカル手動も可):
  - `session_lifecycle_test`: controller を使って start → 疑似 event 投入 → stop → events.ndjson 検証。
- legacy テスト: 既存 `db.rs` / `capture.rs` のテストはそのまま保つ。

### 18.2 手動テストケース

requirements §20 をそのまま利用:

- Excel A1 に `売上` 入力。
- パスワード欄入力で masked + redaction。
- E2E (Excel → メモ帳 → Stop)。
- 長時間記録 60 分自動 Stop。
- マルチモニタ警告。

### 18.3 回帰チェック

- scheduler / hourly summarizer / batch loop が起動しない (ログで確認)。
- 旧 `recording-status` event が引き続き tray に届く。
- legacy テーブルへの書き込みが発生しない。
- `bundles` migration が idempotent。

---

## 19. リスクと緩和策

| リスク | 影響 | 緩和策 |
|---|---|---|
| rdev が同一プロセスで不安定 | Phase 1 で詰む | Phase 0 で先に判定 / helper process 用 `bin/kiroku_input_hook.rs` |
| UIA 応答が遅く writer が詰まる | NDJSON 書き込み遅延 | 150ms timeout + 古い request drop |
| 日本語 IME の確定文字が取れない | 手順書品質低下 | v1 は best-effort 宣言、prompt.md に注意書き |
| password UI が UIA で判定できない | 情報漏洩リスク | prompt.md 先頭注意書き + v2 で review dialog |
| 30 分で 60 枚を超える高密度セッション | bundle 肥大 | keyframe 選定で均等間引き + 上限 |
| clipboard 書き込みのスレッド競合 | コピー失敗 | `BundleOpenService` に 1 スレッド集約 |
| 長時間記録で events.ndjson が巨大 | parse に時間 | 1 セッション 60 分 (≒数十 MB) までは serde_json で十分 |

---

## 20. 未決事項 / v2 以降

- active monitor capture (primary 以外を動的に選ぶ)
- bundle review dialog (手動再マスク、画像削除)
- helper process の本採用可否
- IME composition 記録
- app-specific adapter (Excel / Browser / IDE)
- Copilot への自動連携 (現 `vlm/copilot_server.rs` を将来拡張する場合の起点)

---

## 21. 実装開始条件の確認

requirements §22 の 3 条件:

1. ✅ Phase 0 で `rdev` の同一プロセス可否判定 → §17 Phase 0 に明記
2. ✅ `TextInput` と `PhysicalKey` を分離したイベントモデル → §5.1
3. ✅ v1 制約を UI / 文書に明記 → §15 (dashboard warning 表示), §16 (prompt.md 冒頭), §18 (user-manual.md 更新タスク)

本設計書で定義した型・モジュール境界・スレッドモデル・フェーズ計画に沿って
Phase 0 から着手する。
