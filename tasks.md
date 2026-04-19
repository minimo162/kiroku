# Kiroku 再設計 実装タスク (tasks.md)

作成日: 2026-04-19
参照: `design.md`

---

## 0. 進め方

- 実装は `design.md` の Phase 0 から順に進める。
- 各 Phase は Exit criteria を満たすまで次 Phase に進めない。
- privacy / raw 保全 / 自動送信しない方針を優先し、迷った場合は `design.md` を先に更新する。
- callback 内では blocking / I/O / lock を避ける。入力経路は bounded channel + `try_send` を基本にする。
- `events.ndjson` の正準順は `seq`。append 順に依存した実装をしない。
- Markdown は 1 チェック項目 1 行を維持し、Phase / subsection 単位で差分確認できるようにする。
- 新規 Tauri command 追加時は `invoke_handler` と capabilities の更新を同じタスクに含める。

### Cross-cutting acceptance criteria

- [ ] 生成物ごとに `*_schema_version` を持たせる（design.md §5.7 参照）。session.json は `session_schema_version` + `events_schema_version`、manifest.json は `manifest_schema_version`。旧 `bundle_version` / `bundle_schema_version` は使わない。
- [ ] EventKind ごとの drop policy を定義する。
- [ ] lifecycle event / key / text / MouseDown / MouseUp は drop 不可、MouseMove / Wheel は drop/coalesce 可にする。
- [ ] Stop sequence は gate input -> final capture -> drain UIA -> SessionStopped -> FlushBarrier -> close/join とする（design.md §7.2）。
- [ ] `SessionStopped` が FlushBarrier の対象に含まれるテストを追加する。
- [ ] UIA/privacy 未確定時は TextInput の実文字列を永続化しない。
- [ ] Phase 3 完了前の bundle は dev/internal only と明記し、`manifest.json.privacy_status = "unredacted"` で出力する。
- [ ] `manifest.json` は design.md §5.7.2 に従い、`manifest_schema_version`, `source_session_id`, `source_events_schema_version`, `source_event_count`, `event_count`, `dropped_event_count`, `warning_counts`, `privacy_status`, `prompt_path`, `prompt_sha256`, `notes` を含める。

---

## Phase 0 — 技術スパイク

目的: Windows 上で Phase 1 の前提技術が成立するかを先に確認する。

- [ ] `src-tauri/examples/input_spike.rs` または `crates/spike/` を作成する。
- [ ] `rdev::listen` を Tauri 同一プロセス相当で動かし、KeyPress / KeyRelease / MouseDown / MouseUp を記録する。
- [ ] Enter / Tab / Esc / Alt+Tab の欠落有無を確認する。
- [ ] Windows Defender 実行下で長時間フックが継続するか確認する。
- [ ] UIA STA thread の最小 spike を作る。
- [ ] Excel の cell address、Notepad の editable field、Web password field の取得可否を確認する。
- [ ] UIA 呼び出しが block した場合に deadline 超過応答を stale 扱いできるか確認する。
- [ ] `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` で foreground change を拾えるか確認する。
- [ ] helper process fallback (`src-tauri/src/bin/kiroku_input_hook.rs`) の採否を決める。
- [ ] 結果を `docs/phase0_report.md` にまとめる。

Exit criteria:

- [ ] `rdev` 同一プロセス利用の可否が判断済み。
- [ ] UIA 取得可能範囲と timeout / stale response の制約が文書化済み。
- [ ] helper process を採用するかどうか決定済み。

---

## Phase 1 — 記録基盤

目的: start / stop で raw session (`session.json`, `events.ndjson`, `screens/`) が残る最小記録基盤を作る。

Phase 1 は複数 PR に分割して進める。各 subsection の Exit criteria を満たしてから次へ進む。

### Phase 1A — Event schema / session skeleton / writer

目的: pseudo event を投入して raw session の最小保存経路を成立させる。

#### 1A.1 依存関係とモジュール雛形

