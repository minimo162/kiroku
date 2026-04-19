# Kiroku 再設計（改訂版）

作成日: 2026-04-17  
対象: `minimo162/kiroku`  
状態: 実装着手前レビュー反映版

---

## 1. 目的

現行の Kiroku は、固定間隔のスクリーンショットを継続取得し、時間ギャップでセッションを自動分割し、説明文を自動生成する流れを前提としている。  
本改訂版では、アプリの役割を次のように切り替える。

- 記録対象は、ユーザーが **Start** を押してから **Stop** を押すまでの **明示的な 1 セッション** に限定する
- セッション中は、スクリーンショットだけでなく、**キーボード・マウス入力** と **UI Automation による UI 要素コンテキスト** を記録する
- Stop 後は、自動送信ではなく、**画像 + prompt.md + 生ログ** をまとめた **Copilot 用バンドル** を生成する
- Copilot には「2〜5 文の要約」ではなく、**第三者が同じ操作を再現できる番号付き手順書** を書かせる

---

## 2. この改訂版で確定したこと

### 2.1 維持するもの

- Tauri コマンド名 `start_recording` / `stop_recording` は維持する
- 既存のスクリーンショット取得プリミティブは流用する
- 既存 DB の `captures / sessions / hourly_summaries / daily_records` は当面残すが、新規書き込みは停止する

### 2.2 置き換えるもの

- 連続監視 + 時間ギャップ分割
- scheduler
- hourly / daily summary
- Copilot 自動送信
- バッチ実行前提の VLM 進捗 UI

### 2.3 新たに明示した制約

- **v1 は Windows 専用**
- **v1 の画面保証範囲は primary monitor のみ**
- **IME 入力は best-effort**
  - 物理キーは必ず記録する
  - 実文字列は OS から確定文字が受け取れた分のみ記録する
  - 変換中の composition state の完全再現は v1 の非目標
- **自動送信はしない**
- **パスワード欄は UIA 判定できた場合のみマスク / 黒塗りする**

---

## 3. ゴール / 非ゴール

## 3.1 ゴール

1. Start/Stop で区切られた 1 セッションを高密度に記録できる
2. 記録結果から、画像とイベントログを束ねた再現用バンドルを生成できる
3. Copilot に手動投入したとき、第三者が再現可能な手順書を出させやすい材料を提供できる
4. 途中クラッシュしても、生ログと撮影済み画像は可能な範囲で回収できる
5. 記録中であること、テキストが含まれること、パスワード欄がマスク対象であることをユーザーに明示できる

## 3.2 非ゴール

- macOS / Linux 対応
- 自動 OCR
- Copilot への自動アップロード / 自動送信
- IME 変換状態の完全再現
- マルチモニタ全画面の完全同期記録
- 操作意図の推測
- 既存 legacy データの即時完全移行

---

## 4. ユーザーフロー

1. ユーザーが **Start** を押す
2. アプリは `Starting` に入り、セッションディレクトリを確保する
3. 状態が `Recording` に遷移し、入力・スクリーンショット・UIA コンテキストを収集する
4. ユーザーが **Stop** を押す、または最大記録時間に到達する
5. アプリは `Stopping` に入り、未書き込みイベントと UIA 応答をドレインする
6. `Bundling` に入り、キーフレーム抽出・注釈・`prompt.md`・`manifest.json` を生成する
7. バンドル完成後、エクスプローラでフォルダを開き、`prompt.md` 本文をクリップボードへコピーする
8. 履歴画面には bundle として保存される

---

## 5. アーキテクチャ方針

本設計は、**長寿命サービス** と **セッション単位オブジェクト** を分離する。

### 5.1 長寿命サービス

- `InputListenerService`
  - アプリ起動中は 1 本だけ存在する
  - `rdev::listen` の blocking 特性に合わせ、**セッションごとの start/stop はしない**
  - 記録中かどうかは `recording_active: AtomicBool` で判定する
