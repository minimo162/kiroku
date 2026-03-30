<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import StatusCard from "$lib/components/dashboard/StatusCard.svelte";
  import { addToast } from "$lib/toast.svelte";
  import type {
    DashboardSnapshot,
    DashboardStat,
    DashboardStatsPayload,
    RecentCapture,
    VlmProgressPayload
  } from "$lib/types/dashboard";

  const defaultStats: DashboardStatsPayload = {
    total_captures: 0,
    effective_captures: 0,
    skipped_captures: 0,
    vlm_processed: 0,
    scheduler_enabled: true,
    is_recording: false,
    server_running: false,
    batch_running: false,
    next_batch_run_at: null,
    last_capture_at: null,
    last_error: null
  };

  const defaultProgress: VlmProgressPayload = {
    total: 0,
    completed: 0,
    failed: 0,
    current_id: null,
    estimated_remaining_secs: null
  };

  let stats = $state<DashboardStatsPayload>({ ...defaultStats });
  let vlmProgress = $state<VlmProgressPayload>({ ...defaultProgress });
  let recentCaptures = $state<RecentCapture[]>([]);
  let loading = $state(true);
  let actionPending = $state<"recording" | "batch" | null>(null);
  let stopConfirmOpen = $state(false);
  let tauriAvailable = $state(false);

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  const formatDateTime = (value: string | null) => {
    if (!value) return "まだ記録されていません";
    const date = new Date(value);
    if (Number.isNaN(date.valueOf())) return value;

    return new Intl.DateTimeFormat("ja-JP", {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit"
    }).format(date);
  };

  const formatSchedule = (value: string | null) => {
    if (!value) return "無効";
    return formatDateTime(value);
  };

  const formatDuration = (value: number | null) => {
    if (value === null) return "推定中";
    if (value < 60) return `約${value}秒`;
    const minutes = Math.floor(value / 60);
    const seconds = value % 60;
    return seconds === 0 ? `約${minutes}分` : `約${minutes}分${seconds}秒`;
  };

  const progressPercent = () => {
    if (vlmProgress.total === 0) return 0;
    return Math.min(100, Math.round((vlmProgress.completed / vlmProgress.total) * 100));
  };

  const buildStatCards = (): DashboardStat[] => [
    {
      title: "本日のキャプチャ",
      value: String(stats.total_captures),
      unit: "枚",
      detail: stats.is_recording
        ? `記録中です。最終更新 ${formatDateTime(stats.last_capture_at)}`
        : `停止中です。最終更新 ${formatDateTime(stats.last_capture_at)}`,
      tone: "brass"
    },
    {
      title: "有効フレーム",
      value: String(stats.effective_captures),
      unit: "枚",
      detail: `差分スキップ ${stats.skipped_captures} 枚`,
      tone: "ink"
    },
    {
      title: "VLM 処理済み",
      value: String(stats.vlm_processed),
      unit: "枚",
      detail: stats.server_running
        ? stats.batch_running
          ? "バッチ処理が進行中です"
          : stats.scheduler_enabled
            ? `次回 ${formatSchedule(stats.next_batch_run_at)}`
            : "手動実行モードです"
        : stats.scheduler_enabled
          ? `次回 ${formatSchedule(stats.next_batch_run_at)}`
          : "VLM サーバー停止中です",
      tone: "brass"
    }
  ];

  async function refreshDashboard() {
    if (!isTauri()) {
      loading = false;
      return;
    }

    try {
      const snapshot = await invoke<DashboardSnapshot>("get_dashboard_snapshot");
      stats = snapshot.stats;
      vlmProgress = snapshot.vlm_progress;
      recentCaptures = snapshot.recent_captures;
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      loading = false;
    }
  }

  async function startRecording() {
    if (!isTauri()) return;

    actionPending = "recording";
    try {
      const started = await invoke<boolean>("start_recording");
      if (started) {
        addToast("success", "記録を開始しました。");
      } else {
        addToast("info", "すでに記録中です。");
      }
      await refreshDashboard();
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      actionPending = null;
    }
  }

  function requestStopRecording() {
    stopConfirmOpen = true;
  }

  async function confirmStopRecording() {
    if (!isTauri()) return;

    actionPending = "recording";
    try {
      const stopped = await invoke<boolean>("stop_recording");
      if (stopped) {
        addToast("success", "記録を停止しました。");
      } else {
        addToast("info", "現在は記録中ではありません。");
      }
      stopConfirmOpen = false;
      await refreshDashboard();
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      actionPending = null;
    }
  }

  async function runBatch() {
    if (!isTauri()) return;

    actionPending = "batch";
    try {
      const started = await invoke<boolean>("run_vlm_batch");
      if (started) {
        addToast("success", "VLM バッチを開始しました。");
      } else {
        addToast("info", "VLM バッチはすでに実行中です。");
      }
      await refreshDashboard();
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      actionPending = null;
    }
  }

  async function clearLastError() {
    if (!isTauri() || !stats.last_error) return;

    try {
      await invoke<boolean>("clear_last_error");
      stats = { ...stats, last_error: null };
      addToast("success", "最新エラーをクリアしました。");
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    }
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void refreshDashboard();

    if (!tauriAvailable) {
      addToast("info", "ブラウザプレビューでは Tauri コマンドを呼び出せません。");
      return;
    }

    let disposed = false;
    const unlisteners: Array<() => void> = [];

    const setupListeners = async () => {
      const captureUnlisten = await listen<RecentCapture>("capture-added", async () => {
        if (!disposed) {
          await refreshDashboard();
        }
      });
      const recordingUnlisten = await listen<boolean>("recording-status", (event) => {
        if (!disposed) {
          stats = { ...stats, is_recording: event.payload };
        }
      });
      const vlmProgressUnlisten = await listen<VlmProgressPayload>("vlm-progress", (event) => {
        if (!disposed) {
          vlmProgress = event.payload;
        }
      });
      const vlmStatusUnlisten = await listen("vlm-status", async () => {
        if (!disposed) {
          await refreshDashboard();
        }
      });
      const batchCompleteUnlisten = await listen("vlm-batch-complete", async () => {
        if (!disposed) {
          await refreshDashboard();
        }
      });
      const schedulerStatusUnlisten = await listen("scheduler-status", async () => {
        if (!disposed) {
          await refreshDashboard();
        }
      });
      const configUpdatedUnlisten = await listen("config-updated", async () => {
        if (!disposed) {
          await refreshDashboard();
        }
      });

      unlisteners.push(
        captureUnlisten,
        recordingUnlisten,
        vlmProgressUnlisten,
        vlmStatusUnlisten,
        batchCompleteUnlisten,
        schedulerStatusUnlisten,
        configUpdatedUnlisten
      );
    };

    void setupListeners();

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) {
        unlisten();
      }
    };
  });
