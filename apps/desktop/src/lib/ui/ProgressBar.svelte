<script lang="ts">
  /**
   * Progreso de algo que termina: una descarga, una transcripción.
   *
   * `indeterminate` es para cuando se sabe que está corriendo pero no cuánto
   * falta. No es lo mismo que 0%: una barra clavada en cero se lee como
   * «colgado».
   */
  let {
    value = 0,
    label,
    indeterminate = false,
    tone = "accent",
  }: {
    /** 0..1. Se recorta al rango. */
    value?: number;
    /** Si viene, se dibuja arriba con el porcentaje al costado. */
    label?: string;
    indeterminate?: boolean;
    tone?: "accent" | "ok" | "warn";
  } = $props();

  const TONES = { accent: "bg-accent", ok: "bg-ok", warn: "bg-warn" };
  const pct = $derived(Math.round(Math.min(Math.max(value, 0), 1) * 100));
</script>

<div class="flex flex-col gap-1">
  {#if label}
    <div class="flex items-baseline justify-between gap-2">
      <span class="text-xs text-muted">{label}</span>
      {#if !indeterminate}
        <span class="font-mono text-xs text-faint" data-numeric>{pct}%</span>
      {/if}
    </div>
  {/if}

  <div
    class="h-1 overflow-hidden rounded-pill bg-surface-2"
    role="progressbar"
    aria-label={label}
    aria-valuenow={indeterminate ? undefined : pct}
    aria-valuemin={indeterminate ? undefined : 0}
    aria-valuemax={indeterminate ? undefined : 100}
  >
    <div
      class="h-full rounded-pill transition-[width] duration-(--duration-fast) ease-calm
             {TONES[tone]} {indeterminate ? 'w-1/3 animate-pulse' : ''}"
      style:width={indeterminate ? undefined : `${pct}%`}
    ></div>
  </div>
</div>