- `UiaService`
  - 専用 STA スレッドを持つ
  - 初回記録開始時に lazily 起動してもよい
  - セッション中のみ要求を受ける
- `BundleOpenService`
  - エクスプローラ起動とクリップボード書き込みを担当する
  - クリップボードは 1 箇所・1 スレッドでだけ扱う

### 5.2 セッション単位オブジェクト

- `RecordingSession`
  - セッション ID
  - monotonic seq generator
  - 生ログ writer
  - screenshot sampler
  - セッション統計
  - stop / flush / bundle build の orchestration

### 5.3 helper process フォールバック

同一プロセス内の `Tauri + rdev` が対象 PC で安定しない場合は、**helper process** に切り替える。  
この分岐は Phase 0 の技術スパイクで判定する。

- 既定方針: 同一プロセスの専用スレッドで実装
- フォールバック方針: `src-tauri/src/bin/kiroku_input_hook.rs` のような別プロセスに入力フックを分離し、IPC でイベントを受け取る

---

## 6. v1 スコープ上の重要制約

### 6.1 マルチモニタ

v1 は **primary monitor only** とする。  
ユーザーが 2 枚目以降のモニタで操作した場合、入力イベント自体は記録できても、画像注釈の一致保証は行わない。

実装上の扱い:

- 記録開始時に複数モニタを検出したら注意トーストを表示する
- 非 primary monitor 上の操作を検知したら `Warning` イベントを残す
- バンドル生成時、その区間には「画面保証外」の注記を入れる
- active-monitor capture は v2 バックログ

### 6.2 IME

v1 では文字入力モデルを以下の 2 系統に分ける。

- `PhysicalKeyDown / PhysicalKeyUp`
- `TextInput`

`TextInput` は OS から確定文字列が受け取れた場合にのみ出す。  
IME の composition 過程そのものは再現対象にしない。

### 6.3 パスワード欄

- UIA で `is_password` と判定できた場合のみマスク対象
- その間の `TextInput` は `"***"` 相当へ置換する
- `UiaContext.value` は保存しない
- `bounding_rect` が取れた場合のみスクリーンショット上を黒塗りする
- UIA 判定不能な独自描画のパスワード UI は v1 では完全保証しない

---

## 7. バックエンドの新モジュール構成

```text
src-tauri/src/
  recording/
    mod.rs
    service.rs          # RecordingController / public API
    session.rs          # 1 セッションのライフサイクル
    event.rs            # Event / EventKind / serialization
    input_listener.rs   # rdev listener service
    screen_sampler.rs   # heartbeat + event-trigger capture
    uia_worker.rs       # STA thread + UIA query
    writer.rs           # NDJSON append / counts / seq consistency
  bundle/
    mod.rs
    normalize.rs        # TextRun 化・ノイズ削減・digest 用整形
    keyframe.rs
    annotate.rs
    prompt.rs
    writer.rs
  bin/
    kiroku_input_hook.rs   # helper process fallback 用（必要時のみ）
```

### 7.1 削除 / 縮小対象

- `src-tauri/src/scheduler.rs` は削除
- `src-tauri/src/session.rs` の旧セッション分割 / コラージュ生成は削除
- `src-tauri/src/vlm/batch.rs` の自動セッション処理ループは削除
- `src-tauri/src/lib.rs` の scheduler / hourly summarizer / copilot auto-connect 起動は削除
- `src/copilot_server.ts` は将来用に残してもよいが、v1 では起動トリガーを外す

---

## 8. 状態モデル

```rust
enum RecordingState {
    Idle,
    Starting { session_id: String },
    Recording {
        session_id: String,
        started_at: DateTime<Local>,
        elapsed_ms: u64,
        event_count: u64,
        screenshot_count: u64,
        text_input_count: u64,
        warning: Option<String>,
    },
    Stopping { session_id: String },
    Bundling { session_id: String, progress: BundleProgress },
    Failed { session_id: Option<String>, message: String },
}
```

### 8.1 遷移

