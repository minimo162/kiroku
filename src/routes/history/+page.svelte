<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { highlightText } from "$lib/highlight";
  import { addToast } from "$lib/toast.svelte";

  type CaptureAppGroup = {
    app: string;
    count: number;
  };

  type HistoryCaptureRecord = {
    id: string;
    timestamp: string;
    capture_date: string;
    app: string;
    window_title: string;
    description: string | null;
    vlm_processed: boolean;
  };

  type HistorySearchResponse = {
    total: number;
    page: number;
    page_size: number;
    apps: CaptureAppGroup[];
    results: HistoryCaptureRecord[];
  };

  type SearchRequest = {
    query: string | null;
    app_filter: string[] | null;
    date_from: string | null;
    date_to: string | null;
    page: number;
  };

  let request = $state<SearchRequest>({
    query: null,
    app_filter: null,
    date_from: null,
    date_to: null,
    page: 1
  });
  let response = $state<HistorySearchResponse>({
    total: 0,
    page: 1,
    page_size: 50,
    apps: [],
    results: []
  });
  let loading = $state(true);
  let searching = $state(false);
  let tauriAvailable = $state(false);
  let debounceTimer: number | undefined;

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  const pageCount = () =>
    response.total === 0 ? 1 : Math.ceil(response.total / response.page_size);

  const hasSelectedApps = (app: string) => request.app_filter?.includes(app) ?? false;
  const dateRangeError = $derived(
    request.date_from && request.date_to && request.date_from > request.date_to
      ? "開始日は終了日以前にしてください"
      : null
  );
  const hasActiveFilters = $derived(
    Boolean(request.query || request.date_from || request.date_to || request.app_filter?.length)
  );

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

  async function runSearch() {
    if (!isTauri() || dateRangeError) {
      loading = false;
      return;
    }

    searching = true;
    try {
      response = await invoke<HistorySearchResponse>("search_captures", { request });
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      loading = false;
      searching = false;
    }
  }

  function scheduleSearch(resetPage = true) {
    if (dateRangeError) {
      if (debounceTimer) {
        window.clearTimeout(debounceTimer);
      }
      return;
    }

    if (resetPage) {
      request = { ...request, page: 1 };
    }

    if (debounceTimer) {
      window.clearTimeout(debounceTimer);
    }
    debounceTimer = window.setTimeout(() => {
      void runSearch();
    }, 250);
  }

  function toggleApp(app: string) {
    const nextApps = new Set(request.app_filter ?? []);
    if (nextApps.has(app)) {
      nextApps.delete(app);
    } else {
      nextApps.add(app);
    }

    request = {
      ...request,
      app_filter: nextApps.size > 0 ? [...nextApps].sort() : null
    };
    scheduleSearch();
  }

  function selectAllApps() {
    request = {
      ...request,
      app_filter: response.apps.map((entry) => entry.app),
      page: 1
    };
    scheduleSearch(false);
  }

  function clearApps() {
    request = {
      ...request,
      app_filter: null,
      page: 1
    };
    scheduleSearch(false);
  }

  function resetFilters() {
    request = {
      query: null,
      app_filter: null,
      date_from: null,
      date_to: null,
      page: 1
    };
    void runSearch();
  }

  function openPreview(record: HistoryCaptureRecord) {
    void goto(`/preview?date=${record.capture_date}`);
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void runSearch();

    if (!tauriAvailable) {
      addToast("info", "ブラウザプレビューでは履歴検索を実行できません。");
    }

    return () => {
      if (debounceTimer) {
        window.clearTimeout(debounceTimer);
      }
    };
  });
</script>

<svelte:head>
  <title>Kiroku | 履歴</title>
</svelte:head>