- [ ] `src-tauri/Cargo.toml` に Phase 1 で必要な依存を追加する。
- [ ] `crossbeam-channel`, `arc-swap`, `bitflags`, `rdev` を追加する。
- [ ] Windows target の `windows` features に COM / UIA / Window event 用 feature を追加する。
- [ ] `src-tauri/src/recording/mod.rs` を追加し、公開 API を集約する。
- [ ] `recording/event.rs` を追加する。
- [ ] `recording/session.rs` を追加する。
- [ ] `recording/service.rs` を追加する。
- [ ] `recording/input_listener.rs` を追加する。
- [ ] `recording/writer.rs` を追加する。
- [ ] `recording/screen_sampler.rs` を追加する。
- [ ] `recording/window_focus.rs` を追加する。

#### 1A.2 Event / seq / schema version

- [ ] `EventEnvelope { seq, t_mono_ms, wallclock, kind }` を実装する。
- [ ] `EventKind` に `SessionStarted`, `SessionStopped`, input, screenshot, UIA, warning を定義する。
- [ ] `EventKind::UiaContext { for_seq, ... }` を定義する。
- [ ] `TextInput { text: Option<String>, masked, mask_reason }` を実装する。
- [ ] `WarningCode` と `MaskReason` を定義する。
- [ ] `RecordingSession::next_seq()` を実装し、producer 側で seq 採番する。
- [ ] `WriterMessage::Event` と `WriterMessage::FlushBarrier` を定義する。
- [ ] `session.json` に `session_schema_version`, `events_schema_version`, `app_version` を保存する（design.md §5.7.1）。
- [ ] 未知の `EventKind` を bundle normalize で Warning として扱い、可能な範囲で継続する方針を定義する。

#### 1A.3 Writer

- [ ] dedicated thread または `spawn_blocking` で writer を動かす。
- [ ] `crossbeam_channel::select!` で event / barrier / UIA response を受ける。
- [ ] writer は input event を UIA 待ちで遅延させず、UIA response を `UiaContext { for_seq, ... }` として別 event で append する。
- [ ] stale UIA response は `Warning(UiaStale)` として記録するか、破棄して metrics に残す。
- [ ] `events.ndjson` に 1 行 1 event で append する。
- [ ] flush を 50ms または 256 events で coalesce する。
- [ ] `FlushBarrier` では pending write を flush して ack する。
- [ ] seq 重複を `Warning(SeqDuplicate)` として記録する。
- [ ] `t_mono_ms` 逆行を `Warning(TimestampRegression)` として記録する。
- [ ] MouseMove を 50Hz に絞る。
- [ ] write error を `WriterSummary.errors` に積み、可能な限り継続する。
- [ ] 最後の不完全行を parse 側で捨てられる NDJSON 方針に合わせる。

#### 1A Exit criteria

- [ ] pseudo event を投入して `events.ndjson` が書ける。
- [ ] `SessionStarted` / `SessionStopped` / `Warning` が JSON roundtrip できる。
- [ ] `FlushBarrier` の unit test が通る。

### Phase 1B — RecordingController / commands / state event

目的: start / stop / state 取得と frontend への状態通知を成立させる。

#### 1B.1 RecordingController / state machine

- [ ] `RecordingState` 内部 enum を実装する。
- [ ] `RecordingStatePayload` を実装する。
- [ ] `RecordingController` を `AppState` に組み込む。
- [ ] `start_recording` command を新 RecordingController に接続する。
- [ ] `stop_recording` command を新 RecordingController に接続する。
- [ ] `get_recording_state` command を追加する。
- [ ] `recording-state` event を emit する。
- [ ] 互換用 `recording-status` は `Starting / Recording / Stopping` のみ `true` にする。
- [ ] Mutex を保持したまま I/O しないよう state transition を分離する。
- [ ] `max_recording_minutes` timeout を実装する: start 時に `RecordingController` が別 tokio task で `tokio::time::sleep_until(started_mono + max_duration)` を張り、発火時に `stop(MaxDurationReached)` を呼ぶ。
- [ ] max duration timeout は stop 成功 / `Failed` 遷移 / app shutdown のいずれでもキャンセルされることを確認する。
- [ ] max duration で stop した場合、`SessionStopped { reason: MaxDurationReached }` と `Warning(MaxDurationReached)` が events.ndjson に残るテストを追加する。