`Idle -> Starting -> Recording -> Stopping -> Bundling -> Idle`

例外系:

- `Starting -> Failed`
- `Recording -> Stopping`（ユーザー Stop）
- `Recording -> Stopping`（最大記録時間到達）
- `Bundling -> Failed`

### 8.2 互換性方針

- Tauri command 名は `start_recording` / `stop_recording` を維持
- 新規 command として `get_recording_state` を追加
- イベントは以下を新設
  - `recording-state`
  - `bundle-ready`
  - `bundle-failed`
- 互換性のため、移行期間中は `recording-status: bool` の emit を併用してよい

---

## 9. 記録パイプライン

## 9.1 InputListenerService

### 責務

- `rdev::listen` によるグローバル入力受信
- 記録中のみ、現在アクティブな `RecordingSession` へイベントを送る
- 記録外のイベントは破棄する

### 重要な設計

- `listen` は **アプリ生存中 1 回だけ起動**
- セッションごとにスレッドを作らない
- callback 内では **絶対に I/O しない**
- callback 内では次だけ行う
  - monotonic time 取得
  - seq 採番
  - `Event` 構築
  - session writer への非同期送信
  - 必要に応じて UIA リクエスト / capture リクエスト投下

### イベント種別

- `MouseDown / MouseUp / MouseMove / Wheel`
- `PhysicalKeyDown / PhysicalKeyUp`
- `TextInput`

### マウス移動の扱い

- 50Hz 上限
- もしくは 200ms 以上の idle 後の最初の move のみ
- bundle digest では通常破棄し、注釈用補助データとしてのみ使う

## 9.2 ScreenSampler

### 役割

スクリーンショットは固定 10 秒間隔ではなく、**心拍 + イベントトリガー** で撮る。

### ルール

1. **Heartbeat**
   - 2 秒ごとに 1 枚
2. **Event trigger**
   - `MouseUp`
   - `Enter / Tab / Escape`
   - `Alt+Tab`
   - 明示的 `FocusChange`
   の後、150ms 後に 1 枚
3. **Debounce**
   - 直近 400ms 以内の capture は統合
4. **Stop final**
   - Stop 時に最終 1 枚

### 方針

- v1 は `capture_primary_monitor` を用いる
- capture 結果には `cause_seq` を残す
- screenshot 保存前に、必要であれば password rect の黒塗りを実施する

## 9.3 UiaService

### 実装

- 専用 OS スレッド
- `CoInitializeEx(COINIT_APARTMENTTHREADED)` を 1 回だけ実行
- `UIAutomation` を保持
- `Receiver<UiaRequest>` で要求を受け取る

### トリガー

- `FocusChange`
- `MouseUp`
- `Enter`
- `Tab`

### 応答

```rust
struct UiaContextPayload {
    for_seq: u64,
    role: Option<String>,
    name: Option<String>,
    value: Option<String>,
    cell_address: Option<String>,
    bounding_rect: Option<Rect>,
    is_password: bool,
}
```

### 付加機能

- `password_focus: AtomicBool`
- `password_rect: Mutex<Option<Rect>>`
- これを screen sampler と input listener が参照する

### フォールバック

UIA 取得に失敗した場合は:

- `window_meta.rs` で取れるアプリ名 / タイトルへフォールバック
- `UiaContext` が取れなかったことを `Warning` イベントに残す

## 9.4 Writer

writer は 1 箇所に集約する。責務は以下。

- seq 整合性の維持
- `events.ndjson` append
- session counters 更新
- 非同期 UIA 応答の再結合
- shutdown 時 flush
- bundle build 前の final snapshot 出力

**重要**: NDJSON 書き込み・集計・UIA 再結合は 1 箇所で行う。  
これにより race の中心を writer に閉じ込める。

---

## 10. タイムラインデータモデル

