<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import type {
    DescriptionHistoryRecord,
    PreviewCaptureRecord,
    PreviewPagePayload
  } from "$lib/types/preview";

  const emptyPage: PreviewPagePayload = {
    selected_date: null,
    available_dates: [],
    total: 0,
    page: 1,
    page_size: 50,
    records: []
  };

  let preview = $state<PreviewPagePayload>({ ...emptyPage });
  let selectedId = $state<string | null>(null);
  let editingDescription = $state("");
  let history = $state<DescriptionHistoryRecord[]>([]);
  let loading = $state(true);
  let saving = $state(false);
  let historyLoading = $state(false);
  let message = $state<string | null>(null);
  let tauriAvailable = $state(false);
  let selectedDateInput = $state("");

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  const formatDateTime = (value: string) => {
    const date = new Date(value);
    if (Number.isNaN(date.valueOf())) return value;

    return new Intl.DateTimeFormat("ja-JP", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit"
    }).format(date);
  };

  const pageCount = () =>
    preview.total === 0 ? 1 : Math.ceil(preview.total / preview.page_size);

  const selectedRecord = () =>
    preview.records.find((record) => record.id === selectedId) ?? null;

  const previewImageUrl = (record: PreviewCaptureRecord | null) => {
    if (!tauriAvailable || !record?.image_exists || !record.image_path) {
      return null;
    }

    return convertFileSrc(record.image_path);
  };

  async function loadHistory(captureId: string) {
    if (!isTauri()) return;

    historyLoading = true;
    try {
      history = await invoke<DescriptionHistoryRecord[]>("get_capture_description_history", {
        captureId
      });
    } finally {
      historyLoading = false;
    }
  }

  async function selectRecord(record: PreviewCaptureRecord) {
    selectedId = record.id;
    editingDescription = record.description ?? "";
    await loadHistory(record.id);
  }

  async function loadPage(date?: string | null, page = 1) {
    if (!isTauri()) {
      loading = false;
      return;
    }

    loading = true;
    message = null;
    try {
      preview = await invoke<PreviewPagePayload>("get_capture_preview_page", {
        date: date ?? null,
        page
      });
      selectedDateInput = preview.selected_date ?? "";

      if (preview.records.length === 0) {
        selectedId = null;
        editingDescription = "";
        history = [];
        return;
      }

      const nextSelected =
        preview.records.find((record) => record.id === selectedId) ?? preview.records[0];
      await selectRecord(nextSelected);
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
    }
  }

  async function saveDescription() {
    if (!isTauri() || !selectedId) return;

    saving = true;
    message = null;
    try {
      const updated = await invoke<PreviewCaptureRecord>("update_capture_description", {
        captureId: selectedId,
        description: editingDescription
      });
      preview = {
        ...preview,
        records: preview.records.map((record) => (record.id === updated.id ? updated : record))
      };
      editingDescription = updated.description ?? "";
      message = "記述を保存しました。";
      await loadHistory(selectedId);
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    } finally {
      saving = false;
    }
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void loadPage();

    if (!tauriAvailable) {
      message = "ブラウザプレビューでは記述の取得や保存を実行できません。";
    }
  });
</script>

<svelte:head>
  <title>Kiroku | 記述プレビュー</title>
</svelte:head>

