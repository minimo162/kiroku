<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { addToast } from "$lib/toast.svelte";

  type MaskRule = {
    pattern: string;
    replacement: string;
    is_regex: boolean;
  };

  type AppConfig = {
    capture_interval_secs: number;
    dhash_threshold: number;
    auto_delete_images: boolean;
    session_enabled: boolean;
    session_gap_secs: number;
    session_window_secs: number;
    max_frames_per_collage: number;
    scheduler_enabled: boolean;
    setup_complete: boolean;
    batch_times: string[];
    vlm_engine: string;
    vlm_host: string;
    vlm_max_tokens: number;
    copilot_port: number;
    edge_cdp_port: number;
    data_dir: string;
    system_prompt: string;
    user_prompt: string;
    session_user_prompt: string;
    mask_rules: MaskRule[];
  };

  type CopilotConnectionStatus = {
    connected: boolean;
    login_required: boolean;
    url?: string | null;
    error?: string | null;
  };

  const createMaskRule = (): MaskRule => ({
    pattern: "",
    replacement: "[MASKED]",
    is_regex: false
  });

  const defaultConfig: AppConfig = {
    capture_interval_secs: 10,
    dhash_threshold: 10,
    auto_delete_images: true,
    session_enabled: true,
    session_gap_secs: 600,
    session_window_secs: 300,
    max_frames_per_collage: 6,
    scheduler_enabled: true,
    setup_complete: false,
    batch_times: ["12:00", "17:30"],
    vlm_engine: "copilot",
    vlm_host: "127.0.0.1:8080",
    vlm_max_tokens: 256,
    copilot_port: 18080,
    edge_cdp_port: 9222,
    data_dir: "",
    system_prompt:
      "あなたは経理部門向けの業務記録アシスタントです。必ず画像内の文字、ラベル、表、ボタン名、件数表示など、画面上で実際に確認できる情報を優先して日本語で簡潔に記述してください。SAP GUI、Excel、Outlook、Teams、Web システムなどの業務画面を対象とし、連結PKG、内部取引消去、UPI、月次決算、メール確認、会議参加などの業務用語は、画面上の表示や文脈から明確に裏付けられる場合のみ使ってください。見えていない操作や意図は推測せず、単に画面を閲覧しているだけに見える場合は閲覧・確認中と明示してください。画像を受け取っている前提で回答し、「スクリーンショットを確認できない」のような定型文は、画像が本当に判読不能な場合を除いて使わないでください。",
    user_prompt:
      "このスクリーンショットに写っている業務操作を1〜3文で説明してください。必ず次の観点を含めてください: 使用中のアプリケーション、現在行っている操作または確認行為、表示されているデータ・対象・画面名。画面内に読める固有ラベル、カード名、件数、ボタン名、表題があれば優先して文章に含めてください。操作が明確でない場合は、何を入力したかを推測せず「ダッシュボードを確認している」「一覧を閲覧している」のように記述してください。出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。",
    session_user_prompt:
      "これは {start_time} から {end_time} の間（{duration_min}分間）の業務画面を{frame_count} 枚のスクリーンショットにまとめたコラージュです。画像は左上から右下へ時系列順に並んでいます。\nこの間の業務操作の流れを2〜5文で説明してください。必ず次の観点を含めてください:\n  使用中のアプリケーション、最初に何をしていたか・途中でどう変化したか・最後の状態、画面内で読み取れる固有ラベル・表題・件数・ボタン名。\n入力内容や意図は画面から裏付けられる範囲に限定し、単に画面を確認しているだけに見える場合は「〇〇を確認・閲覧している」と記述してください。業務と無関係な画面（ブラウザのニュース閲覧等）が含まれる場合はその旨も明記してください。出力は自然な日本語の文章のみとし、箇条書きや JSON は使わないでください。",
    mask_rules: []
  };

  let config = $state<AppConfig>({ ...defaultConfig });
  let loading = $state(true);
  let saving = $state(false);
  let testing = $state(false);
  let selectingFolder = $state(false);
  let copilotStatus = $state<"checking" | "connected" | "login_required" | "disconnected">(
    "disconnected"
  );
  let copilotStatusMessage = $state("接続テストを実行すると Copilot の状態を確認できます。");
  let copilotStatusUrl = $state<string | null>(null);
  let showCopilotAdvanced = $state(false);
  let maskPreviewInput = $state("株式会社A の売上 120,000 円を Excel で確認");
  let tauriAvailable = $state(false);
  let fieldErrors = $state({
    vlmHost: null as string | null,
    vlmMaxTokens: null as string | null
  });
  let touched = $state({
    vlmHost: false,
    vlmMaxTokens: false
  });

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  function validateVlmHost(value: string) {
    return /^[^:\s]+:\d+$/.test(value.trim()) ? null : "host:port 形式で入力してください";
  }

  function validateMaxTokens(value: number) {
    return value >= 64 && value <= 2048 ? null : "64〜2048 の範囲で入力してください";
  }

  function refreshValidation() {
    fieldErrors = currentValidationErrors();
  }

  function updateBatchTime(index: number, value: string) {
    const nextBatchTimes = [...config.batch_times];
    nextBatchTimes[index] = value;
    config = { ...config, batch_times: nextBatchTimes };
  }

  function touchField(field: keyof typeof touched) {
    touched = { ...touched, [field]: true };
    refreshValidation();
  }

  function currentValidationErrors() {
    return {
      vlmHost: null,
      vlmMaxTokens: validateMaxTokens(config.vlm_max_tokens)
    };
  }

  let hasErrors = $derived(Boolean(fieldErrors.vlmHost || fieldErrors.vlmMaxTokens));

  async function loadConfig() {
    if (!isTauri()) {
      loading = false;
      return;
    }

    try {
      config = await invoke<AppConfig>("get_config");
      refreshValidation();
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      loading = false;
    }
  }

  async function saveConfig() {
    if (!isTauri()) return;
    const nextErrors = currentValidationErrors();
    fieldErrors = nextErrors;
    if (nextErrors.vlmHost || nextErrors.vlmMaxTokens) {
      touched = { vlmHost: true, vlmMaxTokens: true };
      addToast("error", "入力エラーを解消してから保存してください。");
      return;
    }

    saving = true;
    try {
      config = await invoke<AppConfig>("save_config_command", { config });
      copilotStatus = "disconnected";
      copilotStatusMessage = "設定を保存しました。必要に応じて接続テストで Copilot の状態を確認してください。";
      copilotStatusUrl = null;
      addToast("success", "設定を保存しました。記録中の場合は新しい設定で再開します。");
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
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

  function applyCopilotStatus(status: CopilotConnectionStatus) {
    copilotStatusUrl = status.url ?? null;

    if (status.login_required) {
      copilotStatus = "login_required";
      copilotStatusMessage =
        status.error ?? "Copilot にログインしてください。Edge の画面を確認してください。";
      return;
    }

    if (status.connected) {
      copilotStatus = "connected";
      copilotStatusMessage = "Copilot へ接続できています。";
      return;
    }

    copilotStatus = "disconnected";
    copilotStatusMessage = status.error ?? "Edge または Copilot に接続できていません。";
  }

  async function testCopilotConnection() {
    if (!isTauri()) return;

    testing = true;
    copilotStatus = "checking";
    copilotStatusMessage = "Copilot への接続を確認しています...";
    try {
      const status = await invoke<CopilotConnectionStatus>("check_copilot_connection");
      applyCopilotStatus(status);

      if (status.connected) {
        addToast("success", "Copilot への接続を確認しました。");
      } else if (!status.login_required) {
        addToast("error", copilotStatusMessage);
      }
    } catch (error) {
      copilotStatus = "disconnected";
      copilotStatusMessage = error instanceof Error ? error.message : String(error);
      addToast("error", copilotStatusMessage);
    } finally {
      testing = false;
    }
  }
  function addMaskRule() {
    config = {
      ...config,
      mask_rules: [...config.mask_rules, createMaskRule()]
    };
  }

  function updateMaskRule(index: number, nextRule: MaskRule) {
    config = {
      ...config,
      mask_rules: config.mask_rules.map((rule, currentIndex) =>
        currentIndex === index ? nextRule : rule
      )
    };
  }

  function removeMaskRule(index: number) {
    config = {
      ...config,
      mask_rules: config.mask_rules.filter((_, currentIndex) => currentIndex !== index)
    };
  }

  const buildMaskPreview = () => {
    try {
      const text = config.mask_rules.reduce((current, rule) => {
        if (!rule.pattern.trim()) {
          return current;
        }

        const replacement = rule.replacement || "[MASKED]";
        if (rule.is_regex) {
          return current.replace(new RegExp(rule.pattern, "g"), replacement);
        }

        return current.split(rule.pattern).join(replacement);
      }, maskPreviewInput);
      return { text, error: null };
    } catch (error) {
      return {
        text: maskPreviewInput,
        error: error instanceof Error ? error.message : String(error)
      };
    }
  };

  let maskPreview = $derived.by(() => buildMaskPreview());

  function handleKeyDown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      if (!saving && !loading && !hasErrors) {
        void saveConfig();
      }
    }
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void loadConfig();
    window.addEventListener("keydown", handleKeyDown);

    if (!tauriAvailable) {
      addToast("info", "ブラウザプレビューでは設定の保存を実行できません。");
    }
  });

  onDestroy(() => {
    if (typeof window !== "undefined") {
      window.removeEventListener("keydown", handleKeyDown);
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
          設定
        </div>
        <h2 class="text-3xl font-bold text-ink-900">記録と分析の設定</h2>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          キャプチャ間隔、画面変化の検出感度、分析エンジンの接続先、一括生成の実行時刻、保存先を管理します。
          保存すると即座に反映されます。記録中の場合は新しい設定で記録を再開します。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">設定のヒント</p>
        <div class="mt-4 space-y-3 text-sm leading-6 text-white/80">
          <p>キャプチャ間隔は 3 秒から 300 秒の範囲で調整できます。</p>
          <p>検出感度を上げるほど、近い画面変化をスキップしやすくなります。</p>
          <p>自動バッチを有効にすると、指定時刻に未処理フレームの説明文を自動生成します。</p>
          <p>保存先ディレクトリはキャプチャ画像と関連データの出力先です。</p>
        </div>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[1fr_0.9fr]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <div class="flex items-center justify-between gap-4">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">キャプチャ設定</p>
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
            min="3"
            max="300"
            step="10"
            bind:value={config.capture_interval_secs}
          />
        </div>

        <div>
          <div class="flex items-center justify-between gap-3">
            <label class="text-sm font-medium text-ink-700" for="dhash-threshold">画面変化の検出感度</label>
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
          <p class="mt-2 text-sm text-ink-500">高いほど微細な変化を無視します。</p>
        </div>

        <label class="flex items-center justify-between rounded-[1.75rem] border border-brass-200 bg-brass-50/80 px-4 py-4 shadow-[inset_0_1px_0_rgba(255,255,255,0.65)]">
          <div>
            <div class="flex flex-wrap items-center gap-2">
              <p class="text-sm font-semibold text-ink-900">画像を即時削除</p>
              <span class="rounded-full bg-ink-900 px-2.5 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-white">推奨</span>
            </div>
            <p class="mt-2 text-sm text-ink-600">
              分析後に画像と関連データを削除し、CSV にはテキストだけを残します。
            </p>
          </div>
          <input class="h-5 w-5 accent-brass-600" type="checkbox" bind:checked={config.auto_delete_images} />
        </label>

        <label class="flex items-center justify-between rounded-2xl border border-ink-100 px-4 py-4">
          <div>
            <p class="text-sm font-medium text-ink-700">自動バッチを有効化</p>
            <p class="mt-1 text-sm text-ink-500">
              指定した時刻になると、未処理の記録から説明文を自動生成します。
            </p>
          </div>
          <input class="h-5 w-5 accent-brass-600" type="checkbox" bind:checked={config.scheduler_enabled} />
        </label>

        <div class="grid gap-4 sm:grid-cols-2">
          <div>
            <label class="text-sm font-medium text-ink-700" for="batch-time-lunch">昼休み前</label>
            <input
              id="batch-time-lunch"
              class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300 disabled:cursor-not-allowed disabled:opacity-50"
              type="time"
              value={config.batch_times[0] ?? "12:00"}
              oninput={(event) => updateBatchTime(0, (event.currentTarget as HTMLInputElement).value)}
              disabled={!config.scheduler_enabled}
            />
          </div>
          <div>
            <label class="text-sm font-medium text-ink-700" for="batch-time-evening">定時前</label>
            <input
              id="batch-time-evening"
              class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300 disabled:cursor-not-allowed disabled:opacity-50"
              type="time"
              value={config.batch_times[1] ?? "17:30"}
              oninput={(event) => updateBatchTime(1, (event.currentTarget as HTMLInputElement).value)}
              disabled={!config.scheduler_enabled}
            />
          </div>
        </div>
      </div>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">分析エンジンと保存先</p>
      <h3 class="mt-2 text-2xl font-bold text-ink-900">接続先と保存先</h3>

      <div class="mt-6 space-y-5">
        <div>
          <p class="text-sm font-medium text-ink-700">分析エンジン</p>
          <div class="mt-3 rounded-2xl border border-brass-300 bg-brass-50 px-4 py-4">
            <p class="text-sm font-semibold text-ink-900">Microsoft Copilot</p>
            <p class="mt-1 text-xs text-ink-500">このアプリは Copilot 専用構成です。Edge ブラウザ経由で説明文を生成します。</p>
          </div>
        </div>

        <div class="space-y-4">
          <div class="rounded-2xl border border-ink-100 bg-ink-50/70 px-4 py-4 text-sm leading-6 text-ink-600">
            <div class="flex flex-wrap items-center justify-between gap-3">
              <div>
                <p class="font-semibold text-ink-900">Copilot 接続ステータス</p>
                <p class="mt-1 text-sm text-ink-500">{copilotStatusMessage}</p>
              </div>
              <span
                class={`rounded-full px-3 py-1 text-xs font-semibold uppercase tracking-[0.18em] ${
                  copilotStatus === "connected"
                    ? "bg-emerald-100 text-emerald-700"
                    : copilotStatus === "login_required"
                      ? "bg-amber-100 text-amber-700"
                      : copilotStatus === "checking"
                        ? "bg-slate-200 text-slate-700"
                        : "bg-cinnabar-100 text-cinnabar-700"
                }`}
              >
                {copilotStatus === "connected"
                  ? "接続済み"
                  : copilotStatus === "login_required"
                    ? "ログイン必要"
                    : copilotStatus === "checking"
                      ? "確認中"
                      : "未接続"}
              </span>
            </div>

            {#if copilotStatusUrl}
              <p class="mt-3 text-xs text-ink-400 break-all">{copilotStatusUrl}</p>
            {/if}

            <div class="mt-4 flex flex-wrap gap-3">
              <button
                class="rounded-full border border-ink-200 bg-white px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-60"
                type="button"
                onclick={testCopilotConnection}
                disabled={!tauriAvailable || testing}
              >
                {testing ? "接続確認中..." : "接続テスト"}
              </button>
            </div>
          </div>

          <div>
            <label class="text-sm font-medium text-ink-700" for="vlm-max-tokens">説明文の最大長</label>
            <input
              id="vlm-max-tokens"
              class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
              type="number"
              min="64"
              max="2048"
              step="64"
              bind:value={config.vlm_max_tokens}
              oninput={refreshValidation}
              onblur={() => touchField("vlmMaxTokens")}
            />
            {#if touched.vlmMaxTokens && fieldErrors.vlmMaxTokens}
              <p class="mt-2 text-sm text-cinnabar-700">{fieldErrors.vlmMaxTokens}</p>
            {/if}
          </div>

          <details class="rounded-2xl border border-ink-100 bg-white px-4 py-4" bind:open={showCopilotAdvanced}>
            <summary class="cursor-pointer text-sm font-semibold text-ink-900">
              詳細設定と手動セットアップ手順
            </summary>
            <div class="mt-4 space-y-4">
              <div class="rounded-2xl border border-ink-100 bg-ink-50/70 px-4 py-4 text-sm leading-6 text-ink-600">
                <p class="font-semibold text-ink-900">手動セットアップが必要な場合</p>
                <ol class="mt-2 list-decimal space-y-1 pl-5">
                  <li>Edge を起動し、Microsoft 365 にログイン</li>
                  <li>Copilot 画面が開かれない場合は接続テストを再実行</li>
                  <li>CDP ポートを変更している場合のみ下の詳細設定を調整</li>
                </ol>
              </div>

              <div>
                <label class="text-sm font-medium text-ink-700" for="edge-cdp-port"
                  >Edge CDP ポート</label
                >
                <input
                  id="edge-cdp-port"
                  class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
                  type="number"
                  min="1024"
                  max="65535"
                  bind:value={config.edge_cdp_port}
                />
                <p class="mt-2 text-xs text-ink-400">通常は変更不要です（既定: 9222）</p>
              </div>
            </div>
          </details>

          <div class="space-y-4 rounded-2xl border border-ink-100 bg-white px-4 py-4">
            <div class="flex items-center justify-between gap-4">
              <div>
                <p class="text-sm font-semibold text-ink-900">セッション処理</p>
                <p class="mt-0.5 text-xs text-ink-500">
                  複数フレームを結合して Copilot に送信します。
                </p>
              </div>
              <input
                type="checkbox"
                bind:checked={config.session_enabled}
                class="h-4 w-4 rounded border-ink-300 text-brass-600 accent-brass-600"
              />
            </div>

            {#if config.session_enabled}
              <div class="space-y-4 border-t border-ink-100 pt-4">
                <div>
                  <div class="mb-1 flex items-center justify-between gap-3">
                    <label class="text-xs font-medium text-ink-700" for="session-gap-secs"
                      >セッション区切り（無操作）</label
                    >
                    <span class="text-xs text-ink-500">{Math.round(config.session_gap_secs / 60)} 分</span>
                  </div>
                  <input
                    id="session-gap-secs"
                    type="range"
                    min="120"
                    max="1800"
                    step="60"
                    bind:value={config.session_gap_secs}
                    class="w-full accent-brass-600"
                  />
                  <div class="mt-0.5 flex justify-between text-xs text-ink-400">
                    <span>2分</span>
                    <span>30分</span>
                  </div>
                </div>

                <div>
                  <div class="mb-1 flex items-center justify-between gap-3">
                    <label class="text-xs font-medium text-ink-700" for="session-window-secs"
                      >セッション最大長</label
                    >
                    <span class="text-xs text-ink-500">{Math.round(config.session_window_secs / 60)} 分</span>
                  </div>
                  <input
                    id="session-window-secs"
                    type="range"
                    min="60"
                    max="900"
                    step="60"
                    bind:value={config.session_window_secs}
                    class="w-full accent-brass-600"
                  />
                  <div class="mt-0.5 flex justify-between text-xs text-ink-400">
                    <span>1分</span>
                    <span>15分</span>
                  </div>
                </div>

                <div>
                  <div class="mb-1 flex items-center justify-between gap-3">
                    <label
                      class="text-xs font-medium text-ink-700"
                      for="max-frames-per-collage"
                      >コラージュ最大フレーム数</label
                    >
                    <span class="text-xs text-ink-500">{config.max_frames_per_collage} 枚</span>
                  </div>
                  <input
                    id="max-frames-per-collage"
                    type="range"
                    min="2"
                    max="6"
                    step="1"
                    bind:value={config.max_frames_per_collage}
                    class="w-full accent-brass-600"
                  />
                  <div class="mt-0.5 flex justify-between text-xs text-ink-400">
                    <span>2</span>
                    <span>6</span>
                  </div>
                </div>

                <div>
                  <label
                    class="mb-1 block text-xs font-medium text-ink-700"
                    for="session-user-prompt"
                  >
                    セッション用プロンプト
                  </label>
                  <textarea
                    id="session-user-prompt"
                    bind:value={config.session_user_prompt}
                    rows="4"
                    class="w-full rounded-2xl border border-ink-100 bg-white px-3 py-2 text-xs leading-6 text-ink-700 outline-none transition focus:border-brass-300"
                  ></textarea>
                  <p class="mt-1 text-xs text-ink-400">
                    プレースホルダ:
                    <span>{"{start_time}"}</span>
                    <span>{" {end_time}"}</span>
                    <span>{" {duration_min}"}</span>
                    <span>{" {frame_count}"}</span>
                  </p>
                </div>
              </div>
            {/if}
          </div>
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

        <div class="rounded-[1.5rem] border border-ink-100 bg-ink-50/70 px-4 py-4">
          <div class="flex items-start justify-between gap-4">
            <div>
              <p class="text-sm font-medium text-ink-700">生成プロンプトの調整</p>
              <p class="mt-1 text-sm leading-6 text-ink-500">
                経理業務向けの既定プロンプトをベースに、説明文の粒度や表現を調整できます。
              </p>
            </div>
          </div>

          <div class="mt-5 space-y-4">
            <div>
              <label class="text-sm font-medium text-ink-700" for="system-prompt">システムプロンプト</label>
              <textarea
                id="system-prompt"
                class="mt-3 min-h-32 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm leading-6 text-ink-700 outline-none transition focus:border-brass-300"
                bind:value={config.system_prompt}
              ></textarea>
            </div>

            <div>
              <label class="text-sm font-medium text-ink-700" for="user-prompt">ユーザープロンプト</label>
              <textarea
                id="user-prompt"
                class="mt-3 min-h-28 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm leading-6 text-ink-700 outline-none transition focus:border-brass-300"
                bind:value={config.user_prompt}
              ></textarea>
            </div>
          </div>
        </div>

        <div class="rounded-[1.5rem] border border-ink-100 bg-white px-4 py-4">
          <div class="flex items-start justify-between gap-4">
            <div>
              <p class="text-sm font-medium text-ink-700">マスキングルール</p>
              <p class="mt-1 text-sm leading-6 text-ink-500">
                CSV エクスポート時に、取引先名や金額などの表現を自動置換します。
              </p>
            </div>
            <button
              class="rounded-full border border-ink-200 bg-white px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700"
              type="button"
              onclick={addMaskRule}
            >
              ルールを追加
            </button>
          </div>

          <div class="mt-4 space-y-3">
            {#if config.mask_rules.length === 0}
              <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-6 text-sm text-ink-400">
                まだマスキングルールはありません。必要な場合のみ追加してください。
              </div>
            {:else}
              {#each config.mask_rules as rule, index}
                <div class="rounded-2xl border border-ink-100 bg-ink-50/70 px-4 py-4">
                  <div class="grid gap-3 lg:grid-cols-[1.1fr_0.9fr_auto]">
                    <input
                      class="rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
                      type="text"
                      value={rule.pattern}
                      placeholder="例: 株式会社A または \\b\\d{3},\\d{3}\\b"
                      oninput={(event) =>
                        updateMaskRule(index, {
                          ...rule,
                          pattern: (event.currentTarget as HTMLInputElement).value
                        })}
                    />
                    <input
                      class="rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
                      type="text"
                      value={rule.replacement}
                      placeholder="[MASKED]"
                      oninput={(event) =>
                        updateMaskRule(index, {
                          ...rule,
                          replacement: (event.currentTarget as HTMLInputElement).value
                        })}
                    />
                    <button
                      class="rounded-full border border-cinnabar-200 bg-cinnabar-50 px-4 py-3 text-sm font-semibold text-cinnabar-700 transition hover:bg-cinnabar-100"
                      type="button"
                      onclick={() => removeMaskRule(index)}
                    >
                      削除
                    </button>
                  </div>

                  <label class="mt-3 flex items-center gap-3 text-sm text-ink-600">
                    <input
                      class="h-4 w-4 accent-brass-600"
                      type="checkbox"
                      checked={rule.is_regex}
                      onchange={(event) =>
                        updateMaskRule(index, {
                          ...rule,
                          is_regex: (event.currentTarget as HTMLInputElement).checked
                        })}
                    />
                    正規表現として扱う
                  </label>
                </div>
              {/each}
            {/if}
          </div>

          <div class="mt-4 rounded-2xl border border-ink-100 bg-ink-50/70 px-4 py-4">
            <label class="text-sm font-medium text-ink-700" for="mask-preview">置換テスト</label>
            <textarea
              id="mask-preview"
              class="mt-3 min-h-24 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm leading-6 text-ink-700 outline-none transition focus:border-brass-300"
              bind:value={maskPreviewInput}
            ></textarea>
            <div class="mt-3 rounded-2xl border border-dashed border-brass-200 bg-brass-50/60 px-4 py-4 text-sm leading-6 text-ink-700">
              {maskPreview.text}
            </div>
            {#if maskPreview.error}
              <p class="mt-2 text-sm leading-6 text-brass-700">{maskPreview.error}</p>
            {/if}
          </div>
        </div>

        <div class="rounded-2xl border border-ink-100 bg-ink-50/70 px-4 py-4">
          <div class="flex flex-wrap gap-3">
            <button
              class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
              onclick={saveConfig}
              disabled={!tauriAvailable || saving || hasErrors}
            >
              {saving ? "保存中..." : "設定を保存"}
            </button>
          </div>
          <p class="mt-4 text-sm leading-6 text-ink-500">`Ctrl+S` でも設定を保存できます。</p>
        </div>
      </div>
    </article>
  </div>
</section>
