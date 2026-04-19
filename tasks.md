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

### 1. 依存関係とモジュール雛形

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

### 2. Event / seq / channel policy

- [ ] `EventEnvelope { seq, t_mono_ms, wallclock, kind }` を実装する。
- [ ] `EventKind` に `SessionStarted`, `SessionStopped`, input, screenshot, UIA, warning を定義する。
- [ ] `TextInput { text: Option<String>, masked, mask_reason }` を実装する。
- [ ] `WarningCode` と `MaskReason` を定義する。
- [ ] `RecordingSession::next_seq()` を実装し、producer 側で seq 採番する。
- [ ] `WriterMessage::Event` と `WriterMessage::FlushBarrier` を定義する。
- [ ] input channel を bounded にする。
- [ ] channel full 時の drop policy を実装する。
- [ ] `Warning(EventDropped)` を 1 秒単位で集約する。

### 3. RecordingController / state machine

- [ ] `RecordingState` 内部 enum を実装する。
- [ ] `RecordingStatePayload` を実装する。
- [ ] `RecordingController` を `AppState` に組み込む。
- [ ] `start_recording` command を新 RecordingController に接続する。
- [ ] `stop_recording` command を新 RecordingController に接続する。
- [ ] `get_recording_state` command を追加する。
- [ ] `recording-state` event を emit する。
- [ ] 互換用 `recording-status` は `Starting / Recording / Stopping` のみ `true` にする。
- [ ] Mutex を保持したまま I/O しないよう state transition を分離する。

### 4. RecordingSession lifecycle

- [ ] `SessionId` を UUID newtype として実装する。
- [ ] `SessionPaths` を実装する。
- [ ] `recordings/<session_id>/events.ndjson` と `screens/` を作成する。
- [ ] `session.json` に config snapshot / primary monitor / app version を保存する。
- [ ] start 時に writer / sampler / focus task を起動する。
- [ ] start 時に `SessionStarted` を writer に送る。
- [ ] stop 時に `recording_active=false` と `ArcSwapOption::store(None)` を先に行う。
- [ ] stop 時に `CaptureRequest::Final { ack }` を送る。
- [ ] stop 時に `FlushBarrier` ack を待つ。
- [ ] stop 時に `SessionStopped` を送る。
- [ ] capture / focus / uia request channel を close して task を join する。
- [ ] UIA response を短時間 drain する。
- [ ] writer を close / join して `WriterSummary` を返す。

### 5. InputListenerService

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

### 6. Writer

- [ ] dedicated thread または `spawn_blocking` で writer を動かす。
- [ ] `crossbeam_channel::select!` で event / barrier / UIA response を受ける。
- [ ] `events.ndjson` に 1 行 1 event で append する。
- [ ] flush を 50ms または 256 events で coalesce する。
- [ ] `FlushBarrier` では pending write を flush して ack する。
- [ ] seq 重複を `Warning(SeqDuplicate)` として記録する。
- [ ] `t_mono_ms` 逆行を `Warning(TimestampRegression)` として記録する。
- [ ] MouseMove を 50Hz に絞る。
- [ ] write error を `WriterSummary.errors` に積み、可能な限り継続する。
- [ ] 最後の不完全行を parse 側で捨てられる NDJSON 方針に合わせる。

### 7. ScreenSampler / WindowFocus

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

### 8. DB / config / legacy 起動経路

- [ ] `bundles` table migration を追加する。
- [ ] 起動時に `recording` / `bundling` row を `failed` に戻す。
- [ ] `start_recording` 時に `bundles(status='recording')` を insert する。
- [ ] `stop_recording` 時に `status='bundling'` と `ended_at` を update する。
- [ ] `AppConfig` を design.md §13 の項目へ整理する。
- [ ] 旧 config を `config.legacy.backup.json` に保存してから migration する。
- [ ] `record_keystrokes=false` の意味を backend に実装する。
- [ ] `record_uia_context=false` の場合、TextInput を強制 masked にする。
- [ ] scheduler / hourly summarizer / VLM batch loop の起動経路を外す。
- [ ] legacy tables への新規 write が発生しないことを確認する。

### 9. Phase 1 UI

- [ ] `src/lib/types/recording.ts` を追加する。
- [ ] Dashboard を `RecordingStatePayload` ベースにする。
- [ ] `RecordingStatusCard` を追加する。
- [ ] `Starting / Recording / Stopping / Bundling / Failed` の表示を実装する。
- [ ] 経過時間 / event count / screenshot count を表示する。
- [ ] Stop / max duration 到達時の表示を実装する。

Exit criteria:

- [ ] Excel で start -> 入力 -> stop した raw session が残る。
- [ ] `session.json` が保存される。
- [ ] `events.ndjson` に `SessionStarted`, input event, screenshot, `SessionStopped` が残る。
- [ ] Dashboard で Recording 状態と経過時間が見える。
- [ ] `bundles` に `recording -> bundling` の状態遷移が残る。

---

## Phase 2 — Bundle 最小版

目的: raw session から Copilot へ渡す folder / prompt / annotated frames を生成する。

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
- [ ] `MouseDown/MouseUp + UiaContext + 直後 Screenshot` を Step に束ねる。
- [ ] `Warning(OffPrimaryMonitor)` 区間を Step に反映する。

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

---

## Phase 3 — UIA / privacy

目的: UIA context と privacy-first text / screenshot redaction を本実装する。

- [ ] `recording/uia_worker.rs` を実装する。
- [ ] STA thread で `CoInitializeEx(COINIT_APARTMENTTHREADED)` を呼ぶ。
- [ ] UIAutomation client を初期化する。
- [ ] `UiaRequest { for_seq, trigger, cursor, deadline_mono_ms }` を処理する。
- [ ] `UiaResponse { for_seq, payload, elapsed_ms, completed_mono_ms }` を返す。
- [ ] 150ms deadline を超えた response を stale として破棄する。
- [ ] worker が N 秒以上戻らない場合の restart 方針を実装する。
- [ ] request queue を bounded にし、古い request を drop する。
- [ ] `FocusChange / MouseUp / Enter / Tab` で UIA request を発行する。
- [ ] `TextPrivacyTracker` を実装する。
- [ ] editable focus 直後は `UnknownSensitive` にする。
- [ ] `is_password=false` が deadline 内で確定した場合だけ `ConfirmedPlain` に戻す。
- [ ] timeout / unknown / password は masked にする。
- [ ] pending text は raw 永続化せず、plain 確定時だけ writer に送る。
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
- [ ] `recording/writer.rs`: duplicate seq warning。
- [ ] `recording/writer.rs`: timestamp regression warning。
- [ ] `recording/writer.rs`: FlushBarrier が flush 後に ack する。
- [ ] `bundle/normalize.rs`: fixture events -> expected Step。
- [ ] `bundle/keyframe.rs`: frame cap が最大 60 に収まる。
- [ ] `bundle/prompt.rs`: digest format と 200 行制限。
- [ ] `config.rs`: legacy backup を作って migration する。
- [ ] `db.rs`: bundles migration が idempotent。

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
