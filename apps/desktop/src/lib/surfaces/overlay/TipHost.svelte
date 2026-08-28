<script lang="ts">
  /**
   * El tooltip visible. Va montado UNA vez por ventana, al final del layout.
   *
   * El porqué de dibujarlo a mano está en `tip.svelte.ts`. Acá solo queda la
   * ubicación: debajo del ancla si entra, arriba si no, y siempre dentro del
   * viewport.
   */
  import { tipState } from "./tip.svelte";

  /** Aire entre el ancla y el globo. */
  const GAP = 8;
  /** Margen mínimo contra el borde de la ventana. */
  const EDGE = 6;

  let el = $state<HTMLElement | null>(null);
  let x = $state(0);
  let y = $state(0);
  /**
   * Hasta no estar medido se pinta transparente: al primer cuadro el globo
   * todavía está en 0,0 y sin esto se ve saltar desde la esquina.
   */
  let placed = $state(false);

  $effect(() => {
    const anchor = tipState.anchor;
    const text = tipState.text;
    const placement = tipState.placement;
    if (!tipState.open || !anchor || !text || !el) {
      placed = false;
      return;
    }
    // Medir el globo ya renderizado: su alto depende de cuántas líneas entren
    // en `max-width`, así que no se puede suponer.
    const box = el.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const below = anchor.y + anchor.h + GAP;
    const above = anchor.y - box.height - GAP;
    const fitsBelow = below + box.height + EDGE <= vh;
    const fitsAbove = above >= EDGE;
    const top =
      placement === "top"
        ? (fitsAbove ? above : below)
        : placement === "bottom"
          ? (fitsBelow ? below : above)
          : (fitsBelow ? below : fitsAbove ? above : below);
    const left = anchor.x + anchor.w / 2 - box.width / 2;
    x = Math.min(Math.max(left, EDGE), Math.max(EDGE, vw - box.width - EDGE));
    y = Math.min(Math.max(top, EDGE), Math.max(EDGE, vh - box.height - EDGE));
    placed = true;
  });
</script>

{#if tipState.open && tipState.text}
  <!-- `aria-hidden`: el nombre accesible lo pone la action sobre el ancla
       (ver `syncAriaLabel`), así el lector no lo anuncia dos veces. -->
  <div
    class="tip"
    class:is-placed={placed}
    bind:this={el}
    style:left="{x}px"
    style:top="{y}px"
    aria-hidden="true"
  >
    {tipState.text}
  </div>
{/if}

<style>
  .tip {
    position: fixed;
    z-index: var(--z-toast, 100);
    max-width: 22rem;
    border: 1px solid color-mix(in sRGB, var(--line) 80%, transparent);
    border-radius: 0.4rem;
    padding: 0.26rem 0.46rem;
    background: color-mix(in sRGB, var(--surface) 96%, var(--bg));
    box-shadow: 0 8px 22px color-mix(in sRGB, rgb(0 0 0) 32%, transparent);
    color: var(--text);
    font-size: 0.72rem;
    font-weight: 500;
    line-height: 1.3;
    white-space: pre-line;
    opacity: 0;

    /* Duro: el overlay es click-through salvo en sus hit-rects, y un globo que
       reciba el mouse taparía el escritorio de abajo. Tampoco se publica como
       zona viva por lo mismo. */
    pointer-events: none;
    transition: opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out);
  }

  .tip.is-placed {
    opacity: 1;
  }

  @media (prefers-reduced-motion: reduce) {
    .tip {
      transition: none;
    }
  }
</style>
