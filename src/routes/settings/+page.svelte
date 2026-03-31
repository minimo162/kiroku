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
    scheduler_enabled: boolean;
    setup_complete: boolean;
    batch_time: string;
    vlm_host: string;
    vlm_max_tokens: number;
    data_dir: string;
    system_prompt: string;
    user_prompt: string;
    mask_rules: MaskRule[];
  };

  const createMaskRule = (): MaskRule => ({
    pattern: "",
    replacement: "[MASKED]",
    is_regex: false
  });

  const defaultConfig: AppConfig = {
    capture_interval_secs: 30,
    dhash_threshold: 10,
    auto_delete_images: true,
    scheduler_enabled: true,
    setup_complete: false,
    batch_time: "22:00",
    vlm_host: "127.0.0.1:8080",
    vlm_max_tokens: 256,
    data_dir: "",
    system_prompt:
      "あなたは経理部門向けの業務記録アシスタントです。画面上で確認できる事実を優先し、日本語で簡潔に記述してください。SAP GUI、Excel、Outlook、Teams などの画面を対象とし、連結PKG、内部取引消去、UPI、月次決算、メール確認、会議参加などの業務文脈が明確な場合のみ用語を使ってください。推測は控えめにし、不確実な場合は一般的な表現に留めてください。",
    user_prompt:
      "このスクリーンショットに写っている業務操作を1から3文で説明してください。必ず次の観点を含めてください: 使用中のアプリケーション、実行している操作、表示されているデータや対象。出力は自然な日本語の文章のみとし、箇条書きやJSONは使わないでください。",
    mask_rules: []
  };

  let config = $state<AppConfig>({ ...defaultConfig });
  let loading = $state(true);
  let saving = $state(false);
  let testing = $state(false);
  let selectingFolder = $state(false);
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
    fieldErrors = {
      vlmHost: validateVlmHost(config.vlm_host),
      vlmMaxTokens: validateMaxTokens(config.vlm_max_tokens)
    };
  }

  function touchField(field: keyof typeof touched) {
    touched = { ...touched, [field]: true };
    refreshValidation();
  }

  function currentValidationErrors() {
    return {
      vlmHost: validateVlmHost(config.vlm_host),
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

  async function testConnection() {
    if (!isTauri()) return;
    const hostError = validateVlmHost(config.vlm_host);
    fieldErrors = {
      ...fieldErrors,
      vlmHost: hostError
    };
    touched = {
      ...touched,
      vlmHost: true
    };
    if (hostError) {
      addToast("error", hostError);
      return;
    }

    testing = true;
    try {
      const ok = await invoke<boolean>("test_vlm_connection", { vlmHost: config.vlm_host });
      addToast(
        ok ? "success" : "info",
        ok
          ? "分析エンジンへの接続に成功しました。"
          : "分析エンジンは応答しましたが、正常ステータスではありません。"
      );
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
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
      addToast("info", "ブラウザプレビューでは設定の保存や接続テストを実行できません。");
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
          <p>キャプチャ間隔は 10 秒から 300 秒の範囲で調整できます。</p>
          <p>検出感度を上げるほど、近い画面変化をスキップしやすくなります。</p>
          <p>夜間バッチを有効にすると、指定時刻に未処理フレームの説明文を自動生成します。</p>
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
            min="10"
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
            <p class="text-sm font-medium text-ink-700">夜間バッチを有効化</p>
            <p class="mt-1 text-sm text-ink-500">
              指定時刻になると、未処理の記録から説明文を自動生成します。
            </p>
          </div>
          <input class="h-5 w-5 accent-brass-600" type="checkbox" bind:checked={config.scheduler_enabled} />
        </label>

        <div>
          <label class="text-sm font-medium text-ink-700" for="batch-time">バッチ開始時刻</label>
          <input
            id="batch-time"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300 disabled:cursor-not-allowed disabled:opacity-50"
            type="time"
            bind:value={config.batch_time}
            disabled={!config.scheduler_enabled}
          />
        </div>
      </div>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">分析エンジンと保存先</p>
      <h3 class="mt-2 text-2xl font-bold text-ink-900">接続先と保存先</h3>

      <div class="mt-6 space-y-5">
        <div>
          <label class="text-sm font-medium text-ink-700" for="vlm-host">分析エンジンのアドレス</label>
          <input
            id="vlm-host"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="text"
            bind:value={config.vlm_host}
            placeholder="127.0.0.1:8080"
            oninput={refreshValidation}
            onblur={() => touchField("vlmHost")}
          />
          {#if touched.vlmHost && fieldErrors.vlmHost}
            <p class="mt-2 text-sm text-cinnabar-700">{fieldErrors.vlmHost}</p>
          {/if}
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
            <button
              class="rounded-full border border-ink-200 bg-white px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-60"
              onclick={testConnection}
              disabled={!tauriAvailable || testing}
            >
              {testing ? "確認中..." : "接続を確認"}
            </button>
          </div>
          <p class="mt-4 text-sm leading-6 text-ink-500">`Ctrl+S` でも設定を保存できます。</p>
        </div>
      </div>
    </article>
  </div>
</section>