```rust
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

enum StopReason {
    UserRequested,
    MaxDurationReached,
    AppShutdown,
    FatalError,
}

enum EventKind {
    SessionStarted,
    SessionStopped { reason: StopReason },

    FocusChange {
        hwnd: Option<i64>,
        title: String,
        process: String,
    },

    MouseDown {
        button: String,
        x: i32,
        y: i32,
    },
    MouseUp {
        button: String,
        x: i32,
        y: i32,
    },
    MouseMove {
        x: i32,
        y: i32,
    },
    Wheel {
        dx: i64,
        dy: i64,
    },

    PhysicalKeyDown {
        key: String,
        shift: bool,
        ctrl: bool,
        alt: bool,
        win: bool,
    },
    PhysicalKeyUp {
        key: String,
    },

    TextInput {
        text: Option<String>,       // masked 時は None
        masked: bool,
        mask_reason: Option<MaskReason>,
    },

    Screenshot {
        path: String,
        dhash: Option<String>,
        width: u32,
        height: u32,
        cause_seq: Option<u64>,
        redactions: Vec<Rect>,
    },

    UiaContext {
        for_seq: u64,
        role: Option<String>,
        name: Option<String>,
        value: Option<String>,
        cell_address: Option<String>,
        bounding_rect: Option<Rect>,
        is_password: bool,
    },

    Warning {
        code: WarningCode,
        message: String,
    },
}

enum WarningCode {
    OffPrimaryMonitor,
    UiaUnavailable,
    UiaTimeout,
    UiaWorkerRestarted,
    EventDropped,
    SeqDuplicate,
    TimestampRegression,
    ImeCompositionBestEffort,
    ScreenshotFailed,
    MaxDurationReached,
}

enum MaskReason {
    PasswordField,
    UiaPending,
    UiaTimeout,
    UiaDisabled,
    MaskRule,
}

struct Event {
    seq: u64,
    t_mono_ms: u64,
    wallclock: DateTime<Local>,
    kind: EventKind,
}
```

### 10.1 正規化ルール

bundle 生成前に `normalize.rs` で次を行う。

- 連続 `TextInput` を `TextRun` 化
- `PhysicalKeyDown/Up` のうち、実文字に対応しない特殊キーだけを digest に残す
- 高頻度 `MouseMove` を落とす
- `MouseDown/MouseUp + Screenshot + UiaContext` を 1 ステップ候補に束ねる

---

## 11. 永続化とフォルダ構成

### 11.1 生セッション

```text
<data_dir>/
  recordings/
    <session_id>/
      session.json
      events.ndjson
      screens/
        000001.png
        000002.png
        ...
```

`session.json` のフィールド定義は design.md §5.7.1 を正準とする。要点:

- `session_schema_version` / `events_schema_version`
- `session_id`, `started_at`, `app_version`
- `primary_monitor`（width / height / scale）
- `config_snapshot`（max_recording_minutes / record_keystrokes / record_uia_context）

### 11.2 バンドル

```text
<data_dir>/
  bundles/
    20260417_153012_excel/
      01_focus_excel.png
      02_click_a1.png
      03_type_uriage.png
      ...
      events.ndjson
      manifest.json
      prompt.md
```

### 11.3 原則

- `events.ndjson` は raw session から **コピー** して bundle に同梱する
- raw session は bundle 生成後も残す
- bundle は何度でも再生成できる

---

## 12. Stop 時のバンドル生成

## 12.1 Stop シーケンス

1. `recording_active = false`
2. screen sampler 停止
3. UIA request channel をクローズ
4. 未処理 UIA 応答を drain
5. writer flush
6. final screenshot
7. `Bundling` 状態へ遷移
8. bundle 生成
9. DB 更新
10. フォルダ open + clipboard copy

## 12.2 正規化

`bundle/normalize.rs` で次を行う。

- `TextInput` を連結して `"売上"` のような塊にする
- `Enter / Tab / Escape / Alt+Tab` を step boundary とみなす
- `UiaContext(cell_address)` が付いたクリックを Excel 操作候補として優先する
- `Warning(off_primary_monitor)` 区間を bundle に注記する

## 12.3 キーフレーム選定

