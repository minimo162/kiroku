<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type {
    ModelDownloadProgress,
    ModelDownloadResult,
    SetupStatus
  } from "$lib/types/setup";

  type Step = "welcome" | "model-download" | "test-capture" | "complete";

  const emptyStatus: SetupStatus = {
    setup_complete: false,
    model_ready: false,
    llama_server_available: false,
    models_dir: "",
    model_path: null,
    mmproj_path: null
  };

  let currentStep = $state<Step>("welcome");
  let status = $state<SetupStatus>({ ...emptyStatus });
  let downloadProgress = $state<ModelDownloadProgress | null>(null);
  let loading = $state(true);
  let downloading = $state(false);
  let completing = $state(false);
  let testingCapture = $state(false);
  let message = $state<string | null>(null);
  let testCaptureMessage = $state<string | null>(null);
  let tauriAvailable = $state(false);

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  async function loadStatus() {
    if (!isTauri()) {
      loading = false;
      return;
    }

    status = await invoke<SetupStatus>("get_setup_status");
    if (status.model_ready) {
      currentStep = "test-capture";
    }
    if (status.setup_complete) {
      currentStep = "complete";
    }
    loading = false;
  }

  async function startModelDownload() {
    if (!isTauri()) return;

    downloading = true;
    message = null;
    currentStep = "model-download";
    try {
      await invoke<ModelDownloadResult>("download_model");
      await loadStatus();
      currentStep = "test-capture";
      message = "モデルファイルの準備が完了しました。";
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      downloading = false;
    }
  }

  async function runTestCapture() {
    if (!isTauri()) return;

    testingCapture = true;
    testCaptureMessage = null;
    try {
      await invoke("capture_now");
      testCaptureMessage = "テストキャプチャに成功しました。";
      currentStep = "complete";
    } catch (error) {
      testCaptureMessage = error instanceof Error ? error.message : String(error);
    } finally {
      testingCapture = false;
    }
  }

  async function finishSetup() {
    if (!isTauri()) return;

    completing = true;
    message = null;
    try {
      await invoke("complete_setup");
      await goto("/dashboard");
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      completing = false;
    }
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void loadStatus();

    if (!tauriAvailable) {
      message = "ブラウザプレビューでは初回セットアップを実行できません。";
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      unlisten = await listen<ModelDownloadProgress>("model-download-progress", (event) => {
        if (!disposed) {
          downloadProgress = event.payload;
        }
      });
    })();

    return () => {
      disposed = true;
      unlisten?.();
    };
  });
</script>

<svelte:head>
  <title>Kiroku | 初回セットアップ</title>
</svelte:head>

