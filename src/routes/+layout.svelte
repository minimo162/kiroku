<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import type { Snippet } from "svelte";
  import ToastContainer from "$lib/components/ToastContainer.svelte";

  let { children }: { children: Snippet } = $props();

  type SetupStatus = {
    setup_complete: boolean;
  };

  const navItems = [
    { href: "/dashboard", label: "ダッシュボード", status: "live" },
    { href: "/history", label: "履歴", status: "live" },
    { href: "/settings", label: "設定", status: "live" },
    { href: "/preview", label: "記述プレビュー", status: "live" },
    { href: "/export", label: "エクスポート", status: "live" }
  ];

  let checkingSetup = $state(true);
  let tauriAvailable = $state(false);

  const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  const isSetupRoute = () => $page.url.pathname.startsWith("/setup");

  onMount(() => {
    tauriAvailable = isTauri();
    if (!tauriAvailable) {
      checkingSetup = false;
      return;
    }

    void (async () => {
      try {
        const status = await invoke<SetupStatus>("get_setup_status");
        if (!status.setup_complete && !isSetupRoute()) {
          await goto("/setup");
        } else if (status.setup_complete && isSetupRoute()) {
          await goto("/dashboard");
        }
      } finally {
        checkingSetup = false;
      }
    })();
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
        {@render children()}
      </main>
    </div>
  </div>
  <ToastContainer />
{/if}
