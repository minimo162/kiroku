# Codex 実装指示: スケジュール複数時刻対応 + session_user_prompt 改善

以下の 2 つの変更を実装すること。

---

## 変更 1: バッチスケジュールを複数時刻対応にする

### 概要

現状は `AppConfig.batch_time: String`（1 日 1 回）のみ対応。
これを `batch_times: Vec<String>` に変更し、デフォルトで 12:00 と 17:30 の 2 回実行できるようにする。

既存の config.json との後方互換性を保つこと（`#[serde(default)]` + マイグレーション）。

---

### A. `src-tauri/src/models.rs`

`AppConfig` 構造体の `batch_time: String` を以下に置き換える:

```rust
pub batch_times: Vec<String>,
```

`Default` impl を更新:

```rust
batch_times: vec!["12:00".to_string(), "17:30".to_string()],
```

`batch_time: String` フィールドは **削除する**。

既存の unit test `app_config_roundtrip_json` の `batch_time` を `batch_times` に更新する。

---

### B. `src-tauri/src/scheduler.rs`

#### 1. `DEFAULT_BATCH_TIME` 定数を削除し、`DEFAULT_BATCH_TIMES` に置き換える

```rust
const DEFAULT_BATCH_TIMES: &[&str] = &["12:00", "17:30"];
```

#### 2. `next_run_at` 関数を全面的に書き換える

旧実装（`batch_time: &str` 1 つを処理）を廃止し、以下に変更する:

```rust
pub fn next_run_at(now: DateTime<Local>, config: &AppConfig) -> Option<DateTime<Local>> {
    if !config.scheduler_enabled {
        return None;
    }

    let times: Vec<&str> = if config.batch_times.is_empty() {
        DEFAULT_BATCH_TIMES.to_vec()
    } else {
        config.batch_times.iter().map(|s| s.as_str()).collect()
    };

    let now_naive = now.naive_local();

    // 各 batch_time について「今日の実行時刻」と「明日の実行時刻」を計算し、
    // now より後の最小値を選ぶ
    let candidate = times
        .iter()
        .filter_map(|t| NaiveTime::parse_from_str(t, "%H:%M").ok())
        .flat_map(|naive_time| {
            let today = now.date_naive().and_time(naive_time);
            let tomorrow = (now.date_naive() + chrono::Duration::days(1)).and_time(naive_time);
            [today, tomorrow]
        })
        .filter(|&dt| dt > now_naive)
        .min()?;

    match Local.from_local_datetime(&candidate) {
        LocalResult::Single(value) => Some(value),
        LocalResult::Ambiguous(earliest, _) => Some(earliest),
        LocalResult::None => None,
    }
}
```

#### 3. テストを更新する

既存の 2 つのテストを `batch_time` → `batch_times` に更新し、さらに以下のテストを追加する:

```rust
#[test]
fn next_run_at_picks_earlier_time_in_same_day() {
    let config = AppConfig {
        batch_times: vec!["12:00".to_string(), "17:30".to_string()],
        ..AppConfig::default()
    };

    // 11:00 → 次は今日の 12:00
    let now = Local
        .with_ymd_and_hms(2026, 4, 2, 11, 0, 0)
        .single()
        .expect("time should resolve");
    let next = next_run_at(now, &config).expect("next run should be calculated");
    assert_eq!(next.format("%Y-%m-%d %H:%M").to_string(), "2026-04-02 12:00");
}

#[test]
fn next_run_at_picks_second_time_after_first_passed() {
    let config = AppConfig {
        batch_times: vec!["12:00".to_string(), "17:30".to_string()],
        ..AppConfig::default()
    };

    // 13:00 → 今日の 12:00 は過ぎているので次は今日の 17:30
    let now = Local
        .with_ymd_and_hms(2026, 4, 2, 13, 0, 0)
        .single()
        .expect("time should resolve");
    let next = next_run_at(now, &config).expect("next run should be calculated");
    assert_eq!(next.format("%Y-%m-%d %H:%M").to_string(), "2026-04-02 17:30");
}

#[test]
fn next_run_at_rolls_to_next_day_after_all_times_passed() {
    let config = AppConfig {
        batch_times: vec!["12:00".to_string(), "17:30".to_string()],
        ..AppConfig::default()
    };

    // 18:00 → 今日の両方とも過ぎているので次は明日の 12:00
    let now = Local
        .with_ymd_and_hms(2026, 4, 2, 18, 0, 0)
        .single()
        .expect("time should resolve");
    let next = next_run_at(now, &config).expect("next run should be calculated");
    assert_eq!(next.format("%Y-%m-%d %H:%M").to_string(), "2026-04-03 12:00");
}
```

---

### C. `src-tauri/src/config.rs`

#### 1. マイグレーション関数を更新

`migrate_default_prompts` 内の `batch_time` 関連を以下に置き換える:

旧コード（削除）:
```rust
const LEGACY_BATCH_TIME: &str = "22:00";
const PREVIOUS_BATCH_TIME: &str = "18:00";
// ...
if config.batch_time == LEGACY_BATCH_TIME || config.batch_time == PREVIOUS_BATCH_TIME {
    config.batch_time = AppConfig::default().batch_time;
}
```

新コード:
```rust
// batch_times が空（旧 config.json に batch_times キーがない）の場合はデフォルトを設定
if config.batch_times.is_empty() {
    config.batch_times = AppConfig::default().batch_times;
}
```

