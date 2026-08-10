<script lang="ts">
  interface Props {
    label: string;
    value: string;
    hint?: string;
    accent?: 'amber' | 'slate';
  }

  let { label, value, hint = '', accent = 'slate' }: Props = $props();

  const ring = $derived(
    accent === 'amber' ? 'border-amber-500/30 bg-amber-500/5' : 'border-slate-800 bg-slate-900/60'
  );
  const tone = $derived(accent === 'amber' ? 'text-amber-300' : 'text-slate-100');
</script>

<div class="rounded-2xl border p-6 {ring}">
  <div class="text-xs font-medium uppercase tracking-wider text-slate-400">{label}</div>
  <div class="tnum mt-2 text-4xl font-semibold tracking-tight {tone}">{value}</div>
  {#if hint}
    <div class="mt-2 text-xs leading-relaxed text-slate-500">{hint}</div>
  {/if}
</div>