採択条件:

- `FocusChange` 直後
- `MouseUp` 直前 / 直後
- `TextRun` 完了直後
- `Enter / Tab / Escape / Alt+Tab` 直後
- `dhash` 差分が閾値超過
- 2 秒 heartbeat 画像

制限:

- 30 分で最大 60 フレーム目安
- 超過時は、ステップ優先 + 均等サンプリング

## 12.4 画像注釈

- クリック位置に赤丸 + ステップ番号
- 右上に UI ラベル
  - 例: `Excel / cell A1`
- 下部に入力キャプション
  - 例: `入力: "売上"`
- `off_primary_monitor` 区間には `primary monitor 外の操作` と明記
- password rect がある場合は先に黒塗りし、その後に注釈する

## 12.5 manifest.json

フィールド定義は design.md §5.7.2 を正準とする。要点のみ抜粋:

- `manifest_schema_version` / `source_session_id` / `source_events_schema_version`
- `status`, `started_at`, `ended_at`, `primary_app`
- `frame_count`, `source_event_count`, `event_count`, `dropped_event_count`
- `warning_counts`（`WarningCode` → 件数）
- `privacy_status`: `"unredacted" | "redacted"` — Phase 3 未完了 bundle は `"unredacted"`
- `prompt_path`, `prompt_sha256`
- 人間向け注記は `notes` に載せる（例: "IME input is best-effort", "v1 captures primary monitor only"）

## 12.6 prompt.md

```md
# 手順マニュアル作成依頼

添付の画像 (01〜NN) は、私が行った一連の操作を時系列順に示しています。
各画像には次の情報が含まれます。

- 赤い丸 + 番号: クリック位置
- 右上ラベル: その時点での UI 要素やアプリ情報
- 下部キャプション: 入力した文字列
- 一部区間には「primary monitor 外の操作」等の注意書きが含まれます

以下のイベントダイジェストも参考にしてください。

<event_digest>
12:03:01  focus  -> Excel 月次決算.xlsx
12:03:04  click  -> cell A1
12:03:05  type   -> "売上"
12:03:07  key    -> Enter
...
</event_digest>

これらを元に、第三者が同じ操作を再現できる手順書を作成してください。

制約:
- 画像とイベントログから確認できる事実だけを書く
- 推測はしない
- 目的が不明なら「目的は不明」と書く
- 出力は Markdown の番号付きリストのみ
- 各ステップは次の形式にする

N. <操作内容>
   - 画面: <アプリ名 / ウィンドウ名>
   - 根拠: 画像 NN
```

## 12.7 ユーザー引き渡し

- `explorer.exe` で bundle フォルダを開く
- `prompt.md` 本文を clipboard にコピー
- フロントには以下を表示
  - `バンドルを開きました`
  - `画像を Copilot にドラッグし、プロンプトを貼り付けてください`
  - `このバンドルには入力テキストが含まれる場合があります`

---

## 13. 依存クレート

```toml
[dependencies]
rdev = "0.5"
uiautomation = "0.24"
crossbeam-channel = "0.5"
arboard = "3"
```

既存の `xcap`, `image`, `chrono`, `uuid`, `windows`, `tokio`, `serde_json`, `anyhow` は継続利用する。

---

## 14. プライバシー / 安全設計

### 14.1 記録中インジケータ

- トレイアイコンを赤にする
- ダッシュボードに赤パルスと経過時間を表示する
- bundle 生成中は `Bundling` を明示する

### 14.2 パスワードマスク

- `password_focus = true` 中の `TextInput` はマスク
- `UiaContext.value` は保存しない
- 取得できた `bounding_rect` は黒塗り
- 判定不能時は完全保証しないため、v1 の制約として明記する

### 14.3 記録最大長

- デフォルト 60 分
- **Phase 1 で実装**
- 超過時は強制 Stop + 通知 + bundle 生成

### 14.4 レビュー機能

- v1: フォルダを開く前に注意トーストのみ
- v2 候補: bundle review dialog / 手動再マスク / 画像削除