#### 1B.2 RecordingSession lifecycle

- [ ] `SessionId` を UUID newtype として実装する。
- [ ] `SessionPaths` を実装する。
- [ ] `recordings/<session_id>/events.ndjson` と `screens/` を作成する。
- [ ] `session.json` に config snapshot / primary monitor / app version / `session_schema_version` / `events_schema_version` を保存する。
- [ ] start 時に writer / sampler / focus task を起動する。
- [ ] start 時に `SessionStarted` を writer に送る。
- [ ] stop 時に state = `Stopping` へ遷移し、frontend / tray に emit する。
- [ ] stop 時に input producer から見える active flag を false にする。
- [ ] stop 時に新規 input / trigger を遮断する。
- [ ] stop 中も controller 内部では session handle を保持する。
- [ ] stop 時に `CaptureRequest::Final { ack }` を送る。
- [ ] stop 時に UIA response を短時間 drain する。
- [ ] stop 時に `SessionStopped` を writer に送る。
- [ ] stop 時に `FlushBarrier` ack を待つ。
- [ ] stop 時に producer channel を close する。
- [ ] sampler / focus / uia request task を join する。
- [ ] writer を close / join して `WriterSummary` を返す。
- [ ] DB を `bundling` へ遷移して bundle builder へ渡す。
- [ ] producer 公開 handle の無効化と controller 内部 session 保持を分離する。

#### 1B Exit criteria

- [ ] `start_recording` / `stop_recording` / `get_recording_state` が動く。
- [ ] `recording-state` が frontend に届く。
- [ ] 既存 `recording-status` 互換が残る。
- [ ] `SessionStopped` が FlushBarrier の対象に含まれる unit/integration test が通る。

### Phase 1C — ScreenSampler / raw session

目的: heartbeat / trigger / final capture を raw session に保存する。

#### 1C.1 ScreenSampler / WindowFocus

- [ ] `CaptureRequest::{Heartbeat, Triggered, Final}` を実装する。
- [ ] sampler は dedicated thread または `spawn_blocking` で動かす。
- [ ] heartbeat 2s capture を実装する。
- [ ] trigger capture は `MouseUp / Enter / Tab / Esc / Alt+Tab / FocusChange` 後 150ms にする。
- [ ] 400ms debounce を実装する。
- [ ] `Final` capture は debounce 対象外にする。
- [ ] screenshot event の seq と `screens/{seq:06}.png` を一致させる。
- [ ] `capture::capture_primary_monitor` を流用する。
- [ ] `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` で FocusChange を発火する。
- [ ] 500ms polling fallback を実装する。

#### 1C Exit criteria

- [ ] heartbeat screenshot が `screens/` に残る。
- [ ] screenshot event seq と filename が一致する。
- [ ] final capture が debounce されずに保存される。

### Phase 1D — InputListener / channel policy

目的: global input hook を bounded channel 経由で raw session に流す。

前提: Phase 0 で `rdev` 同一プロセス方針か helper process fallback かを決定済み（`docs/phase0_report.md`）。
以下の 1D.1 / 1D.2 は同一プロセス方針時のタスク。helper 採用時は 1D.3 を追加で実施する。

#### 1D.1 Channel / drop policy

- [ ] input channel を bounded にする。
- [ ] EventKind ごとの drop 可否を定義する。
- [ ] Never drop: `SessionStarted`, `SessionStopped`, `TextInput`, `PhysicalKeyDown`, `PhysicalKeyUp`, `MouseDown`, `MouseUp`, `Warning`。
- [ ] Coalesce/drop: `MouseMove`, `Wheel`。
- [ ] trigger capture request は debounce/coalesce 可能にする。
- [ ] channel full で non-droppable event が送れない場合は recording を `Failed` に遷移する。
- [ ] `Warning(EventDropped)` を 1 秒単位で集約する。

#### 1D.2 InputListenerService

