<script lang="ts">
  import { fly } from "svelte/transition";
  import { removeToast, toastState } from "$lib/toast.svelte";

  const toneClasses = {
    success: "border-brass-200 bg-brass-50 text-brass-900",
    error: "border-cinnabar-200 bg-cinnabar-50 text-cinnabar-900",
    info: "border-ink-200 bg-ink-50 text-ink-900"
  } as const;

  const badgeClasses = {
    success: "bg-brass-100 text-brass-700",
    error: "bg-cinnabar-100 text-cinnabar-700",
    info: "bg-ink-100 text-ink-600"
  } as const;

  const labels = {
    success: "success",
    error: "error",
    info: "info"
  } as const;
</script>

<div class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-[min(360px,calc(100vw-2rem))] flex-col gap-3">
  {#each toastState.toasts as toast (toast.id)}
    <div
      class={`pointer-events-auto rounded-[1.35rem] border px-4 py-4 shadow-panel backdrop-blur ${toneClasses[toast.type]}`}
      in:fly={{ y: 16, duration: 180 }}
      out:fly={{ y: 16, duration: 150 }}
    >
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0">
          <span
            class={`inline-flex rounded-full px-2.5 py-1 text-[10px] font-semibold uppercase tracking-[0.2em] ${badgeClasses[toast.type]}`}
          >
            {labels[toast.type]}
          </span>
          <p class="mt-3 text-sm leading-6">{toast.message}</p>
        </div>
        <button
          class="rounded-full border border-current/15 px-2 py-1 text-xs font-semibold uppercase tracking-[0.2em] transition hover:bg-white/60"
          type="button"
          onclick={() => removeToast(toast.id)}
          aria-label="トーストを閉じる"
        >
          ×
        </button>
      </div>
    </div>
  {/each}
</div>