`#[serde(default)]` が `AppConfig` に付いているため、旧 config.json に `batch_times` キーが存在しない場合は空 Vec になる。
これを上記コードでデフォルト値に補完する。

---

### D. `src/routes/settings/+page.svelte`

#### 1. 型定義を更新

```typescript
// 削除
batch_time: string;

// 追加
batch_times: string[];
```

#### 2. `defaultConfig` を更新

```typescript
// 削除
batch_time: "17:30",

// 追加
batch_times: ["12:00", "17:30"],
```

#### 3. 「バッチ開始時刻」UI を 2 つの時刻入力に変更

既存の `<input type="time" bind:value={config.batch_time} ...>` 部分を以下に置き換える:

```svelte
<div class="space-y-3">
  <label class="text-sm font-medium text-ink-700">バッチ開始時刻</label>
  <div class="flex items-center gap-3">
    <div class="flex-1">
      <p class="text-xs text-ink-500 mb-1">昼休み前</p>
      <input
        class="w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300 disabled:cursor-not-allowed disabled:opacity-50"
        type="time"
        value={config.batch_times[0] ?? "12:00"}
        oninput={(e) => {
          config.batch_times = [e.currentTarget.value, config.batch_times[1] ?? "17:30"];
        }}
        disabled={!config.scheduler_enabled}
      />
    </div>
    <div class="flex-1">
      <p class="text-xs text-ink-500 mb-1">定時前</p>
      <input
        class="w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300 disabled:cursor-not-allowed disabled:opacity-50"
        type="time"
        value={config.batch_times[1] ?? "17:30"}
        oninput={(e) => {
          config.batch_times = [config.batch_times[0] ?? "12:00", e.currentTarget.value];
        }}
        disabled={!config.scheduler_enabled}
      />
    </div>
  </div>
</div>
```

また、ラベルテキスト「夜間バッチを有効化」と説明文「指定時刻になると...」の「夜間バッチ」を
「自動バッチ」「バッチ処理」等に変更して時刻が昼にも対応することを示す。

---

## 変更 2: `session_user_prompt` デフォルト値の改善

### A. `src-tauri/src/models.rs`

`default_session_user_prompt()` 関数の中身を以下に置き換える:

```rust
pub fn default_session_user_prompt() -> String {
    concat!(
        "これは {start_time} から {end_time} の間（{duration_min}分間）の業務画面を",
        "{frame_count} 枚のスクリーンショットにまとめたコラージュです。",
        "画像は左上から右下へ時系列順に並んでいます。\n",
        "この間の業務操作の流れを2〜5文で説明してください。",
        "必ず次の観点を含めてください:\n",
        "  使用中のアプリケーション、",
        "最初に何をしていたか・途中でどう変化したか・最後の状態、",
        "画面内で読み取れる固有ラベル・表題・件数・ボタン名。\n",
        "入力内容や意図は画面から裏付けられる範囲に限定し、",
        "単に画面を確認しているだけに見える場合は「〇〇を確認・閲覧している」と記述してください。",
        "業務と無関係な画面（ブラウザのニュース閲覧等）が含まれる場合はその旨も明記してください。",
        "出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。"
    )
    .to_string()
}
```

### B. `src-tauri/src/config.rs`

`migrate_default_prompts` 内の `LEGACY_SESSION_USER_PROMPT` 定数を、
**変更前の `default_session_user_prompt()` の内容**（現在の値）で更新する。
具体的には現在の定数値をそのまま LEGACY として残し、新しい値は `default_session_user_prompt()` が返す。

現在の `LEGACY_SESSION_USER_PROMPT` に加えて、中間バージョンとして以下も追加する:

```rust
const PREVIOUS_SESSION_USER_PROMPT: &str = concat!(
    "これは {start_time} から {end_time} の間（{duration_min}分間）の",
    "業務画面の流れです。{frame_count} 枚のスクリーンショットを",
    "時系列順に並べたコラージュを見て、この間に行っていた業務操作を",
    "1〜3文で説明してください。必ず次の観点を含めてください: ",
    "使用中のアプリケーション、実行している操作の流れ、",
    "表示されているデータや対象、画面内で読み取れる固有ラベルや表題。",
    "入力内容や意図は画面から裏付けられる範囲に限定し、",
    "単に画面を追っているだけに見える場合は確認・閲覧の流れとして記述してください。",
    "出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。"
);
```

マイグレーション条件を以下に更新:

```rust
if config.session_user_prompt.trim().is_empty()
    || config.session_user_prompt == LEGACY_SESSION_USER_PROMPT
    || config.session_user_prompt == PREVIOUS_SESSION_USER_PROMPT
{
    config.session_user_prompt = default_session_user_prompt();
}
```

---

## 完了条件チェックリスト

- [ ] `cargo check` がエラーなしで通過する
- [ ] `cargo test` が全テスト通過する（特に scheduler.rs の新テスト 3 件）
- [ ] 旧 `batch_time` フィールドへの参照が models.rs / scheduler.rs / config.rs / settings/+page.svelte のいずれにも残っていない
- [ ] 設定画面に「昼休み前」「定時前」の 2 つの時刻入力が表示される
- [ ] `next_run_at` が 11:00 → 12:00 / 13:00 → 17:30 / 18:00 → 翌日 12:00 を正しく返す
- [ ] `session_user_prompt` のデフォルト値が新しい内容になっている
- [ ] 既存の config.json（`batch_times` キーなし）を読み込んだ場合に `batch_times` が `["12:00", "17:30"]` に補完される
