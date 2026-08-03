<script lang="ts">
  /**
   * Un nivel instantáneo de audio. No es progreso: no va a ninguna parte.
   *
   * Se dibuja con `scaleX` y `transform-origin` en el borde y no con `width`,
   * porque esto se actualiza decenas de veces por segundo: animar `width`
   * obliga a recalcular layout en cada cuadro, y `transform` solo compone.
   *
   * Sin transición a propósito: un medidor suavizado miente sobre lo que está
   * entrando por el micrófono justo ahora.
   */
  let {
    value = 0,
    tone = "mic",
    label,
  }: {
    /** 0..1. Se recorta al rango. */
    value?: number;
    tone?: "mic" | "sys";
    label?: string;
  } = $props();

  const TONES = { mic: "bg-mic", sys: "bg-sys" };
  const level = $derived(Math.min(Math.max(value, 0), 1));
</script>

<div class="flex items-center gap-1.5">
  {#if label}
    <span class="w-6 shrink-0 text-micro text-faint uppercase">{label}</span>
  {/if}
  <div class="h-1 flex-1 overflow-hidden rounded-pill bg-surface-2">
    <div
      class="h-full origin-left rounded-pill {TONES[tone]}"
      style:transform="scaleX({level})"
    ></div>
  </div>
</div>
