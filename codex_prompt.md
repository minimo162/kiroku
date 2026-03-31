# Codex 実装指示: UI/UX 改善 — 初見でわかりやすい画面設計 (T21〜T25)

## 方針

- **既存デザインシステムを維持すること**: brass / ink / cinnabar パレット、
  Noto Sans JP、glassmorphism パネル（backdrop-blur、rounded-[2rem]）は変えない
- **Svelte 5 の $state / $derived を使うこと**（$store は使わない）
- **cargo check / pnpm build でエラーがないこと**（Rust 側の変更なし）
- 各タスクは独立して実装できる。T21 → T22 → T23 → T24 → T25 の順で実装すること

---

## T21: サイドバーに記録状態インジケーターを追加

### 対象ファイル
`src/routes/+layout.svelte`

### 変更内容

#### 1. script セクションに追加

既存の import 群の末尾に追加:
```typescript
import type { DashboardSnapshot } from "$lib/types/dashboard";
```

既存の `$state` 変数群の下に追加:
```typescript
let isRecording = $state(false);
let nextBatchAt = $state<string | null>(null);
```

`formatNextBatch` 関数を追加:
```typescript
function formatNextBatch(iso: string | null): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.valueOf())) return iso;
  return new Intl.DateTimeFormat("ja-JP", {
    hour: "2-digit",
    minute: "2-digit"
  }).format(date);
}
```

#### 2. onMount の非同期処理に追加

`disposed = false;` の直後（既存の copilot-login-required リスナーの前）に追加:

```typescript
// 起動時の初期状態を取得
void invoke<DashboardSnapshot>("get_dashboard_snapshot")
  .then((snap) => {
    if (!disposed) {
      isRecording = snap.stats.is_recording;
      nextBatchAt = snap.stats.next_batch_run_at;
    }
  })
  .catch(() => {});

// recording-status イベントを受信
const recordingUnlisten = await listen<boolean>("recording-status", (event) => {
  if (!disposed) isRecording = event.payload;
});
unlisteners.push(recordingUnlisten);

// scheduler-status イベントを受信
const schedulerUnlisten = await listen<string | null>("scheduler-status", (event) => {
  if (!disposed) nextBatchAt = event.payload;
});
unlisteners.push(schedulerUnlisten);
```

#### 3. サイドバーのヘッダーパネルに状態チップを追加

サイドバーの `<div class="rounded-[1.5rem] bg-ink-900 ...">` 内、
`<p class="mt-3 text-sm leading-6 text-white/75">...</p>` の直後に追加:

```svelte
<div class="mt-4 flex items-center gap-2">
  {#if isRecording}
    <span class="relative flex h-2.5 w-2.5 flex-shrink-0">
      <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-cinnabar-400 opacity-75"></span>
      <span class="relative inline-flex h-2.5 w-2.5 rounded-full bg-cinnabar-500"></span>
    </span>
    <span class="text-xs font-semibold text-cinnabar-300">記録中</span>
  {:else}
    <span class="h-2.5 w-2.5 flex-shrink-0 rounded-full bg-white/20"></span>
    <span class="text-xs text-white/40">停止中</span>
  {/if}
</div>
```

#### 4. サイドバー下部ショートカット欄に次回バッチ時刻を追加

既存の `<div class="mt-auto rounded-[1.5rem] border border-ink-100 ...">` 内、
`<p class="mt-3 text-sm leading-6 text-ink-600">...</p>` の直後に追加:

```svelte
{#if nextBatchAt}
  <p class="mt-3 text-xs font-medium text-ink-500">
    次回バッチ: <span class="font-semibold text-ink-700">{formatNextBatch(nextBatchAt)}</span>
  </p>
{/if}
```

---

## T22: ダッシュボードの記録アクション導線を明確化

### 対象ファイル
`src/routes/dashboard/+page.svelte`

### 変更内容

#### 1. メインカードの左カラムに状態バナーを追加

`<div class="space-y-5">` の先頭（`<div class="inline-flex items-center rounded-full ...">` の前）に追加:

```svelte
{#if stats.is_recording}
  <div class="flex items-center gap-3 rounded-2xl border border-cinnabar-200 bg-cinnabar-50 px-4 py-3">
    <span class="relative flex h-3 w-3 flex-shrink-0">
      <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-cinnabar-400 opacity-75"></span>
      <span class="relative inline-flex h-3 w-3 rounded-full bg-cinnabar-500"></span>
    </span>
    <div>
      <p class="text-sm font-semibold text-cinnabar-900">記録中</p>
      <p class="text-xs text-cinnabar-600">
        最終キャプチャ: {formatDateTime(stats.last_capture_at)}
      </p>
    </div>
  </div>
{:else}
  <div class="flex items-center gap-3 rounded-2xl border border-ink-200 bg-ink-50 px-4 py-3">
    <span class="h-3 w-3 flex-shrink-0 rounded-full bg-ink-300"></span>
    <p class="text-sm text-ink-500">記録は停止中です — 開始するには「記録開始」を押してください</p>
  </div>
{/if}
```

