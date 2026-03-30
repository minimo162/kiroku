<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";

  const navItems = [
    { href: "/dashboard", label: "ダッシュボード", status: "live" },
    { href: "/settings", label: "設定", status: "live" },
    { href: "/preview", label: "記述プレビュー", status: "live" },
    { href: "/export", label: "エクスポート", status: "live" }
  ];
</script>

<div class="min-h-screen px-4 py-4 text-ink-900 sm:px-6 lg:px-8">
  <div class="mx-auto grid min-h-[calc(100vh-2rem)] max-w-7xl gap-4 lg:grid-cols-[280px_minmax(0,1fr)]">
    <aside class="overflow-hidden rounded-[2rem] border border-white/70 bg-white/80 shadow-panel backdrop-blur">
      <div class="flex h-full flex-col px-5 py-6">
        <div class="rounded-[1.5rem] bg-ink-900 px-5 py-5 text-white">
          <p class="text-xs font-semibold uppercase tracking-[0.32em] text-white/55">Kiroku</p>
          <h1 class="mt-3 text-2xl font-bold">業務記録ダッシュボード</h1>
          <p class="mt-3 text-sm leading-6 text-white/75">
            キャプチャ、差分検出、VLM 処理状況を一つの画面で確認します。
          </p>
        </div>

        <nav class="mt-6 space-y-2">
          {#each navItems as item}
            <a
              href={item.href}
              class={`flex items-center justify-between rounded-2xl border px-4 py-3 text-sm font-medium transition ${
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
              <span
                class={`rounded-full px-2 py-1 text-[10px] font-semibold uppercase tracking-[0.24em] ${
                  item.status === "live"
                    ? "bg-brass-100 text-brass-700"
                    : "bg-ink-100 text-ink-400"
                }`}
              >
                {item.status}
              </span>
            </a>
          {/each}
        </nav>

        <div class="mt-auto rounded-[1.5rem] border border-ink-100 bg-ink-50/80 px-4 py-4">
          <p class="text-xs font-semibold uppercase tracking-[0.24em] text-ink-400">ショートカット</p>
          <p class="mt-3 text-sm leading-6 text-ink-600">
            `Ctrl+Shift+R` で記録開始/停止。
            ウィンドウを閉じてもトレイに常駐します。
          </p>
        </div>
      </div>
    </aside>

    <main class="min-w-0">
      <slot />
    </main>
  </div>
</div>