### 14.5 ネットワーク送信

- v1 の bundle 生成では自動送信しない
- クリップボード書き込み以外の外部転送は発生しない

---

## 15. フロントエンド変更

## 15.1 ダッシュボード

- Start / Stop ボタンは維持
- 表示内容を `RecordingState` ベースに変更
- リアルタイム表示:
  - 経過時間
  - 入力イベント数
  - 画像枚数
  - 現在状態 (`Recording / Stopping / Bundling`)
  - 注意文 (`primary monitor のみ保証` など)

Stop 完了後に以下を表示する。

- 保存先パス
- 画像枚数
- `プロンプトを再コピー`
- `フォルダを再オープン`

## 15.2 履歴

履歴は 2 画面に分ける。

- `Bundle History`
- `Legacy Capture History`

legacy を即時消さず、移行期間中は read-only で残す。

## 15.3 Settings

### 新設 / 維持

- `procedure_prompt_template`
- `max_recording_minutes`
- `record_keystrokes`
- `record_uia_context`

### 削除

- batch time
- hourly / daily summary prompt
- session prompt / system prompt / user prompt の多重定義
- 旧 capture interval 関連
- 自動 Copilot 接続前提の設定

### 重要方針

**設定の source of truth は Rust 側に置く。**  
Svelte 側はデフォルト値を持たず、バックエンドから取得した値だけを表示・編集する。

---

## 16. Tauri コマンド / イベント契約

### 16.1 Commands

- `start_recording() -> StartRecordingResult`
- `stop_recording() -> StopRecordingResult`
- `get_recording_state() -> RecordingStatePayload`
- `list_bundles() -> Vec<BundleSummary>`
- `open_bundle(bundle_id)`
- `recopy_bundle_prompt(bundle_id)`

### 16.2 Events

- `recording-state`
- `recording-status`（bool, 移行期間のみ）
- `bundle-ready`
- `bundle-failed`

---

## 17. データベース変更

`bundles` テーブルを追加する。

```sql
CREATE TABLE IF NOT EXISTS bundles (
  id TEXT PRIMARY KEY,
  bundle_version INTEGER NOT NULL,
  status TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  session_dir_path TEXT NOT NULL,
  folder_path TEXT,
  frame_count INTEGER NOT NULL DEFAULT 0,
  event_count INTEGER NOT NULL DEFAULT 0,
  primary_app TEXT,
  prompt_path TEXT,
  error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_bundles_started_at
  ON bundles(started_at);
```

### 17.1 status

- `recording`
- `bundling`
- `ready`
- `failed`

### 17.2 起動時の整合性回復

アプリ起動時に `recording / bundling` のまま残った row を調べる。

- raw session が残っていれば `failed` に更新
- `error_message = "interrupted before completion"` を設定
- 必要なら後で再 bundle 可能

### 17.3 legacy テーブル

- `captures / sessions / hourly_summaries / daily_records` は read-only
- 新規書き込みは停止
- 履歴画面からは legacy と bundle を分けて扱う

---

## 18. 変更対象ファイル

## 18.1 新規

- `src-tauri/src/recording/{mod,service,session,event,input_listener,screen_sampler,uia_worker,writer}.rs`
- `src-tauri/src/bundle/{mod,normalize,keyframe,annotate,prompt,writer}.rs`
- `src-tauri/src/bin/kiroku_input_hook.rs`（helper fallback が必要な場合）

## 18.2 大改造

- `src-tauri/src/lib.rs`
- `src-tauri/src/recorder.rs` または recording 起点となる既存箇所
- `src-tauri/src/db.rs`
- `src-tauri/src/models.rs`
- `src-tauri/src/Cargo.toml`
- `src/routes/dashboard/+page.svelte`
- `src/routes/history/+page.svelte`
- `src/routes/settings/+page.svelte`
- `src/lib/types/*.ts`

## 18.3 削除 / 縮小

