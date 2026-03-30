<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  type AppConfig = {
    capture_interval_secs: number;
    dhash_threshold: number;
    auto_delete_images: boolean;
    batch_time: string;
    vlm_host: string;
    vlm_max_tokens: number;
    data_dir: string;
  };

  const defaultConfig: AppConfig = {
    capture_interval_secs: 30,
    dhash_threshold: 10,
    auto_delete_images: true,
    batch_time: "22:00",
    vlm_host: "127.0.0.1:8080",
    vlm_max_tokens: 256,
    data_dir: ""
  };

  let config = $state<AppConfig>({ ...defaultConfig });
  let loading = $state(true);
  let saving = $state(false);
  let testing = $state(false);
  let selectingFolder = $state(false);
  let message = $state<string | null>(null);
  let testMessage = $state<string | null>(null);
  let tauriAvailable = $state(false);

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  async function loadConfig() {
    if (!isTauri()) {
      loading = false;
      return;
    }

    config = await invoke<AppConfig>("get_config");
    loading = false;
  }

  async function saveConfig() {
    if (!isTauri()) return;

    saving = true;
    message = null;
    try {
      config = await invoke<AppConfig>("save_config_command", { config });
      message = "設定を保存しました。記録中だった場合は新しい設定で再開しています。";
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }

  async function chooseDataDir() {
    if (!isTauri()) return;

    selectingFolder = true;
    try {
      const path = await invoke<string | null>("select_data_dir");
      if (path) {
        config = { ...config, data_dir: path };
      }
    } finally {
      selectingFolder = false;
    }
  }

  async function testConnection() {
    if (!isTauri()) return;

    testing = true;
    testMessage = null;
    try {
      const ok = await invoke<boolean>("test_vlm_connection", { vlmHost: config.vlm_host });
      testMessage = ok
        ? "VLM サーバーへの接続に成功しました。"
        : "VLM サーバーは応答しましたが、正常ステータスではありません。";
    } catch (error) {
      testMessage = error instanceof Error ? error.message : String(error);
    } finally {
      testing = false;
    }
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void loadConfig();

    if (!tauriAvailable) {
      message = "ブラウザプレビューでは設定の保存や接続テストを実行できません。";
    }
  });
</script>

<svelte:head>
  <title>Kiroku | 設定</title>
</svelte:head>

<section class="space-y-4">
  <div class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
    <div class="grid gap-6 px-6 py-6 lg:grid-cols-[1.1fr_0.9fr] lg:px-8">
      <div class="space-y-4">
        <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
          Settings
        </div>
        <h2 class="text-3xl font-bold text-ink-900">記録と VLM の設定</h2>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          キャプチャ間隔、差分閾値、VLM 接続先、バッチ実行時刻、保存先ディレクトリを管理します。
          保存時は `config.json` に永続化され、記録中なら新しい設定で録画ループを再起動します。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">Quick Notes</p>
        <div class="mt-4 space-y-3 text-sm leading-6 text-white/80">
          <p>キャプチャ間隔は 10 秒から 300 秒の範囲で調整できます。</p>
          <p>差分閾値を上げるほど、近い画面変化をスキップしやすくなります。</p>
          <p>保存先ディレクトリはキャプチャ画像と sidecar JSON の出力先です。</p>
        </div>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[1fr_0.9fr]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <div class="flex items-center justify-between gap-4">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Capture Settings</p>
          <h3 class="mt-2 text-2xl font-bold text-ink-900">キャプチャ設定</h3>
        </div>
        {#if loading}
          <span class="text-sm text-ink-400">読み込み中...</span>
        {/if}
      </div>

      <div class="mt-6 space-y-6">
        <div>
          <div class="flex items-center justify-between gap-3">
            <label class="text-sm font-medium text-ink-700" for="capture-interval">キャプチャ間隔</label>
            <span class="text-sm font-semibold text-brass-700">{config.capture_interval_secs} 秒</span>
          </div>
          <input
            id="capture-interval"
            class="mt-3 w-full accent-brass-600"
            type="range"
            min="10"
            max="300"
            step="10"
            bind:value={config.capture_interval_secs}
          />
        </div>

        <div>
          <div class="flex items-center justify-between gap-3">
            <label class="text-sm font-medium text-ink-700" for="dhash-threshold">差分閾値</label>
            <span class="text-sm font-semibold text-brass-700">{config.dhash_threshold}</span>
          </div>
          <input
            id="dhash-threshold"
            class="mt-3 w-full accent-brass-600"
            type="range"
            min="1"
            max="32"
            step="1"
            bind:value={config.dhash_threshold}
          />
          <p class="mt-2 text-sm text-ink-500">高いほど小さな画面変化を無視しやすくなります。</p>
        </div>

        <label class="flex items-center justify-between rounded-2xl border border-ink-100 px-4 py-4">
          <div>
            <p class="text-sm font-medium text-ink-700">VLM 処理後に画像を削除</p>
            <p class="mt-1 text-sm text-ink-500">ローカル画像を残さず、テキスト記述だけを保持します。</p>
          </div>
          <input class="h-5 w-5 accent-brass-600" type="checkbox" bind:checked={config.auto_delete_images} />
        </label>

        <div>
          <label class="text-sm font-medium text-ink-700" for="batch-time">バッチ開始時刻</label>
          <input
            id="batch-time"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="time"
            bind:value={config.batch_time}
          />
        </div>
      </div>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">VLM & Storage</p>
      <h3 class="mt-2 text-2xl font-bold text-ink-900">VLM 接続と保存先</h3>

      <div class="mt-6 space-y-5">
        <div>
          <label class="text-sm font-medium text-ink-700" for="vlm-host">VLM ホスト</label>
          <input
            id="vlm-host"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="text"
            bind:value={config.vlm_host}
            placeholder="127.0.0.1:8080"
          />
        </div>

        <div>
          <label class="text-sm font-medium text-ink-700" for="vlm-max-tokens">VLM 最大トークン数</label>
          <input
            id="vlm-max-tokens"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="number"
            min="64"
            max="2048"
            step="64"
            bind:value={config.vlm_max_tokens}
          />
        </div>

        <div>
          <label class="text-sm font-medium text-ink-700" for="data-dir">データ保存ディレクトリ</label>
          <div class="mt-3 flex flex-col gap-3 sm:flex-row">
            <input
              id="data-dir"
              class="min-w-0 flex-1 rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
              type="text"
              bind:value={config.data_dir}
              placeholder="C:\\Users\\..."
            />
            <button
              class="rounded-full border border-ink-200 bg-white px-4 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-60"
              onclick={chooseDataDir}
              disabled={!tauriAvailable || selectingFolder}
            >
              {selectingFolder ? "選択中..." : "フォルダを選択"}
            </button>
          </div>
        </div>

        <div class="rounded-2xl border border-ink-100 bg-ink-50/70 px-4 py-4">
          <div class="flex flex-wrap gap-3">
            <button
              class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
              onclick={saveConfig}
              disabled={!tauriAvailable || saving}
            >
              {saving ? "保存中..." : "設定を保存"}
            </button>
            <button
              class="rounded-full border border-ink-200 bg-white px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-60"
              onclick={testConnection}
              disabled={!tauriAvailable || testing}
            >
              {testing ? "確認中..." : "VLM 接続テスト"}
            </button>
          </div>

          {#if message}
            <p class="mt-4 text-sm leading-6 text-brass-700">{message}</p>
          {/if}
          {#if testMessage}
            <p class="mt-2 text-sm leading-6 text-ink-600">{testMessage}</p>
          {/if}
        </div>
      </div>
    </article>
  </div>
</section>