</script>

<svelte:head>
  <title>Kiroku | ダッシュボード</title>
</svelte:head>

<section class="space-y-4">
  <div class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
    <div class="grid gap-6 px-6 py-6 lg:grid-cols-[1.2fr_0.8fr] lg:px-8">
      <div class="space-y-5">
        <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
          Live Monitor
        </div>
        <div class="space-y-3">
          <h2 class="text-3xl font-bold text-ink-900 sm:text-4xl">今日の記録状況を一目で確認</h2>
          <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
            キャプチャ枚数、VLM 進捗、最新フレームを同じ画面で追えます。
            トレイ常駐中でもこの画面に戻れば直近の状態を再取得します。
          </p>
        </div>

        <div class="flex flex-wrap gap-3">
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

          <button
            class="rounded-full border border-ink-200 bg-white px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-60"
            onclick={runBatch}
            disabled={!tauriAvailable || actionPending !== null || stats.batch_running}
          >
            {#if actionPending === "batch"}
              実行中...
            {:else if stats.batch_running}
              VLM 実行中
            {:else}
              VLM バッチ実行
            {/if}
          </button>
        </div>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">System State</p>
        <div class="mt-5 grid gap-3 sm:grid-cols-2">
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">記録状態</p>
            <p class="mt-2 text-lg font-semibold">{stats.is_recording ? "録画中" : "停止中"}</p>
          </div>
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">VLM サーバー</p>
            <p class="mt-2 text-lg font-semibold">{stats.server_running ? "起動中" : "停止中"}</p>
          </div>
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">次回バッチ</p>
            <p class="mt-2 text-sm font-medium">{formatSchedule(stats.next_batch_run_at)}</p>
          </div>
          <div class="rounded-2xl bg-white/8 px-4 py-4">
            <p class="text-xs uppercase tracking-[0.2em] text-white/50">最終キャプチャ</p>
            <p class="mt-2 text-sm font-medium">{formatDateTime(stats.last_capture_at)}</p>
          </div>
          <div class="rounded-2xl bg-white/8 px-4 py-4 sm:col-span-2">
            <div class="flex items-center justify-between gap-3">
              <p class="text-xs uppercase tracking-[0.2em] text-white/50">最新エラー</p>
              {#if stats.last_error}
                <button
                  class="rounded-full border border-white/10 px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.2em] text-white/80 transition hover:bg-white/10"
                  type="button"
                  onclick={clearLastError}
                >
                  ×
                </button>
              {/if}
            </div>
            <p class="mt-2 text-sm font-medium">{stats.last_error ?? "なし"}</p>
          </div>
        </div>
      </div>
    </div>
  </div>

  <section class="grid gap-4 md:grid-cols-3">
    {#each buildStatCards() as stat}
      <StatusCard {...stat} />
    {/each}
  </section>

  <section class="grid gap-4 xl:grid-cols-[0.95fr_1.05fr]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <div class="flex items-center justify-between gap-4">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">VLM Progress</p>
          <h3 class="mt-2 text-2xl font-bold text-ink-900">夜間バッチの進捗</h3>
        </div>
        <div class="rounded-full bg-ink-100 px-3 py-2 text-xs font-semibold uppercase tracking-[0.2em] text-ink-500">
          {progressPercent()}%
        </div>
      </div>

      <div class="mt-6 h-3 overflow-hidden rounded-full bg-ink-100">
        <div
          class="h-full rounded-full bg-gradient-to-r from-brass-500 to-cinnabar-500 transition-all duration-300"
          style={`width: ${progressPercent()}%`}
        ></div>
      </div>

      <div class="mt-5 grid gap-3 sm:grid-cols-3">
        <div class="rounded-2xl border border-ink-100 px-4 py-4">
          <p class="text-xs uppercase tracking-[0.2em] text-ink-400">対象</p>
          <p class="mt-2 text-xl font-semibold text-ink-900">{vlmProgress.total}</p>
        </div>
        <div class="rounded-2xl border border-ink-100 px-4 py-4">
          <p class="text-xs uppercase tracking-[0.2em] text-ink-400">完了</p>
          <p class="mt-2 text-xl font-semibold text-ink-900">{vlmProgress.completed}</p>
        </div>
        <div class="rounded-2xl border border-ink-100 px-4 py-4">
          <p class="text-xs uppercase tracking-[0.2em] text-ink-400">残り目安</p>
          <p class="mt-2 text-xl font-semibold text-ink-900">{formatDuration(vlmProgress.estimated_remaining_secs)}</p>
        </div>
      </div>

      <p class="mt-5 text-sm leading-6 text-ink-500">
        {#if vlmProgress.current_id}
          現在処理中: <span class="font-medium text-ink-700">{vlmProgress.current_id}</span>
        {:else if stats.batch_running}
          次のフレームを処理中です。
        {:else if stats.scheduler_enabled}
          次回バッチは <span class="font-medium text-ink-700">{formatSchedule(stats.next_batch_run_at)}</span> に予定されています。
        {:else if stats.server_running}
          VLM サーバーは起動済みです。バッチを開始できます。
        {:else}
          VLM サーバーが停止中のため、バッチは待機しています。
        {/if}
      </p>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <div class="flex items-center justify-between gap-4">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Recent Frames</p>
          <h3 class="mt-2 text-2xl font-bold text-ink-900">最新キャプチャ</h3>
        </div>
        <div class="rounded-full border border-ink-100 px-3 py-2 text-xs font-semibold text-ink-500">
          {recentCaptures.length} 件
        </div>
      </div>

      <div class="mt-5 space-y-3">
        {#if loading}
          <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-6 text-sm text-ink-400">
            ダッシュボードを読み込んでいます。
          </div>
        {:else if recentCaptures.length === 0}
          <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-6 text-sm text-ink-400">
            まだ有効フレームがありません。
          </div>
        {:else}
          {#each recentCaptures as capture}
            <div class="rounded-2xl border border-ink-100 px-4 py-4">
              <div class="flex flex-wrap items-center justify-between gap-3">
                <div>
                  <p class="text-sm font-semibold text-ink-900">{capture.app}</p>
                  <p class="mt-1 text-sm text-ink-500">{capture.window_title}</p>
                </div>
                <div class="text-right">
                  <p class="text-sm font-medium text-ink-700">{formatDateTime(capture.timestamp)}</p>
                  <p
                    class={`mt-1 text-xs font-semibold uppercase tracking-[0.2em] ${
                      capture.vlm_processed ? "text-brass-700" : "text-ink-400"
                    }`}
                  >
                    {capture.vlm_processed ? "processed" : "queued"}
                  </p>
                </div>
              </div>
              <p class="mt-3 text-sm leading-6 text-ink-500">
                {capture.description ?? "VLM 記述はまだ生成されていません。"}
              </p>
            </div>
          {/each}
        {/if}
      </div>
    </article>
  </section>
</section>

{#if stopConfirmOpen}
  <div class="fixed inset-0 z-40 flex items-center justify-center bg-ink-950/45 px-4">
    <div class="w-full max-w-md rounded-[1.75rem] border border-white/70 bg-white px-6 py-6 shadow-panel">
      <p class="text-xs font-semibold uppercase tracking-[0.24em] text-cinnabar-600">Stop Recording</p>
      <h3 class="mt-3 text-2xl font-bold text-ink-900">記録を停止しますか？</h3>
      <p class="mt-3 text-sm leading-7 text-ink-500">
        記録を停止すると新しいキャプチャ取得が中断されます。
      </p>

      <div class="mt-6 flex justify-end gap-3">
        <button
          class="rounded-full border border-ink-200 bg-white px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700"
          type="button"
          onclick={() => {
            stopConfirmOpen = false;
          }}
        >
          キャンセル
        </button>
        <button
          class="rounded-full bg-cinnabar-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-cinnabar-500 disabled:cursor-not-allowed disabled:opacity-60"
          type="button"
          onclick={confirmStopRecording}
          disabled={actionPending === "recording"}
        >
          停止する
        </button>
      </div>
    </div>
  </div>
{/if}