<section class="space-y-4">
  <div class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
    <div class="grid gap-6 px-6 py-6 lg:grid-cols-[1.1fr_0.9fr] lg:px-8">
      <div class="space-y-4">
        <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
          履歴
        </div>
        <h2 class="text-3xl font-bold text-ink-900">履歴と検索</h2>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          日付、アプリ、キーワードで過去の記録を横断検索できます。結果から該当日のプレビュー画面へ遷移して詳細確認に進めます。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">検索のヒント</p>
        <div class="mt-4 space-y-3 text-sm leading-6 text-white/80">
          <p>ウィンドウタイトル、説明文、アプリ名が検索対象です。</p>
          <p>検索結果は新しい記録から順に 50 件ずつ表示します。</p>
          <p>条件未指定なら全件を対象にします。</p>
        </div>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[360px_minmax(0,1fr)]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <div class="flex items-start justify-between gap-4">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">絞り込み</p>
          <h3 class="mt-2 text-2xl font-bold text-ink-900">検索条件</h3>
        </div>
        <div class="flex flex-wrap justify-end gap-2">
          {#if hasActiveFilters}
            <button
              class="rounded-full border border-ink-200 bg-white px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700"
              type="button"
              onclick={resetFilters}
            >
              フィルターをリセット
            </button>
          {/if}
          <button
            class="rounded-full bg-ink-900 px-4 py-2 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
            type="button"
            onclick={() => void runSearch()}
            disabled={searching || !!dateRangeError}
          >
            検索
          </button>
        </div>
      </div>

      <div class="mt-6 space-y-5">
        <div>
          <label class="text-sm font-medium text-ink-700" for="history-query">キーワード</label>
          <input
            id="history-query"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="text"
            value={request.query ?? ""}
            placeholder="例: 連結PKG, Outlook, 月次決算"
            oninput={(event) => {
              request = {
                ...request,
                query: (event.currentTarget as HTMLInputElement).value || null
              };
              scheduleSearch();
            }}
          />
        </div>

        <div class="grid gap-4 sm:grid-cols-2">
          <div>
            <label class="text-sm font-medium text-ink-700" for="history-date-from">開始日</label>
            <input
              id="history-date-from"
              class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
              type="date"
              value={request.date_from ?? ""}
              onchange={(event) => {
                request = {
                  ...request,
                  date_from: (event.currentTarget as HTMLInputElement).value || null
                };
                scheduleSearch();
              }}
            />
          </div>

          <div>
            <label class="text-sm font-medium text-ink-700" for="history-date-to">終了日</label>
            <input
              id="history-date-to"
              class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
              type="date"
              value={request.date_to ?? ""}
              onchange={(event) => {
                request = {
                  ...request,
                  date_to: (event.currentTarget as HTMLInputElement).value || null
                };
                scheduleSearch();
              }}
            />
          </div>
        </div>
        {#if dateRangeError}
          <p class="text-sm text-cinnabar-700">{dateRangeError}</p>
        {/if}

        <div>
          <div class="flex items-center justify-between gap-3">
            <div>
              <p class="text-sm font-medium text-ink-700">アプリフィルター</p>
              <p class="mt-1 text-sm text-ink-500">複数選択できます。未選択なら全件対象です。</p>
            </div>
            <div class="flex items-center gap-3">
              <button
                class="text-sm font-semibold text-brass-700 transition hover:text-brass-800"
                type="button"
                onclick={selectAllApps}
                disabled={response.apps.length === 0}
              >
                全選択
              </button>
              <button
                class="text-sm font-semibold text-brass-700 transition hover:text-brass-800"
                type="button"
                onclick={clearApps}
                disabled={response.apps.length === 0}
              >
                全解除
              </button>
              {#if loading}
                <span class="text-sm text-ink-400">読み込み中...</span>
              {/if}
            </div>
          </div>

          <div class="mt-4 grid gap-3">
            {#if response.apps.length === 0}
              <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-8 text-sm text-ink-400">
                利用可能なアプリがまだありません。
              </div>
            {:else}
              {#each response.apps as entry}
                <label class="flex items-center justify-between rounded-2xl border border-ink-100 bg-ink-50/60 px-4 py-3">
                  <div class="min-w-0">
                    <p class="truncate text-sm font-medium text-ink-700">{entry.app}</p>
                    <p class="mt-1 text-xs text-ink-400">{entry.count} 件</p>
                  </div>
                  <input
                    class="h-5 w-5 accent-brass-600"
                    type="checkbox"
                    checked={hasSelectedApps(entry.app)}
                    onchange={() => toggleApp(entry.app)}
                  />
                </label>
              {/each}
            {/if}
          </div>
        </div>
      </div>
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <div class="flex flex-wrap items-end justify-between gap-4">
        <div>
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">結果</p>
          <h3 class="mt-2 text-2xl font-bold text-ink-900">検索結果</h3>
        </div>
        <div class="text-right text-sm text-ink-500">
          <p>{response.total} 件</p>
          <p>{response.page} / {pageCount()} ページ</p>
        </div>
      </div>

      <div class="mt-6 space-y-3">
        {#if searching}
          <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-10 text-center text-sm text-ink-400">
            検索中...
          </div>
        {:else if response.results.length === 0}
          <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-10 text-center text-sm text-ink-400">
            条件に一致する記録はありません。
          </div>
        {:else}
          {#each response.results as record}
            <button
              class="w-full rounded-[1.5rem] border border-ink-100 bg-ink-50/60 px-5 py-4 text-left transition hover:border-brass-200 hover:bg-brass-50/60"
              type="button"
              onclick={() => openPreview(record)}
            >
              <div class="flex flex-wrap items-center justify-between gap-3">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="rounded-full bg-white px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-ink-500">
                    {record.app}
                  </span>
                  <span
                    class={`rounded-full px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] ${
                      record.vlm_processed
                        ? "bg-brass-100 text-brass-700"
                        : "bg-ink-100 text-ink-500"
                    }`}
                  >
                    {record.vlm_processed ? "処理済み" : "待機中"}
                  </span>
                </div>
                <span class="text-xs text-ink-400">{formatDateTime(record.timestamp)}</span>
              </div>
              <p class="mt-3 truncate text-sm font-semibold text-ink-900">
                {@html highlightText(record.window_title, request.query)}
              </p>
              <p class="mt-2 text-sm leading-6 text-ink-500">
                {@html highlightText(record.description ?? "まだ説明文は生成されていません。", request.query)}
              </p>
            </button>
          {/each}
        {/if}
      </div>

      <div class="mt-6 flex flex-wrap items-center justify-between gap-3">
        <button
          class="rounded-full border border-ink-200 bg-white px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-50"
          type="button"
          onclick={() => {
            request = { ...request, page: Math.max(1, request.page - 1) };
            void runSearch();
          }}
          disabled={request.page <= 1 || searching}
        >
          前のページ
        </button>

        <button
          class="rounded-full border border-ink-200 bg-white px-4 py-2 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700 disabled:cursor-not-allowed disabled:opacity-50"
          type="button"
          onclick={() => {
            request = { ...request, page: Math.min(pageCount(), request.page + 1) };
            void runSearch();
          }}
          disabled={request.page >= pageCount() || searching}
        >
          次のページ
        </button>
      </div>
    </article>
  </div>
</section>
