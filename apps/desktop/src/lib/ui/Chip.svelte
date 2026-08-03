<script lang="ts">
  /**
   * Una etiqueta de estado. Reemplaza a `.rb-chip*`.
   *
   * `neutral` no lleva color por definición: en esta paleta el color significa
   * estado, y un chip que solo clasifica no es un estado.
   */
  import type { Snippet } from "svelte";

  type Tone = "neutral" | "ok" | "warn" | "danger" | "rec" | "info";

  let {
    tone = "neutral",
    children,
  }: {
    tone?: Tone;
    children: Snippet;
  } = $props();

  const TONES: Record<Tone, string> = {
    neutral: "bg-surface-2 text-muted border-line",
    ok: "bg-ok-soft text-ok border-transparent",
    warn: "bg-warn-soft text-warn border-transparent",
    danger: "bg-danger-soft text-danger border-transparent",
    info: "bg-info-soft text-info border-transparent",
    rec: "bg-danger-soft text-rec border-transparent",
  };
</script>

<span
  class="inline-flex h-5 items-center rounded-pill border px-2 text-xs font-medium {TONES[
    tone
  ]}"
>
  {@render children()}
</span>
