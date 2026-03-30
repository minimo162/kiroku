<script lang="ts">
  type Tone = "brass" | "ink";
  const toneClasses: Record<Tone, string> = {
    brass: "border-brass-200 bg-brass-50/70 text-brass-700",
    ink: "border-ink-200 bg-ink-50/70 text-ink-700"
  };

  let {
    title,
    value,
    unit,
    detail,
    tone = "brass"
  }: {
    title: string;
    value: string;
    unit: string;
    detail: string;
    tone?: Tone;
  } = $props();
</script>

<article class={`rounded-[1.5rem] border p-5 shadow-panel ${toneClasses[tone]}`}>
  <p class="text-sm font-semibold uppercase tracking-[0.2em]">{title}</p>
  <div class="mt-6 flex items-end gap-2">
    <span class="text-4xl font-bold leading-none text-ink-900">{value}</span>
    {#if unit}
      <span class="pb-1 text-sm font-medium text-ink-500">{unit}</span>
    {/if}
  </div>
  <p class="mt-4 text-sm leading-6 text-ink-500">{detail}</p>
</article>