<section class="space-y-4">
  <div class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/85 shadow-panel backdrop-blur">
    <div class="grid gap-6 px-6 py-8 lg:grid-cols-[1.05fr_0.95fr] lg:px-8">
      <div class="space-y-4">
        <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
          Setup Wizard
        </div>
        <h1 class="text-4xl font-bold text-ink-900">Kiroku の初回セットアップ</h1>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          モデルファイルの準備、テストキャプチャ、完了フラグの保存を順に進めます。
          非エンジニアでも使い始められるよう、必要な手順だけを 3 ステップでまとめています。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">Current Status</p>
        <div class="mt-4 grid gap-3 sm:grid-cols-2">
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">モデル</p>
            <p class="mt-2 text-lg font-semibold">{status.model_ready ? "準備完了" : "未配置"}</p>
          </div>
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">llama-server</p>
            <p class="mt-2 text-lg font-semibold">{status.llama_server_available ? "検出済み" : "未検出"}</p>
          </div>
        </div>
        <p class="mt-4 text-sm leading-7 text-white/75">
          モデル保存先: <span class="font-medium text-white">{status.models_dir || "未解決"}</span>
        </p>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[0.8fr_1.2fr]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Steps</p>
      <div class="mt-5 space-y-3">
        {#each [
          { id: "welcome", title: "1. モデル準備" },
          { id: "model-download", title: "2. ダウンロード進捗" },
          { id: "test-capture", title: "3. テストキャプチャ" },
          { id: "complete", title: "4. 完了" }
        ] as step}
          <div
            class={`rounded-[1.35rem] border px-4 py-4 ${
              currentStep === step.id
                ? "border-brass-200 bg-brass-50"
                : "border-ink-100 bg-ink-50/60"
            }`}
          >
            <p class="text-sm font-semibold text-ink-900">{step.title}</p>
          </div>
        {/each}
      </div>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      {#if loading}
        <div class="rounded-[1.5rem] border border-dashed border-ink-200 px-6 py-16 text-center text-sm text-ink-400">
          セットアップ情報を読み込んでいます。
        </div>
      {:else if currentStep === "welcome"}
        <div class="space-y-5">
          <div>
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Welcome</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">まずモデルを準備します</h2>
          </div>
          <p class="text-sm leading-7 text-ink-500">
            Qwen 系 GGUF モデルと `mmproj` を `models/` に配置します。すでに存在する場合は再ダウンロードしません。
          </p>
          <button
            class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
            onclick={startModelDownload}
            disabled={!tauriAvailable || downloading}
          >
            {downloading ? "準備中..." : "モデル準備を開始"}
          </button>
        </div>
      {:else if currentStep === "model-download"}
        <div class="space-y-5">
          <div>
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Download</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">モデルをダウンロード中</h2>
          </div>
          <div class="rounded-[1.5rem] border border-ink-100 bg-ink-50/70 px-5 py-5">
            <p class="text-sm font-semibold text-ink-900">{downloadProgress?.file_name ?? "接続待ち"}</p>
            <div class="mt-4 h-3 overflow-hidden rounded-full bg-ink-100">
              <div
                class="h-full rounded-full bg-gradient-to-r from-brass-500 to-cinnabar-500 transition-all duration-300"
                style={`width: ${downloadProgress?.percent ?? 0}%`}
              ></div>
            </div>
            <div class="mt-4 grid gap-3 sm:grid-cols-3">
              <div class="rounded-2xl border border-white bg-white px-4 py-4">
                <p class="text-xs uppercase tracking-[0.2em] text-ink-400">進捗</p>
                <p class="mt-2 text-xl font-semibold text-ink-900">
                  {downloadProgress ? `${Math.round(downloadProgress.percent)}%` : "-"}
                </p>
              </div>
              <div class="rounded-2xl border border-white bg-white px-4 py-4">
                <p class="text-xs uppercase tracking-[0.2em] text-ink-400">速度</p>
                <p class="mt-2 text-sm font-semibold text-ink-900">{downloadProgress?.speed ?? "-"}</p>
              </div>
              <div class="rounded-2xl border border-white bg-white px-4 py-4">
                <p class="text-xs uppercase tracking-[0.2em] text-ink-400">残り</p>
                <p class="mt-2 text-sm font-semibold text-ink-900">{downloadProgress?.remaining ?? "計算中"}</p>
              </div>
            </div>
          </div>
          <button
            class="rounded-full border border-ink-200 bg-white px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-60"
            onclick={startModelDownload}
            disabled={!tauriAvailable || downloading}
          >
            {downloading ? "ダウンロード中..." : "再試行"}
          </button>
        </div>
      {:else if currentStep === "test-capture"}
        <div class="space-y-5">
          <div>
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Test Capture</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">動作確認を行います</h2>
          </div>
          <p class="text-sm leading-7 text-ink-500">
            モデル準備が終わったので、最後にテストキャプチャを実行して記録経路が動くか確認します。
          </p>
          {#if testCaptureMessage}
            <div class="rounded-[1.25rem] border border-brass-100 bg-brass-50 px-4 py-3 text-sm text-brass-800">
              {testCaptureMessage}
            </div>
          {/if}
          <div class="flex flex-wrap gap-3">
            <button
              class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
              onclick={runTestCapture}
              disabled={!tauriAvailable || testingCapture}
            >
              {testingCapture ? "確認中..." : "テストキャプチャを実行"}
            </button>
            <button
              class="rounded-full border border-ink-200 bg-white px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700"
              onclick={() => {
                currentStep = "complete";
              }}
            >
              スキップして続行
            </button>
          </div>
        </div>
      {:else}
        <div class="space-y-5">
          <div>
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Complete</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">セットアップを完了します</h2>
          </div>
          <p class="text-sm leading-7 text-ink-500">
            ここで完了フラグを保存すると、次回起動時から通常のダッシュボードへ直接入ります。
          </p>
          <div class="rounded-[1.5rem] border border-ink-100 bg-ink-50/70 px-5 py-5 text-sm leading-7 text-ink-600">
            <p><span class="font-semibold text-ink-900">モデル:</span> {status.model_path ?? "未配置"}</p>
            <p><span class="font-semibold text-ink-900">mmproj:</span> {status.mmproj_path ?? "未配置"}</p>
            <p>
              <span class="font-semibold text-ink-900">llama-server:</span>
              {status.llama_server_available ? " 検出済み" : " 未検出"}
            </p>
          </div>
          <button
            class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
            onclick={finishSetup}
            disabled={!tauriAvailable || completing || !status.model_ready}
          >
            {completing ? "保存中..." : "セットアップ完了"}
          </button>
        </div>
      {/if}

      {#if message}
        <div class="mt-5 rounded-[1.25rem] border border-brass-100 bg-brass-50 px-4 py-3 text-sm text-brass-800">
          {message}
        </div>
      {/if}
    </article>
  </div>
</section>
