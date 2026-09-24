<script lang="ts">
  /**
   * La pizarra en miniatura: todas las consolas y el recuadro de lo que se
   * está mirando. Tocar o arrastrar encima lleva la vista a ese punto.
   *
   * El mundo que dibuja junta las consolas y la vista actual: si se mira un
   * lugar vacío, el recuadro igual aparece y se entiende dónde se está.
   */
  import { t } from "$domain/i18n.svelte";
  import { bounds, fitInto, type Point, type Rect } from "./agentBoard";

  let {
    cards,
    view,
    onGo,
  }: {
    cards: { key: string; rect: Rect; active: boolean; tone: string | null }[];
    /** Lo que se ve ahora, en coordenadas de pizarra. */
    view: Rect;
    /** Llevar la vista a este punto de la pizarra. */
    onGo: (at: Point) => void;
  } = $props();

  const BOX = { w: 184, h: 116 };
  const PAD = 0.08;

  let el = $state<HTMLElement | null>(null);
  let dragging = false;

  const world = $derived.by(() => {
    const box = bounds([...cards.map((c) => c.rect), view]) ?? view;
    const px = box.w * PAD;
    const py = box.h * PAD;
    return { x: box.x - px, y: box.y - py, w: box.w + px * 2, h: box.h + py * 2 };
  });
  const map = $derived(fitInto(world, BOX));
  const seen = $derived(place(view));

  function place(r: Rect) {
    return {
      left: `${r.x * map.scale + map.x}px`,
      top: `${r.y * map.scale + map.y}px`,
      width: `${Math.max(2, r.w * map.scale)}px`,
      height: `${Math.max(2, r.h * map.scale)}px`,
    };
  }

  function go(event: PointerEvent) {
    if (!el) return;
    const box = el.getBoundingClientRect();
    onGo({
      x: (event.clientX - box.left - map.x) / map.scale,
      y: (event.clientY - box.top - map.y) / map.scale,
    });
  }
</script>

<div
  class="minimap"
  bind:this={el}
  style:width={`${BOX.w}px`}
  style:height={`${BOX.h}px`}
  role="slider"
  tabindex="-1"
  aria-label={t("page.agents.board.minimap")}
  aria-valuenow={0}
  onpointerdown={(e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    el?.setPointerCapture(e.pointerId);
    dragging = true;
    go(e);
  }}
  onpointermove={(e) => dragging && go(e)}
  onpointerup={() => (dragging = false)}
  onpointercancel={() => (dragging = false)}
>
  {#each cards as card (card.key)}
    {@const at = place(card.rect)}
    <span
      class="card"
      class:is-active={card.active}
      data-agent={card.tone}
      style:left={at.left}
      style:top={at.top}
      style:width={at.width}
      style:height={at.height}
    ></span>
  {/each}
  <span
    class="view"
    style:left={seen.left}
    style:top={seen.top}
    style:width={seen.width}
    style:height={seen.height}
  ></span>
</div>

<style>
  .minimap {
    position: relative;
    overflow: hidden;
    border-radius: 12px;
    background: color-mix(in sRGB, var(--rb-surface) 82%, transparent);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 10%, transparent),
      0 14px 32px -16px rgb(0 0 0 / 55%);
    cursor: crosshair;
    backdrop-filter: blur(18px) saturate(1.2);
    touch-action: none;
  }

  .card {
    position: absolute;
    border-radius: 2px;
    background: color-mix(
      in sRGB,
      var(--agent-accent, var(--rb-text)) 45%,
      transparent
    );
    pointer-events: none;
  }

  .card.is-active {
    background: var(--agent-accent, var(--accent));
  }

  .view {
    position: absolute;
    border-radius: 3px;
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
    box-shadow: inset 0 0 0 1.5px color-mix(in sRGB, var(--rb-text) 55%, transparent);
    pointer-events: none;
  }
</style>
