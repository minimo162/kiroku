<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import type { SetupStatus } from "$lib/types/setup";

  type CopilotConnectionStatus = {
    connected: boolean;
    login_required: boolean;
    url?: string | null;
    error?: string | null;
  };

  type Step = "welcome" | "edge-setup" | "test-capture" | "complete";

  const emptyStatus: SetupStatus = {
    setup_complete: false,
    engine_ready: false,
    node_available: false,
    copilot_server_available: false,
    edge_debugging_ready: false,
    edge_debugging_url: "http://127.0.0.1:9222/json/version"
  };

  let currentStep = $state<Step>("welcome");
  let status = $state<SetupStatus>({ ...emptyStatus });
  let loading = $state(true);
  let completing = $state(false);
  let launchingEdge = $state(false);
  let edgePollInterval = $state<number | null>(null);
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
    if (status.setup_complete) {
      currentStep = "complete";
    } else if (status.edge_debugging_ready) {
      currentStep = "test-capture";
    } else {
      currentStep = "edge-setup";
    }
    loading = false;
  }

  async function launchAndWaitForEdge() {
    if (!isTauri()) return;
    if (edgePollInterval !== null) {
      window.clearInterval(edgePollInterval);
      edgePollInterval = null;
    }

    launchingEdge = true;
    message = "Edge を起動しています...";

    try {
      const connectionStatus = await invoke<CopilotConnectionStatus>("check_copilot_connection");
      if (connectionStatus.connected) {
        message = "Copilot に接続しました。";
        await loadStatus();
      } else if (connectionStatus.login_required) {
        message = "Edge に接続しました。M365 Copilot にログインしてください...";
        await loadStatus();
        // ログイン完了を待つポーリングを開始
        startLoginPoll();
      } else {
        message = "接続を待機しています...";
        startConnectionPoll();
      }
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      launchingEdge = false;
    }
  }

  function startLoginPoll() {
    if (edgePollInterval !== null) return;
    edgePollInterval = window.setInterval(async () => {
      try {
        const s = await invoke<CopilotConnectionStatus>("check_copilot_connection");
        if (s.connected) {
          if (edgePollInterval !== null) {
            window.clearInterval(edgePollInterval);
            edgePollInterval = null;
          }
          message = "Copilot に接続しました。";
          await loadStatus();
        }
      } catch {
        // ポーリング中の一時的な失敗は無視する。
      }
    }, 3000);
  }

  function startConnectionPoll() {
    edgePollInterval = window.setInterval(async () => {
      try {
        const s = await invoke<CopilotConnectionStatus>("check_copilot_connection");
        if (s.connected) {
          if (edgePollInterval !== null) {
            window.clearInterval(edgePollInterval);
            edgePollInterval = null;
          }
          message = "Copilot に接続しました。";
          await loadStatus();
        } else if (s.login_required) {
          if (edgePollInterval !== null) {
            window.clearInterval(edgePollInterval);
            edgePollInterval = null;
          }
          message = "Edge に接続しました。M365 Copilot にログインしてください...";
          await loadStatus();
          startLoginPoll();
        }
      } catch {
        // ポーリング中の一時的な失敗は無視する。
      }
    }, 2000);
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
    }
  });

  onDestroy(() => {
    if (edgePollInterval !== null) {
      window.clearInterval(edgePollInterval);
    }
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
          セットアップ
        </div>
        <h1 class="text-4xl font-bold text-ink-900">Copilot 連携を準備します</h1>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          Kiroku は Microsoft Copilot を分析エンジンとして使用します。Edge のリモートデバッグ起動と M365 ログインだけ確認すれば利用できます。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">現在の状態</p>
        <div class="mt-4 grid gap-3 sm:grid-cols-2">
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">ブリッジ</p>
            <p class="mt-2 text-lg font-semibold">{status.engine_ready ? "準備完了" : "確認待ち"}</p>
          </div>
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">Edge デバッグ</p>
            <p class="mt-2 text-lg font-semibold">
              {status.edge_debugging_ready ? "接続可能" : "未接続"}
            </p>
          </div>
        </div>
        <p class="mt-4 text-sm leading-7 text-white/75">
          確認先: <span class="font-medium text-white">{status.edge_debugging_url}</span>
        </p>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[0.8fr_1.2fr]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">手順</p>
      <div class="mt-5 space-y-3">
        {#each [
          { id: "welcome", title: "1. 連携概要" },
          { id: "edge-setup", title: "2. Edge 準備" },
          { id: "test-capture", title: "3. 動作確認" },
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
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">ようこそ</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">ローカルモデルは不要です</h2>
          </div>
          <p class="text-sm leading-7 text-ink-500">
            この構成では追加のローカル推論サーバーは不要です。Node.js 同梱の Copilot ブリッジが、ログイン済みの Edge に接続して分析を行います。
          </p>
          <div class="flex flex-wrap gap-3">
            <button
              class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700"
              onclick={() => {
                currentStep = "edge-setup";
              }}
            >
              次へ進む
            </button>
          </div>
        </div>
      {:else if currentStep === "edge-setup"}
        <div class="space-y-5">
          <div>
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Edge 準備</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">Edge をデバッグ起動します</h2>
          </div>
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
        </div>
      {:else if currentStep === "test-capture"}
        <div class="space-y-5">
          <div>
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">動作確認</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">キャプチャ動作を確認します</h2>
          </div>
          <p class="text-sm leading-7 text-ink-500">
            最後にテストキャプチャを実行して、Kiroku の基本動作を確認します。Copilot の説明文生成はダッシュボードからバッチ実行時に確認できます。
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
            <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">完了</p>
            <h2 class="mt-2 text-3xl font-bold text-ink-900">Copilot 構成で開始します</h2>
          </div>
          <div class="rounded-[1.5rem] border border-ink-100 bg-ink-50/70 px-5 py-5 text-sm leading-7 text-ink-600">
            <p><span class="font-semibold text-ink-900">Node.js:</span> {status.node_available ? " 検出済み" : " 未検出"}</p>
            <p><span class="font-semibold text-ink-900">Copilot ブリッジ:</span> {status.copilot_server_available ? " 配置済み" : " 未配置"}</p>
            <p><span class="font-semibold text-ink-900">Edge デバッグ:</span> {status.edge_debugging_ready ? " 接続可能" : " 未接続"}</p>
          </div>
          <button
            class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
            onclick={finishSetup}
            disabled={!tauriAvailable || completing || !status.engine_ready}
          >
            {completing ? "保存中..." : "セットアップ完了"}
          </button>
        </div>
      {/if}

      {#if message && currentStep !== "edge-setup"}
        <div class="mt-5 rounded-[1.25rem] border border-brass-100 bg-brass-50 px-4 py-3 text-sm text-brass-800">
          {message}
        </div>
      {/if}
    </article>
  </div>
</section>