- [ ] `rdev::listen` をプロセス常駐で 1 本起動する。
- [ ] callback は `recording_active=false` なら即 return する。
- [ ] `ArcSwapOption<SessionChannels>` から現在 session を lock-free に読む。
- [ ] modifier state を `AtomicU8` bitfield で保持する。
- [ ] printable key から `PhysicalKeyDown` と `TextInput` を生成する。
- [ ] control / navigation key は `PhysicalKeyDown` のみ生成する。
- [ ] `PhysicalKeyUp`, `MouseDown`, `MouseUp`, `MouseMove`, `Wheel` を生成する。
- [ ] MouseMove は callback 側で過剰に allocate しない。
- [ ] IME best-effort warning を session 開始時に 1 回記録する。
- [ ] UIA 未実装時は `TextInput` を強制 masked にする。
- [ ] UIA/privacy 未実装状態では `TextInput.text` は常に `None` または `[MASKED]` にし、実文字列を `events.ndjson` に残さない。
- [ ] `record_keystrokes=false` のとき key/text event が保存されないようにする。
- [ ] password field でなくても Phase 1 では raw text を保存しない。

#### 1D.3 helper process fallback（Phase 0 で採用決定時のみ）

- [ ] `src-tauri/src/bin/kiroku_input_hook.rs` を新規作成する。
- [ ] helper 側で `rdev::listen` を起動し、stdout に NDJSON 1 行 / event で書き出す最小実装を作る。
- [ ] main プロセスから helper を `std::process::Command` で起動 / 監視し、stdout を line-delimited JSON として読む。
- [ ] helper が死んだ場合の検知と restart / `Warning(EventDropped)` への集約を実装する。
- [ ] main プロセス側の `InputListenerService` を、rdev 直呼び経路と helper 経路の両方に切り替え可能にする（コンパイル時 feature または起動時 config）。
- [ ] helper 採用時の Phase 4 packaging と署名タスクと紐付ける。

#### 1D Exit criteria

- [ ] rdev event が bounded channel 経由で writer に入る（同一プロセス方針 or helper 方針のいずれか）。
- [ ] callback で lock / I/O / blocking がないことをコードレビューで確認する。
- [ ] channel full で MouseMove だけが落ち、TextInput が保持される unit test が通る。
- [ ] UIA/privacy 未確定時に TextInput の実文字列が JSON に出ない unit test が通る。
- [ ] helper 採用時は helper プロセスが落ちても main がハングせず `Failed` に遷移できる。

### Phase 1E — DB / config / legacy cutover / UI

目的: 新 recording lifecycle を既存アプリ起動経路と UI に接続し、旧自動処理を止める。

#### 1E.1 DB / config / legacy 起動経路

- [ ] `bundles` table migration を追加する。
- [ ] 起動時に `recording` / `bundling` row を `failed` に戻す。
- [ ] `start_recording` 時に `bundles(status='recording')` を insert する。
- [ ] `stop_recording` 時に `status='bundling'` と `ended_at` を update する。
- [ ] `AppConfig` を design.md §13 の項目へ整理する。
- [ ] 旧 config を `config.legacy.backup.json` に保存してから migration する。
- [ ] `record_keystrokes=false` の意味を backend に実装する。
- [ ] `record_uia_context=false` の場合、TextInput を強制 masked にする。
- [ ] scheduler / hourly summarizer / VLM batch loop の起動経路を外す。
- [ ] `spawn_scheduler`, `spawn_hourly_summarizer`, `spawn_copilot_auto_connect` が setup 時に起動しないことを確認する。
- [ ] VLM batch / VLM server / Copilot server 系 command の invoke handler 登録を v1 方針に合わせて整理する。
- [ ] legacy tables への新規 write が発生しないことを確認する。

#### 1E.2 Phase 1 UI

- [ ] `src/lib/types/recording.ts` を追加する。
- [ ] Dashboard を `RecordingStatePayload` ベースにする。
- [ ] `RecordingStatusCard` を追加する。
- [ ] `Starting / Recording / Stopping / Bundling / Failed` の表示を実装する。
- [ ] 経過時間 / event count / screenshot count を表示する。
- [ ] Stop / max duration 到達時の表示を実装する。

#### 1E Exit criteria