- `src-tauri/src/scheduler.rs`
- 旧 `src-tauri/src/session.rs`
- `src-tauri/src/vlm/batch.rs` の自動処理ループ
- scheduler / hourly summary / auto copilot 接続を前提とするコード

---

## 19. フェーズ計画

## Phase 0 — 技術スパイク

### 目的

以下を先に確定する。

1. `rdev + UIA` が対象 PC で動くか
2. Tauri 同一プロセスで keyboard event が欠落しないか
3. 欠落するなら helper process に切り替えるべきか
4. Excel / Notepad / password field の最低限ケースが通るか

### Exit Criteria

- standalone bin で `click -> text -> enter` が取れる
- Tauri 窓 focused / unfocused の両方で keyboard event を比較する
- AV 有効時に listener がブロックされない
- Excel の `cell A1` が最低 1 ケース取得できる

## Phase 1 — 記録の基盤

- `RecordingState`
- `InputListenerService`
- `RecordingSession`
- `events.ndjson`
- `get_recording_state`
- max recording duration
- `bundles.status = recording`
- legacy scheduler 系停止

## Phase 2 — bundle 最小版

- normalize
- keyframe
- annotate
- `manifest.json`
- `prompt.md`
- explorer open
- clipboard copy
- `bundles.status = bundling / ready / failed`

## Phase 3 — UIA / privacy 強化

- UIA worker
- Excel cell address
- password mask
- screenshot redaction
- fallback warning

## Phase 4 — UI / polish / signing

- dashboard / history / settings 完成
- legacy history 分離
- helper process packaging（必要時）
- installer / signing / Defender 確認
- ドキュメント整備

### リリース条件

**外部配布可能な build は Phase 3 完了後のみ。**  
理由: password mask 未実装の build は安全条件を満たさない。

---

## 20. 動作検証

## 20.1 単体スパイク

Excel で以下を実行。

1. A1 をクリック
2. `売上` と入力
3. Enter

期待値:

- `FocusChange`
- `MouseUp`
- `TextInput("売上")`
- `PhysicalKeyDown("Enter")`
- `UiaContext(cell_address = "A1")`

## 20.2 パスワードマスク

ログイン画面のパスワード欄で入力。

期待値:

- `TextInput` が masked
- `UiaContext.value` が保存されない
- 可能なら screenshot 上の欄が黒塗り

## 20.3 End-to-End

1. Start
2. Excel で A1=`売上`, B1=`100`
3. B2 クリック
4. Alt+Tab でメモ帳へ移動
5. `done` 入力
6. Stop

期待値:

- bundle フォルダが開く
- 画像 6〜12 枚程度
- `prompt.md` が存在
- `<event_digest>` に `cell A1`, `"売上"`, `Enter`, `focus -> メモ帳` が入る
- clipboard に prompt 本文が入る

## 20.4 回帰

- scheduler が起動しない
- hourly / daily summary 書き込みが走らない
- legacy table は read-only
- `bundles` migration が適用される

## 20.5 長時間記録

- 60 分到達で自動 Stop
- bundle 生成
- UI 通知

## 20.6 マルチモニタ

- 複数モニタ接続時に注意表示
- primary monitor 外操作で `Warning` イベント
- bundle に注記

---

## 21. 未来のバックログ

- active monitor capture
- review dialog
- 手動再マスク
- app-specific adapters（Excel / Browser / IDE）
- helper process を既定実装に昇格するかの再評価
- IME composition の追加記録

---

## 22. 実装開始時の判断基準

この設計は **GO**。  
ただし、着手条件は次の 3 点を満たすこと。

1. Phase 0 で `rdev` の同一プロセス可否を判定する
2. `TextInput` と `PhysicalKey` を分離したイベントモデルで始める
3. v1 制約（primary monitor only / IME best-effort / password mask 条件付き）を UI と文書に明記する

以上を守る限り、本設計は現行 Kiroku を「粗い自動要約アプリ」から「再現可能な操作記録ツール」へ安全に移行できる。
