<script lang="ts">
  import StatusCard from "$lib/components/dashboard/StatusCard.svelte";
  import type { DashboardStat } from "$lib/types/dashboard";

  const stats: DashboardStat[] = [
    { title: "本日のキャプチャ", value: "0", unit: "枚", detail: "初期構成では未記録", tone: "brass" },
    { title: "有効フレーム", value: "0", unit: "枚", detail: "差分検出は次タスクで実装", tone: "ink" },
    { title: "VLM 処理状況", value: "0%", unit: "", detail: "バッチ基盤は未接続", tone: "brass" }
  ];

  const highlights = [
    "Tauri v2 と Svelte 5 の SPA 構成",
    "Tailwind CSS と Noto Sans JP の日本語 UI ベース",
    "Rust バックエンドに録画・推論用 crate を事前追加",
    "MSI 配布を見据えた Windows バンドル設定"
  ];
</script>

<svelte:head>
  <title>Kiroku | 業務記録ダッシュボード</title>
</svelte:head>

<main class="min-h-screen px-6 py-8 text-ink-900 sm:px-8 lg:px-10">
  <div class="mx-auto flex max-w-6xl flex-col gap-8">
    <section class="overflow-hidden rounded-[2rem] border border-white/60 bg-white/75 shadow-panel backdrop-blur">
      <div class="grid gap-8 px-6 py-8 lg:grid-cols-[1.3fr_0.7fr] lg:px-8">
        <div class="space-y-6">
          <div class="inline-flex items-center rounded-full border border-brass-200 bg-brass-50 px-3 py-1 text-sm font-medium text-brass-700">
            Kiroku Desktop Recorder
          </div>
          <div class="space-y-4">
            <p class="text-sm font-semibold uppercase tracking-[0.28em] text-ink-400">Task 1 bootstrap</p>
            <h1 class="max-w-3xl text-4xl font-bold leading-tight text-ink-900 sm:text-5xl">
              業務記録アプリの初期構成を
              <span class="text-brass-600">Tauri v2 + Svelte 5</span>
              で構築
            </h1>
            <p class="max-w-2xl text-base leading-7 text-ink-500 sm:text-lg">
              画面キャプチャ、差分検出、VLM バッチ処理を段階的に載せるための基盤です。
              この段階では UI 骨格、Tailwind、Rust 依存、Windows 向けバンドル設定まで整えています。
            </p>
          </div>
          <div class="flex flex-wrap gap-3">
            <button class="rounded-full bg-ink-900 px-5 py-3 text-sm font-semibold text-white transition hover:bg-ink-700">
              記録開始
            </button>
            <button class="rounded-full border border-ink-200 bg-white px-5 py-3 text-sm font-semibold text-ink-700 transition hover:border-brass-300 hover:text-brass-700">
              設定を開く
            </button>
          </div>
        </div>

        <div class="rounded-[1.75rem] border border-ink-100 bg-ink-900 px-5 py-5 text-white">
          <p class="text-sm font-semibold uppercase tracking-[0.24em] text-white/60">Ready for next tasks</p>
          <div class="mt-5 space-y-4">
            {#each highlights as item}
              <div class="flex items-start gap-3">
                <div class="mt-1 h-2.5 w-2.5 rounded-full bg-brass-400"></div>
                <p class="text-sm leading-6 text-white/85">{item}</p>
              </div>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <section class="grid gap-4 md:grid-cols-3">
      {#each stats as stat}
        <StatusCard {...stat} />
      {/each}
    </section>

    <section class="grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
      <div class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
        <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">ロードマップ</p>
        <div class="mt-5 space-y-4">
          <div class="rounded-2xl border border-brass-200 bg-brass-50/70 p-4">
            <p class="text-sm font-semibold text-brass-700">次の実装対象</p>
            <p class="mt-2 text-sm leading-6 text-ink-600">
              Rust の共有状態、キャプチャレコード、設定永続化を追加して、
              スクリーンショット取得と記録ループの土台を作ります。
            </p>
          </div>
          <div class="rounded-2xl border border-ink-100 p-4">
            <p class="text-sm font-semibold text-ink-700">作成済みディレクトリ</p>
            <p class="mt-2 text-sm leading-6 text-ink-500">
              `src/lib/components`、`src/lib/types` を用意し、以後の画面実装をここへ集約できます。
            </p>
          </div>
        </div>
      </div>

      <div class="rounded-[1.75rem] border border-white/70 bg-white/80 p-6 shadow-panel backdrop-blur">
        <p class="text-sm font-semibold uppercase tracking-[0.24em] text-ink-400">スタック</p>
        <dl class="mt-5 space-y-4 text-sm text-ink-600">
          <div class="flex items-start justify-between gap-4 border-b border-ink-100 pb-3">
            <dt class="font-medium text-ink-800">Frontend</dt>
            <dd>Svelte 5 / SvelteKit / TypeScript</dd>
          </div>
          <div class="flex items-start justify-between gap-4 border-b border-ink-100 pb-3">
            <dt class="font-medium text-ink-800">Style</dt>
            <dd>Tailwind CSS / Noto Sans JP</dd>
          </div>
          <div class="flex items-start justify-between gap-4 border-b border-ink-100 pb-3">
            <dt class="font-medium text-ink-800">Desktop</dt>
            <dd>Tauri v2 / Rust</dd>
          </div>
          <div class="flex items-start justify-between gap-4">
            <dt class="font-medium text-ink-800">Bundle</dt>
            <dd>Windows MSI target</dd>
          </div>
        </dl>
      </div>
    </section>
  </div>
</main>
