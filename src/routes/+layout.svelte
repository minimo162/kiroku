<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { page } from "$app/stores";
  import type { Snippet } from "svelte";
  import { addToast } from "$lib/toast.svelte";
  import ToastContainer from "$lib/components/ToastContainer.svelte";
  import type { DashboardSnapshot } from "$lib/types/dashboard";

  let { children }: { children: Snippet } = $props();

  type SetupStatus = {
    setup_complete: boolean;
  };

  const navItems = [
    { href: "/dashboard", label: "ダッシュボード", status: "live" },
    { href: "/history", label: "履歴", status: "live" },
    { href: "/settings", label: "設定", status: "live" },
    { href: "/preview", label: "プレビュー", status: "live" },
    { href: "/export", label: "エクスポート", status: "live" }
  ];

  let checkingSetup = $state(true);
  let tauriAvailable = $state(false);
  let isRecording = $state(false);
  let nextBatchAt = $state<string | null>(null);

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  const isSetupRoute = () => $page.url.pathname.startsWith("/setup");

  function formatNextBatch(iso: string | null): string {
    if (!iso) return "";
    const date = new Date(iso);
    if (Number.isNaN(date.valueOf())) return iso;
    return new Intl.DateTimeFormat("ja-JP", {
      hour: "2-digit",
      minute: "2-digit"
    }).format(date);
  }

  onMount(() => {
    tauriAvailable = isTauri();
    if (!tauriAvailable) {
      checkingSetup = false;
      return;
    }

    let disposed = false;
    const unlisteners: Array<() => void> = [];

    void invoke<DashboardSnapshot>("get_dashboard_snapshot")
      .then((snap) => {
        if (!disposed) {
          isRecording = snap.stats.is_recording;
          nextBatchAt = snap.stats.next_batch_run_at;
        }
      })
      .catch(() => {});

    void (async () => {
      try {
        const status = await invoke<SetupStatus>("get_setup_status");
        if (!disposed && !status.setup_complete && !isSetupRoute()) {
          await goto("/setup");
        } else if (!disposed && status.setup_complete && isSetupRoute()) {
          await goto("/dashboard");
        }
      } finally {
        if (!disposed) {
          checkingSetup = false;
        }
      }
    })();

    void (async () => {
      const recordingUnlisten = await listen<boolean>("recording-status", (event) => {
        if (!disposed) isRecording = event.payload;
      });
      unlisteners.push(recordingUnlisten);

      const schedulerUnlisten = await listen<string | null>("scheduler-status", (event) => {
        if (!disposed) nextBatchAt = event.payload;
      });
      unlisteners.push(schedulerUnlisten);

      const loginRequiredUnlisten = await listen<string>("copilot-login-required", async (event) => {
        if (disposed) {
          return;
        }

        addToast("info", event.payload || "Copilot にログインしてください。Edge の画面を確認してください。");
        await invoke("pause_vlm_batch").catch(() => false);
      });
      unlisteners.push(loginRequiredUnlisten);
    })();

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  });
</script>

{#if checkingSetup && tauriAvailable}
  <div class="flex min-h-screen items-center justify-center px-4 py-8 text-ink-900">
    <div class="w-full max-w-lg rounded-[2rem] border border-white/70 bg-white/85 px-8 py-8 text-center shadow-panel backdrop-blur">
      <p class="text-xs font-semibold uppercase tracking-[0.3em] text-ink-400">Kiroku</p>
      <h1 class="mt-4 text-3xl font-bold text-ink-900">セットアップ状態を確認中</h1>
      <p class="mt-4 text-sm leading-7 text-ink-500">
        モデル配置と初回セットアップの状態を読み込んでいます。
      </p>
    </div>
  </div>
{:else if isSetupRoute()}
  <main class="min-h-screen px-4 py-4 text-ink-900 sm:px-6 lg:px-8">
    <div class="mx-auto max-w-6xl">
      {@render children()}
    </div>
  </main>
  <ToastContainer />
{:else}
  <div class="min-h-screen px-4 py-4 text-ink-900 sm:px-6 lg:px-8">
    <div class="mx-auto grid min-h-[calc(100vh-2rem)] max-w-7xl gap-4 lg:grid-cols-[280px_minmax(0,1fr)]">
      <aside class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
        <div class="flex h-full flex-col px-5 py-6">
          <div class="rounded-[1.5rem] bg-ink-900 px-5 py-5 text-white">
            <p class="text-xs font-semibold uppercase tracking-[0.32em] text-white/55">Kiroku</p>
            <h1 class="mt-3 text-2xl font-bold">業務記録ダッシュボード</h1>
            <p class="mt-3 text-sm leading-6 text-white/75">
              キャプチャ状況と説明文の生成状況を一つの画面で確認します。
            </p>
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
          </div>

          <nav class="mt-6 space-y-2">
            {#each navItems as item}
              <a
                href={item.href}
                class={`flex items-center gap-2 rounded-2xl border px-4 py-3 text-sm font-medium transition ${
                  $page.url.pathname === item.href
                    ? "border-brass-200 bg-brass-50 text-brass-700"
                    : "border-transparent bg-transparent text-ink-600 hover:border-ink-100 hover:bg-ink-50/80 hover:text-ink-900"
                }`}
                aria-disabled={item.status !== "live"}
                onclick={(event) => {
                  if (item.status !== "live") {
                    event.preventDefault();
                  }
                }}
              >
                <span>{item.label}</span>
              </a>
            {/each}
          </nav>

          <div class="mt-auto rounded-[1.5rem] border border-ink-100 bg-ink-50/80 px-4 py-4">
            <p class="text-xs font-semibold uppercase tracking-[0.24em] text-ink-400">ショートカット</p>
            <p class="mt-3 text-sm leading-6 text-ink-600">
              `Ctrl+Shift+K` で記録開始/停止。
              ウィンドウを閉じてもトレイに常駐します。
            </p>
            {#if nextBatchAt}
              <p class="mt-3 text-xs font-medium text-ink-500">
                次回バッチ: <span class="font-semibold text-ink-700">{formatNextBatch(nextBatchAt)}</span>
              </p>
            {/if}
          </div>
        </div>
      </aside>

      <main class="min-w-0">
        {@render children()}
      </main>
    </div>
  </div>
  <ToastContainer />
{/if}