#### 2. ボタングループの視覚的重みを変更

既存のボタングループ `<div class="flex flex-wrap gap-3">` 内を以下に変更する:

```svelte
<div class="flex flex-wrap items-center gap-3">
  <!-- 主アクション: 記録開始/停止 (変更なし、既存のクラスを維持) -->
  <button
    class={`rounded-full px-5 py-3 text-sm font-semibold text-white transition ${
      stats.is_recording ? "bg-cinnabar-600 hover:bg-cinnabar-500" : "bg-ink-900 hover:bg-ink-700"
    } disabled:cursor-not-allowed disabled:opacity-60`}
    onclick={() => (stats.is_recording ? requestStopRecording() : void startRecording())}
    disabled={!tauriAvailable || actionPending !== null}
  >
    {#if actionPending === "recording"}
      更新中...
    {:else if stats.is_recording}
      記録停止
    {:else}
      記録開始
    {/if}
  </button>

  <!-- 区切り -->
  <span class="h-4 w-px bg-ink-200"></span>

  <!-- 副アクション: 説明文生成 (小さく、控えめに) -->
  <button
    class="rounded-full px-4 py-2 text-sm font-medium text-ink-400 transition hover:text-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
    onclick={runBatch}
    disabled={!tauriAvailable || actionPending !== null || stats.batch_running}
  >
    {#if actionPending === "batch"}
      実行中...
    {:else if stats.batch_running && !stats.server_running}
      起動中...
    {:else if stats.batch_running}
      生成中...
    {:else}
      説明文を一括生成
    {/if}
  </button>
</div>
```

---

## T23: 設定画面を基本設定と詳細設定に分離

### 対象ファイル
`src/routes/settings/+page.svelte`

### 変更内容

#### 1. $state を追加

既存の `let showCopilotAdvanced = $state(false);` の直下に追加:

```typescript
let showAdvanced = $state(false);
```

#### 2. 詳細設定に属するブロックを条件付きにする

以下の article / section を `{#if showAdvanced} ... {/if}` で囲む。
囲む対象は以下の通り（順番は既存のまま維持）:

**対象 1: セッション処理設定カード**
- Copilot エンジン選択時に表示される `{#if config.vlm_engine === "copilot"}` の中の
  セッション設定ブロック（`session_enabled` トグル以下のスライダー群）全体を囲む

**対象 2: システムプロンプト・ユーザープロンプト・セッションプロンプトのカード**
- これらを含む `<article>` を `{#if showAdvanced} ... {/if}` で囲む

**対象 3: マスクルール設定カード全体**
- マスクルール設定を含む `<article>` を `{#if showAdvanced} ... {/if}` で囲む

#### 3. 詳細設定切り替えボタンを追加

保存ボタンを含む `<div class="flex flex-wrap items-center justify-between gap-4">` の
**直前**（同じ `<section class="space-y-4">` 内）に追加:

```svelte
<div class="flex justify-center">
  <button
    type="button"
    onclick={() => (showAdvanced = !showAdvanced)}
    class="flex items-center gap-2 rounded-full border border-ink-200 bg-white px-4 py-2 text-sm text-ink-500 transition hover:border-ink-300 hover:text-ink-700"
  >
    <span>{showAdvanced ? "詳細設定を隠す" : "詳細設定を表示（プロンプト・マスク等）"}</span>
    <span class="text-ink-400">{showAdvanced ? "▲" : "▼"}</span>
  </button>
</div>
```

#### 4. 設定のヒントパネルのテキストを更新

右カラムの ink-900 パネル内のテキストを更新。
`<p>自動バッチを有効にすると...` の後に以下を追加:

```svelte
<p>「詳細設定を表示」でプロンプトやマスクルール等の上級設定にアクセスできます。</p>
```

---

## T24: ナビゲーションのラベルと不要バッジを整理

### 対象ファイル
`src/routes/+layout.svelte`

### 変更内容

#### 1. navItems 配列を修正

```typescript
// 変更前
{ href: "/preview", label: "記述プレビュー", status: "live" },

// 変更後
{ href: "/preview", label: "プレビュー", status: "live" },
```

#### 2. ナビリンクの右端バッジを削除

既存のナビリンク内:
```svelte
<a ... class={`flex items-center justify-between ...`}>
  <span>{item.label}</span>
  <span class={`rounded-full px-2 py-1 ...`}>
    {item.status === "live" ? "有効" : item.status}
  </span>
</a>
```

を以下に変更:
```svelte
<a ... class={`flex items-center gap-2 ...`}>
  <span>{item.label}</span>
</a>
```

- `justify-between` を削除し `gap-2` に変更
- バッジ `<span>` を完全に削除
- `aria-disabled` と `onclick` は維持する