- [ ] `bundles` table migration が通る。
- [ ] legacy scheduler / hourly summarizer / VLM batch loop が起動しない。
- [ ] Dashboard が新 `RecordingStatePayload` を表示する。

### Phase 1 全体 Exit criteria

1A〜1E を統合した到達条件:

- [ ] Excel で start -> 入力 -> stop した raw session が残る。
- [ ] `session.json` が保存される（`session_schema_version` / `events_schema_version` 付き）。
- [ ] `events.ndjson` に `SessionStarted`, input event, screenshot, `SessionStopped` が残る。
- [ ] UIA/privacy 未実装状態では `TextInput.text` は常に `None` または `[MASKED]` になり、実文字列が `events.ndjson` に残らない。
- [ ] `record_keystrokes=false` のとき key/text event が保存されない。
- [ ] password field でなくても Phase 1 では raw text を保存しない。
- [ ] `max_recording_minutes` で自動 Stop が発火し、`SessionStopped { MaxDurationReached }` が残る。
- [ ] `bundles` に `recording -> bundling` の状態遷移が残る。

---

## Phase 2 — Bundle 最小版

目的: raw session から Copilot へ渡す folder / prompt / annotated frames を生成する。

注意:

- [ ] Phase 3 privacy 完了前の bundle は dev/internal only とし、配布・実業務利用しない。
- [ ] Phase 3 未完了時は UI 上に redaction 未完成 warning を出す。
- [ ] bundle manifest に `privacy_status: "unredacted" | "redacted"` を記録する。

### 1. bundle modules

- [ ] `src-tauri/src/bundle/mod.rs` を追加する。
- [ ] `bundle/normalize.rs` を追加する。
- [ ] `bundle/keyframe.rs` を追加する。
- [ ] `bundle/annotate.rs` を追加する。
- [ ] `bundle/prompt.rs` を追加する。
- [ ] `bundle/writer.rs` を追加する。

### 2. normalize

- [ ] `events.ndjson` を line-by-line parse する。
- [ ] 不完全な最終行を捨てて復旧できるようにする。
- [ ] events を seq 順に正準化する。
- [ ] 連続 `TextInput` を `TextRun` に畳む。
- [ ] `Enter / Tab / Escape / Alt+Tab / FocusChange / MouseUp` で TextRun を区切る。
- [ ] MouseMove を digest から除外し、annotate 用 trail にだけ残す。
- [ ] `for_seq` により input / click / focus と `UiaContext` を結合する。
- [ ] `MouseDown/MouseUp + UiaContext + 直後 Screenshot` を Step に束ねる。
- [ ] `Warning(OffPrimaryMonitor)` 区間を Step に反映する。
- [ ] 未知の `EventKind` は Warning として扱い、可能な範囲で継続する。
- [ ] 不完全行や未知 event を処理した件数を manifest の warning counts に反映する。

### 3. keyframe

- [ ] FocusChange 直後を候補にする。
- [ ] MouseUp 直前 / 直後を候補にする。
- [ ] TextRun 完了直後を候補にする。
- [ ] Enter / Tab / Escape / Alt+Tab 直後を候補にする。
- [ ] heartbeat 画像を候補にする。
- [ ] `KEYFRAME_DHASH_THRESHOLD` を固定値として定義する。
- [ ] `frame_cap = min(60, ceil(duration_min * 2))` を実装する。
- [ ] step 優先 + 均等サンプリングで枝刈りする。

### 4. annotate

- [ ] `imageproc` と `ab_glyph` を使って PNG 注釈を描画する。
- [ ] 日本語対応 TTF を `src-tauri/assets/fonts/` に追加する。
- [ ] redactions を最初に黒塗りする。
- [ ] click 座標に赤丸を描画する。
- [ ] 右上に step 番号を描画する。
- [ ] `{process} / {cell_address or role+name}` ラベルを描画する。
- [ ] 下部 caption を描画する。
- [ ] masked caption は `入力: [MASKED]` にする。
- [ ] off-primary 区間に赤ストライプ帯と注意文を描画する。

### 5. prompt / writer / handoff

