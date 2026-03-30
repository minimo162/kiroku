<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { addToast } from "$lib/toast.svelte";

  type ExportFilter = {
    start_date: string | null;
    end_date: string | null;
    apps: string[] | null;
    only_processed: boolean;
    apply_masking: boolean;
  };

  type CaptureAppGroup = {
    app: string;
    count: number;
  };

  type ExportOptions = {
    apps: CaptureAppGroup[];
  };

  type ExportPreview = {
    count: number;
  };

  type ExportResult = {
    count: number;
    path: string;
  };

  const today = new Date().toISOString().slice(0, 10);

  let filter = $state<ExportFilter>({
    start_date: today,
    end_date: today,
    apps: null,
    only_processed: true,
    apply_masking: true
  });
  let options = $state<ExportOptions>({ apps: [] });
  let previewCount = $state(0);
  let loading = $state(true);
  let previewLoading = $state(false);
  let exporting = $state(false);
  let tauriAvailable = $state(false);
  let lastExportPath = $state<string | null>(null);
  let previewRequestId = 0;

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  const hasSelectedApps = (app: string) => filter.apps?.includes(app) ?? false;
  const dateRangeError = $derived(
    filter.start_date && filter.end_date && filter.start_date > filter.end_date
      ? "開始日は終了日以前にしてください"
      : null
  );

  async function refreshPreviewCount() {
    if (!isTauri()) return;
    if (dateRangeError) {
      previewCount = 0;
      previewLoading = false;
      return;
    }

    const requestId = ++previewRequestId;
    previewLoading = true;
    try {
      const preview = await invoke<ExportPreview>("preview_csv_export", { filter });
      if (requestId === previewRequestId) {
        previewCount = preview.count;
      }
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      if (requestId === previewRequestId) {
        previewLoading = false;
      }
    }
  }

  function toggleApp(app: string) {
    const nextApps = new Set(filter.apps ?? []);
    if (nextApps.has(app)) {
      nextApps.delete(app);
    } else {
      nextApps.add(app);
    }

    filter = {
      ...filter,
      apps: nextApps.size > 0 ? [...nextApps].sort() : null
    };
    void refreshPreviewCount();
  }

  function selectAllApps() {
    filter = {
      ...filter,
      apps: options.apps.map((entry) => entry.app)
    };
    void refreshPreviewCount();
  }

  function clearApps() {
    filter = {
      ...filter,
      apps: null
    };
    void refreshPreviewCount();
  }

  async function loadOptions() {
    if (!isTauri()) {
      loading = false;
      return;
    }

    loading = true;
    try {
      options = await invoke<ExportOptions>("list_export_options");
      await refreshPreviewCount();
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      loading = false;
    }
  }

  async function handleExport() {
    if (!isTauri() || dateRangeError) return;

    exporting = true;
    try {
      const result = await invoke<ExportResult | null>("export_csv", { filter });
      if (!result) {
        addToast("info", "保存をキャンセルしました。");
        return;
      }

      lastExportPath = result.path;
      addToast("success", `${result.count} 件を CSV に書き出しました。`);
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    } finally {
      exporting = false;
    }
  }

  async function revealExport() {
    if (!lastExportPath) return;
    try {
      await revealItemInDir(lastExportPath);
    } catch (error) {
      addToast("error", error instanceof Error ? error.message : String(error));
    }
  }

  onMount(() => {
    tauriAvailable = isTauri();
    void loadOptions();

    if (!tauriAvailable) {
      addToast("info", "ブラウザプレビューでは CSV エクスポートを実行できません。");
    }
  });
</script>

<svelte:head>
  <title>Kiroku | CSV エクスポート</title>
</svelte:head>