---

## T25: セットアップ画面の Edge 起動を自動化

### 対象ファイル
`src/routes/setup/+page.svelte`

### 前提
- `check_copilot_connection` Tauri コマンドが T18〜T19 で実装済み
- このコマンドは内部で `ensureEdgeConnected()` を呼び Edge を自動起動する
- 戻り値: `{ connected: bool, login_required: bool, url?: string, error?: string }`

### 変更内容

#### 1. 型定義を追加

script セクションの先頭付近に追加:
```typescript
type CopilotConnectionStatus = {
  connected: boolean;
  login_required: boolean;
  url?: string | null;
  error?: string | null;
};
```

#### 2. $state を追加

既存の `let completing = $state(false);` の下に追加:
```typescript
let launchingEdge = $state(false);
let edgePollInterval = $state<number | null>(null);
```

#### 3. `launchAndWaitForEdge` 関数を追加

```typescript
async function launchAndWaitForEdge() {
  if (!isTauri()) return;
  launchingEdge = true;
  message = "Edge を起動しています...";

  try {
    const status = await invoke<CopilotConnectionStatus>("check_copilot_connection");
    if (status.connected || status.login_required) {
      // Edge 起動成功（login_required でも CDP は繋がっている）
      message = "Edge に接続しました。M365 Copilot にログインして「次へ」を押してください。";
      await loadStatus();
    } else {
      message = "接続を待機しています...";
      // 接続できない場合はポーリング
      edgePollInterval = window.setInterval(async () => {
        try {
          const s = await invoke<CopilotConnectionStatus>("check_copilot_connection");
          if (s.connected || s.login_required) {
            if (edgePollInterval !== null) {
              window.clearInterval(edgePollInterval);
              edgePollInterval = null;
            }
            message = "Edge に接続しました。M365 Copilot にログインして「次へ」を押してください。";
            await loadStatus();
          }
        } catch {
          // ポーリング中のエラーは無視
        }
      }, 2000);
    }
  } catch (error) {
    message = error instanceof Error ? error.message : String(error);
  } finally {
    launchingEdge = false;
  }
}
```

#### 4. `onDestroy` でポーリングをクリア

`onMount` の import の隣に `onDestroy` を追加し:
```typescript
import { onMount, onDestroy } from "svelte";
```

`onDestroy` を追加:
```typescript
onDestroy(() => {
  if (edgePollInterval !== null) {
    window.clearInterval(edgePollInterval);
  }
});
```

#### 5. edge-setup ステップの表示内容を変更

`{#if currentStep === "edge-setup"}` ブロック（またはそれに相当するステップ表示部分）内で、
Edge の手動起動手順テキスト（「--remote-debugging-port=9222 付きで起動」等）を以下に置き換える:

```svelte
<div class="space-y-4">
  <p class="text-sm leading-7 text-ink-600">
    「起動して接続確認」を押すと、Edge が自動的にデバッグモードで起動し Copilot ページを開きます。
    その後、M365 アカウントでのログインを確認してください。
  </p>

  <button
    type="button"
    onclick={launchAndWaitForEdge}
    disabled={launchingEdge || !tauriAvailable}
    class="w-full rounded-2xl bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
  >
    {launchingEdge ? "Edge を起動中..." : "Edge を起動して接続確認"}
  </button>

  {#if message}
    <p class="text-sm leading-6 text-ink-600">{message}</p>
  {/if}

  <!-- 接続後に「次へ」ボタンを表示 -->
  {#if status.edge_debugging_ready}
    <button
      type="button"
      onclick={() => (currentStep = "test-capture")}
      class="w-full rounded-2xl border border-brass-300 bg-brass-50 px-5 py-3 text-sm font-semibold text-brass-700 transition hover:bg-brass-100"
    >
      接続を確認しました — 次へ
    </button>
  {/if}
</div>
```

---

## 完了条件チェックリスト

- [ ] `pnpm build` がエラーなしで通過する
- [ ] サイドバーのヘッダーに記録中/停止中のインジケーターが表示される
- [ ] 記録中にパルスアニメーション（animate-ping）が動作する
- [ ] サイドバー下部に「次回バッチ: HH:MM」が表示される
- [ ] ダッシュボードのコンテンツ上部に記録中/停止中バナーが表示される
- [ ] 「説明文を一括生成」ボタンが視覚的に小さくなっている
- [ ] 設定画面に「詳細設定を表示 ▼」ボタンがある
- [ ] ボタンを押すとプロンプト・マスクルール・セッション詳細が展開される
- [ ] ナビゲーションの「記述プレビュー」が「プレビュー」になっている
- [ ] ナビゲーションの「有効」バッジが削除されている
- [ ] セットアップ画面の edge-setup ステップに「Edge を起動して接続確認」ボタンがある
- [ ] ボタン押下後に接続ポーリングが動作し、成功後に「次へ」ボタンが表示される