- [ ] `procedure_prompt_template` の `<event_digest>` を差し替える。
- [ ] digest は最大 200 行に制限する。
- [ ] bundle folder を `<data_dir>/bundles/<yyyyMMdd_HHmmss>_<primary_app_slug>/` に作る。
- [ ] annotated images を bundle folder に配置する。
- [ ] raw `events.ndjson` を bundle folder にコピーする。
- [ ] `prompt.md` を出力する。
- [ ] `manifest.json` を出力する。
- [ ] `manifest.json` に design.md §5.7.2 のフィールド（`manifest_schema_version`, `source_session_id`, `source_events_schema_version`, `source_event_count`, `event_count`, `dropped_event_count`, `warning_counts`, `frame_count`, `prompt_path`, `prompt_sha256`, `privacy_status`, `notes`）を保存する。
- [ ] bundle 成功時に DB `status='ready'` を update する。
- [ ] bundle 失敗時に DB `status='failed'` と `error_message` を update する。
- [ ] `tauri-plugin-opener` で bundle folder を開く。
- [ ] `arboard` で prompt 内容を clipboard にコピーする。
- [ ] `bundle-ready` / `bundle-failed` event を emit する。

### 6. Phase 2 UI

- [ ] `BundleReadyCard` を追加する。
- [ ] `open_bundle(bundle_id)` command を実装する。
- [ ] `recopy_bundle_prompt(bundle_id)` command を実装する。
- [ ] History を Bundle / Legacy の 2 タブに分ける。
- [ ] `list_bundles(limit, offset)` を実装する。
- [ ] failed bundle の表示を実装する。

Exit criteria:

- [ ] Excel A1=売上, B1=100, B2 クリック, メモ帳で done の E2E raw session から bundle が生成できる。
- [ ] bundle folder が explorer で開く。
- [ ] `prompt.md` が clipboard に入る。
- [ ] annotated frame が最大 60 枚に収まる。
- [ ] `manifest.json` が design.md §5.7.2 の全必須フィールド（manifest_schema_version / source_session_id / source_events_schema_version / event counts / warning_counts / privacy_status / prompt_sha256）を含む。

---

## Phase 3 — UIA / privacy

目的: UIA context と privacy-first text / screenshot redaction を本実装する。

- [ ] `recording/uia_worker.rs` を実装する。
- [ ] STA thread で `CoInitializeEx(COINIT_APARTMENTTHREADED)` を呼ぶ。
- [ ] UIAutomation client を初期化する。
- [ ] `UiaRequest { for_seq, trigger, cursor, deadline_mono_ms }` を処理する。
- [ ] `UiaResponse { for_seq, payload, elapsed_ms, completed_mono_ms }` を返す。
- [ ] 150ms deadline を超えた response を stale として破棄する。
- [ ] stale UIA response は digest に混入させない。
- [ ] worker が N 秒以上戻らない場合の restart 方針を実装する。
- [ ] request queue を bounded にし、古い request を drop する。
- [ ] `FocusChange / MouseUp / Enter / Tab` で UIA request を発行する。
- [ ] `TextPrivacyTracker` を実装する。
- [ ] editable focus 直後は `UnknownSensitive` にする。
- [ ] `is_password=false` が deadline 内で確定した場合だけ `ConfirmedPlain` に戻す。
- [ ] timeout / unknown / password は masked にする。
- [ ] pending text は raw 永続化せず、plain 確定時だけ writer に送る。
- [ ] UIA が deadline 内に `ConfirmedPlain` を返した場合だけ plain text 保存を許可する。
- [ ] `password_rect` に基づく screenshot redaction を実装する。
- [ ] password rect 不明時の warning を実装する。
- [ ] primary monitor 外検知 warning を実装する。
- [ ] UIA unavailable / timeout warning を整備する。

Exit criteria:

- [ ] Web password field で `TextInput` が masked になる。
- [ ] password field の screenshot が黒塗りされる。
- [ ] UIA で Excel cell address が取れる場合、bundle digest に `cell A1` として残る。
- [ ] UIA timeout 時に raw text が漏れない。

---

## Phase 4 — UI polish / 配布

目的: v1 として使える UI、設定、配布物、ドキュメントを整える。