<section class="space-y-4">
  <div class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
    <div class="grid gap-6 px-6 py-6 lg:grid-cols-[1.1fr_0.9fr] lg:px-8">
      <div class="space-y-4">
        <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-xs font-semibold uppercase tracking-[0.24em] text-brass-700">
          Export
        </div>
        <h2 class="text-3xl font-bold text-ink-900">CSV エクスポート</h2>
        <p class="max-w-2xl text-sm leading-7 text-ink-500 sm:text-base">
          日付範囲、対象アプリ、VLM 処理済みのみの条件を組み合わせて CSV を出力します。
          件数プレビューは条件変更のたびに更新されます。
        </p>
      </div>

      <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
        <p class="text-xs font-semibold uppercase tracking-[0.24em] text-white/60">Export Notes</p>
        <div class="mt-4 space-y-3 text-sm leading-6 text-white/80">
          <p>CSV は UTF-8 BOM 付きで出力され、Excel でも文字化けしにくい形式です。</p>
          <p>アプリフィルタ未選択時は、全アプリが対象になります。</p>
          <p>マスキングを有効にすると、設定画面で登録した置換ルールを適用します。</p>
          <p>出力後は Explorer で保存先ファイルをそのまま表示できます。</p>
        </div>
      </div>
    </div>
  </div>

  <div class="grid gap-4 xl:grid-cols-[1fr_0.92fr]">
    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Filters</p>
      <h3 class="mt-2 text-2xl font-bold text-ink-900">出力条件</h3>

      <div class="mt-6 grid gap-5 md:grid-cols-2">
        <div>
          <label class="text-sm font-medium text-ink-700" for="export-start-date">開始日</label>
          <input
            id="export-start-date"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="date"
            bind:value={filter.start_date}
            onchange={() => void refreshPreviewCount()}
          />
        </div>
        <div>
          <label class="text-sm font-medium text-ink-700" for="export-end-date">終了日</label>
          <input
            id="export-end-date"
            class="mt-3 w-full rounded-2xl border border-ink-100 bg-white px-4 py-3 text-sm text-ink-700 outline-none transition focus:border-brass-300"
            type="date"
            bind:value={filter.end_date}
            onchange={() => void refreshPreviewCount()}
          />
        </div>
      </div>
      {#if dateRangeError}
        <p class="mt-3 text-sm text-cinnabar-700">{dateRangeError}</p>
      {/if}

      <label class="mt-5 flex items-center justify-between rounded-2xl border border-ink-100 px-4 py-4">
        <div>
          <p class="text-sm font-medium text-ink-700">VLM 処理済みのみ</p>
          <p class="mt-1 text-sm text-ink-500">記述が付与されたレコードだけに絞り込みます。</p>
        </div>
        <input
          class="h-5 w-5 accent-brass-600"
          type="checkbox"
          bind:checked={filter.only_processed}
          onchange={() => void refreshPreviewCount()}
        />
      </label>

      <label class="mt-5 flex items-center justify-between rounded-2xl border border-brass-200 bg-brass-50/70 px-4 py-4">
        <div>
          <p class="text-sm font-medium text-ink-700">マスキングを適用</p>
          <p class="mt-1 text-sm text-ink-500">
            設定画面のルールで `window_title` と `description` を置換してから出力します。
          </p>
        </div>
        <input
          class="h-5 w-5 accent-brass-600"
          type="checkbox"
          bind:checked={filter.apply_masking}
        />
      </label>

      <div class="mt-5">
        <div class="flex items-center justify-between gap-3">
          <div>
            <p class="text-sm font-medium text-ink-700">対象アプリ</p>
            <p class="mt-1 text-sm text-ink-500">複数選択できます。未選択なら全件対象です。</p>
          </div>
          <div class="flex items-center gap-3">
            <button
              class="text-sm font-semibold text-brass-700 transition hover:text-brass-800"
              type="button"
              onclick={selectAllApps}
              disabled={options.apps.length === 0}
            >
              全選択
            </button>
            <button
              class="text-sm font-semibold text-brass-700 transition hover:text-brass-800"
              type="button"
              onclick={clearApps}
              disabled={options.apps.length === 0}
            >
              全解除
            </button>
            {#if loading}
              <span class="text-sm text-ink-400">読み込み中...</span>
            {/if}
          </div>
        </div>

        <div class="mt-4 grid gap-3 sm:grid-cols-2">
          {#if options.apps.length === 0}
            <div class="rounded-2xl border border-dashed border-ink-200 px-4 py-8 text-sm text-ink-400">
              登録済みアプリがまだありません。
            </div>
          {:else}
            {#each options.apps as entry}
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
    </article>

    <article class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
      <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">Preview</p>
      <h3 class="mt-2 text-2xl font-bold text-ink-900">出力前の確認</h3>

      <div class="mt-6 rounded-[1.5rem] border border-brass-100 bg-brass-50 px-5 py-5">
        <p class="text-xs font-semibold uppercase tracking-[0.22em] text-brass-700">Preview Count</p>
        <div class="mt-3 flex items-end gap-3">
          <span class="text-4xl font-bold text-ink-900">{previewCount}</span>
          <span class="pb-1 text-sm text-ink-500">件が出力されます</span>
        </div>
        <p class="mt-3 text-sm text-ink-500">
          {#if previewLoading}
            条件を確認しています...
          {:else if previewCount === 0}
            現在の条件では出力対象がありません。
          {:else}
            条件が妥当ならこのまま CSV を作成できます。
          {/if}
        </p>
      </div>

      <div class="mt-5 rounded-[1.5rem] border border-ink-100 bg-ink-50/70 px-5 py-5 text-sm leading-7 text-ink-600">
        <p><span class="font-semibold text-ink-900">期間:</span> {filter.start_date ?? "未指定"} から {filter.end_date ?? "未指定"}</p>
        <p>
          <span class="font-semibold text-ink-900">アプリ:</span>
          {filter.apps?.join(", ") ?? "すべて"}
        </p>
        <p><span class="font-semibold text-ink-900">処理条件:</span> {filter.only_processed ? "VLM 処理済みのみ" : "未処理も含む"}</p>
      </div>

      <div class="mt-6 flex flex-wrap gap-3">
        <button
          class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700 disabled:cursor-not-allowed disabled:opacity-60"
          onclick={handleExport}
          disabled={!tauriAvailable || exporting || previewCount === 0 || !!dateRangeError}
        >
          {exporting ? "エクスポート中..." : "CSV をエクスポート"}
        </button>

        <button
          class="rounded-full border border-ink-100 px-5 py-3 text-sm font-semibold text-ink-600 transition hover:border-brass-200 hover:text-ink-900 disabled:cursor-not-allowed disabled:opacity-45"
          onclick={revealExport}
          disabled={!lastExportPath}
        >
          保存先を表示
        </button>
      </div>
    </article>
  </div>
</section>