<section class="space-y-4">
  <div class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
    <div class="grid gap-6 px-6 py-6 lg:grid-cols-[1.1fr_0.9fr] lg:px-8">
      <div class="space-y-4">
        <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
          Preview
        </div>
        <h2 class="text-3xl font-bold text-ink-900">スクリーンショットと記述の確認</h2>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          日付ごとにキャプチャを確認し、VLM の記述をその場で修正できます。
          画像が削除済みでも説明文と編集履歴は DB から追跡できます。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">Controls</p>
        <div class="mt-4 space-y-3 text-sm leading-6 text-white/80">
          <p>1ページあたり 50 件まで表示します。</p>
          <p>画像が残っている場合は右側にプレビューを表示します。</p>
          <p>保存時は変更前後の記述を履歴テーブルへ追記します。</p>
        </div>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[360px_minmax(0,1fr)]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-5 shadow-panel backdrop-blur">
      <div class="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Record List</p>
          <h3 class="mt-2 text-2xl font-bold text-ink-900">記録一覧</h3>
        </div>
        <div class="min-w-[180px]">
          <label class="text-xs font-semibold uppercase tracking-[0.2em] text-ink-400" for="preview-date">
            対象日
          </label>
          <input
            id="preview-date"
            class="mt-2 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="date"
            bind:value={selectedDateInput}
            onchange={() => void loadPage(selectedDateInput, 1)}
          />
        </div>
      </div>

      {#if preview.available_dates.length > 0}
        <div class="mt-4 flex flex-wrap gap-2">
          {#each preview.available_dates.slice(0, 6) as entry}
            <button
              class={`rounded-full border px-3 py-2 text-xs font-semibold transition ${
                entry.date === preview.selected_date
                  ? "border-brass-200 bg-brass-50 text-brass-700"
                  : "border-ink-100 bg-white text-ink-500 hover:border-brass-200 hover:text-ink-900"
              }`}
              onclick={() => void loadPage(entry.date, 1)}
            >
              {entry.date} · {entry.count}件
            </button>
          {/each}
        </div>
      {/if}

      <div class="mt-5 flex items-center justify-between text-sm text-ink-500">
        <span>{preview.total} 件</span>
        <span>{preview.page} / {pageCount()} ページ</span>
      </div>

      <div class="mt-4 space-y-2">
        {#if loading}
          <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-10 text-center text-sm text-ink-400">
            読み込み中...
          </div>
        {:else if preview.records.length === 0}
          <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-10 text-center text-sm text-ink-400">
            この日の記録はありません。
          </div>
        {:else}
          {#each preview.records as record}
            <button
              class={`w-full rounded-[1.35rem] border px-4 py-4 text-left transition ${
                record.id === selectedId
                  ? "border-brass-200 bg-brass-50/70"
                  : "border-ink-100 bg-white hover:border-ink-200 hover:bg-ink-50/70"
              }`}
              onclick={() => void selectRecord(record)}
            >
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <p class="text-sm font-semibold text-ink-900">{formatDateTime(record.timestamp)}</p>
                  <p class="mt-1 truncate text-sm text-ink-500">{record.app}</p>
                  <p class="mt-1 truncate text-xs text-ink-400">{record.window_title}</p>
                </div>
                <span
                  class={`rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.24em] ${
                    record.vlm_processed
                      ? "bg-brass-100 text-brass-700"
                      : "bg-ink-100 text-ink-500"
                  }`}
                >
                  {record.vlm_processed ? "processed" : "queued"}
                </span>
              </div>
            </button>
          {/each}
        {/if}
      </div>

      <div class="mt-5 flex items-center justify-between gap-3">
        <button
          class="rounded-full border border-ink-100 px-4 py-2 text-sm font-semibold text-ink-600 transition hover:border-brass-200 hover:text-ink-900 disabled:cursor-not-allowed disabled:opacity-45"
          onclick={() => void loadPage(preview.selected_date, preview.page - 1)}
          disabled={loading || preview.page <= 1}
        >
          前へ
        </button>
        <button
          class="rounded-full border border-ink-100 px-4 py-2 text-sm font-semibold text-ink-600 transition hover:border-brass-200 hover:text-ink-900 disabled:cursor-not-allowed disabled:opacity-45"
          onclick={() => void loadPage(preview.selected_date, preview.page + 1)}
          disabled={loading || preview.page >= pageCount()}
        >
          次へ
        </button>
      </div>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      {#if selectedRecord()}
        <div class="grid gap-5 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
          <div class="space-y-4">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Screenshot</p>
                <h3 class="mt-2 text-2xl font-bold text-ink-900">画像プレビュー</h3>
              </div>
              {#if selectedRecord()?.image_exists}
                <span class="rounded-full bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
                  available
                </span>
              {:else}
                <span class="rounded-full bg-ink-100 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-ink-500">
                  removed
                </span>
              {/if}
            </div>

            {#if previewImageUrl(selectedRecord())}
              <div class="overflow-hidden rounded-[1.5rem] border border-ink-100 bg-ink-950/95 p-3">
                <img
                  class="max-h-[420px] w-full rounded-[1.1rem] object-contain"
                  src={previewImageUrl(selectedRecord()) ?? undefined}
                  alt="選択中のスクリーンショット"
                />
              </div>
            {:else}
              <div class="flex min-h-[280px] items-center justify-center rounded-[1.5rem] border border-dashed border-ink-200 bg-ink-50 px-6 text-center text-sm leading-7 text-ink-400">
                画像ファイルは削除済み、またはこの環境では直接表示できません。
              </div>
            {/if}

            <div class="rounded-[1.5rem] border border-ink-100 bg-ink-50/80 px-4 py-4 text-sm leading-7 text-ink-600">
              <p><span class="font-semibold text-ink-900">アプリ:</span> {selectedRecord()?.app}</p>
              <p><span class="font-semibold text-ink-900">ウィンドウ:</span> {selectedRecord()?.window_title}</p>
              <p><span class="font-semibold text-ink-900">時刻:</span> {formatDateTime(selectedRecord()?.timestamp ?? "")}</p>
            </div>
          </div>

          <div class="space-y-4">
            <div class="flex items-center justify-between gap-3">
              <div>
                <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Description</p>
                <h3 class="mt-2 text-2xl font-bold text-ink-900">記述の編集</h3>
              </div>
              <button
                class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
                onclick={saveDescription}
                disabled={!tauriAvailable || saving}
              >
                {saving ? "保存中..." : "保存"}
              </button>
            </div>

            <textarea
              class="min-h-[220px] w-full rounded-[1.5rem] border border-ink-100 bg-white px-4 py-4 text-sm leading-7 text-ink-700 outline-none transition focus:border-brass-300"
              bind:value={editingDescription}
              placeholder="この画面で行っている作業を編集します"
            ></textarea>

            {#if message}
              <div class="rounded-[1.25rem] border border-brass-100 bg-brass-50 px-4 py-3 text-sm text-brass-800">
                {message}
              </div>
            {/if}

            <div class="rounded-[1.5rem] border border-ink-100 bg-ink-50/70 px-4 py-4">
              <div class="flex items-center justify-between gap-3">
                <div>
                  <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">History</p>
                  <h4 class="mt-2 text-lg font-bold text-ink-900">編集履歴</h4>
                </div>
                {#if historyLoading}
                  <span class="text-sm text-ink-400">更新中...</span>
                {/if}
              </div>

              <div class="mt-4 space-y-3">
                {#if history.length === 0}
                  <div class="rounded-[1.25rem] border border-dashed border-ink-200 bg-white px-4 py-5 text-sm text-ink-400">
                    まだ手動編集の履歴はありません。
                  </div>
                {:else}
                  {#each history as entry}
                    <div class="rounded-[1.25rem] border border-white bg-white px-4 py-4">
                      <p class="text-xs font-semibold uppercase tracking-[0.22em] text-ink-400">
                        {formatDateTime(entry.edited_at)}
                      </p>
                      <div class="mt-3 grid gap-3 md:grid-cols-2">
                        <div>
                          <p class="text-xs font-semibold uppercase tracking-[0.18em] text-ink-400">Before</p>
                          <p class="mt-2 text-sm leading-6 text-ink-600">
                            {entry.previous_description ?? "未設定"}
                          </p>
                        </div>
                        <div>
                          <p class="text-xs font-semibold uppercase tracking-[0.18em] text-ink-400">After</p>
                          <p class="mt-2 text-sm leading-6 text-ink-600">
                            {entry.new_description ?? "未設定"}
                          </p>
                        </div>
                      </div>
                    </div>
                  {/each}
                {/if}
              </div>
            </div>
          </div>
        </div>
      {:else}
        <div class="flex min-h-[520px] items-center justify-center rounded-[1.5rem] border border-dashed border-ink-200 bg-ink-50/70 px-6 text-center text-sm leading-7 text-ink-400">
          表示できる記録がありません。日付を変更するか、キャプチャ後に再読み込みしてください。
        </div>
      {/if}
    </article>
  </div>
</section>