- [ ] Settings を新 `AppConfig` に合わせて刷新する。
- [ ] frontend が default config を持たず、backend を source of truth にする。
- [ ] `max_recording_minutes` を 5〜240 の入力にする。
- [ ] `record_keystrokes` と `record_uia_context` の危険な組み合わせを UI で禁止する。
- [ ] 旧 config backup の通知を実装する。
- [ ] Tray icon を `Idle / Recording / Bundling` に対応させる。
- [ ] OS notification を start / stop / bundle-ready で出す。
- [ ] Legacy History を read-only として分離する。
- [ ] helper process 採用時は packaging に含める。
- [ ] installer / Defender / 署名の確認を行う。
- [ ] `docs/user-manual.md` を更新する。
- [ ] v1 では Copilot 自動送信しないことを UI / docs に明記する。

Release criteria:

- [ ] Phase 3 の Exit criteria が完了している。
- [ ] Excel -> Notepad E2E が通る。
- [ ] パスワード欄 privacy test が通る。
- [ ] 60 分 max duration stop が通る。
- [ ] multi-monitor warning が出る。
- [ ] scheduler / hourly summarizer / VLM batch loop が起動していない。

---

## テストタスク

### Unit tests

- [ ] `recording/event.rs`: Event / EventKind JSON roundtrip。
- [ ] `recording/session.rs`: seq が単調増加する。
- [ ] `recording/session.rs`: stop lifecycle で `SessionStopped` が `FlushBarrier` 前に送られ、`events.ndjson` に残る。
- [ ] `recording/writer.rs`: duplicate seq warning。
- [ ] `recording/writer.rs`: timestamp regression warning。
- [ ] `recording/writer.rs`: FlushBarrier が flush 後に ack する。
- [ ] `recording/input_listener.rs`: MouseMove は drop/coalesce されるが TextInput / MouseUp は落ちない。
- [ ] `recording/input_listener.rs`: UIA 未確定時、TextInput の実文字列が JSON に出ない。
- [ ] `recording/writer.rs`: 途中で不完全行がある NDJSON を normalize が復旧できる。
- [ ] `recording/uia_worker.rs`: deadline 超過 response が digest に混入しない。
- [ ] `recording/screen_sampler.rs`: screenshot event seq と `screens/{seq:06}.png` が一致する。
- [ ] `bundle/normalize.rs`: fixture events -> expected Step。
- [ ] `bundle/normalize.rs`: `UiaContext.for_seq` で input / click / focus と UIA context を結合する。
- [ ] `bundle/keyframe.rs`: frame cap が最大 60 に収まる。
- [ ] `bundle/writer.rs`: manifest に design.md §5.7.2 の必須フィールド（manifest_schema_version / source_session_id / source_events_schema_version / event_count / source_event_count / frame_count / warning_counts / privacy_status / prompt_sha256）が入る。
- [ ] `bundle/prompt.rs`: digest format と 200 行制限。
- [ ] `config.rs`: legacy backup を作って migration する。
- [ ] `db.rs`: bundles migration が idempotent。
- [ ] legacy cutover: start/stop 後に旧 captures/sessions へ新規 write されない。

### Integration / manual tests

- [ ] start -> pseudo event -> stop -> events.ndjson 検証。
- [ ] Excel A1 に `売上` 入力。
- [ ] Excel B1 に `100` 入力。
- [ ] Excel から Notepad へ Alt+Tab して `done` 入力。
- [ ] Stop 後に bundle folder が開く。
- [ ] Stop 後に prompt が clipboard に入る。
- [ ] password field 入力で TextInput masked。
- [ ] password field screenshot redaction。
- [ ] 60 分 max duration 自動 stop。
- [ ] primary monitor 外操作 warning。

---

## 完了後の整理

- [ ] 未使用になった旧 command / event を削除する。
- [ ] `run_vlm_batch` 系 command を起動経路から外す。
- [ ] `capture-added` event を削除する。
- [ ] legacy write path が残っていないか `rg` で確認する。
- [ ] `README.md` と `docs/user-manual.md` の説明を v1 設計に合わせる。
