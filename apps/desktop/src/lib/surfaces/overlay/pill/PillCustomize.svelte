<script lang="ts">
  /**
   * Editor de la pill, como cara de la isla.
   *
   * Tres filas —a la vista, detrás de «Más», fuera— con las mismas fichas que
   * la tira. Se arrastran dentro de una fila o entre filas, y cada soltar se
   * guarda en el acto: no hay «guardar», la tira queda como se la dejó. Con
   * teclado, flechas: ←/→ dentro de la fila, ↑/↓ entre filas.
   *
   * La ficha arrastrada sale de su fila (queda un fantasma bajo el cursor) y
   * la fila destino le abre un hueco. El puntero se captura en la raíz y no en
   * la ficha: la ficha se desmonta al salir de su fila y se llevaría la captura.
   */
  import { tick } from "svelte";
  import { flip } from "svelte/animate";
  import {
    placePillTool,
    type PillBucket,
    type PillLayout,
    type PillToolIds,
  } from "$core/pillTools";
  import { WHEEL_TOOLS, type ToolDef, type ToolId } from "$core/tools";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { MOTION, ms, prefersReducedMotion } from "$lib/motion";
  import {
    CUSTOMIZE_CHIP,
    CUSTOMIZE_GAP,
    dropSlot,
    type DropRow,
    type DropSlot,
  } from "./customizeDrop";

  let {
    layout,
    focus = null,
    onplace,
    onreset,
    ondone,
  }: {
    layout: PillLayout;
    /** La ficha desde la que se abrió (presión larga): arranca destacada. */
    focus?: ToolId | null;
    onplace: (next: PillToolIds) => void;
    onreset: () => void;
    ondone: () => void;
  } = $props();

  const BUCKETS: readonly PillBucket[] = ["ring", "more", "hidden"];
  const DRAG_THRESHOLD = 4;
  const REFUSE_MS = 360;

  let rootEl = $state<HTMLElement | null>(null);
  const zoneEls = $state<Record<PillBucket, HTMLElement | null>>({
    ring: null,
    more: null,
    hidden: null,
  });
  const gridEls = $state<Record<PillBucket, HTMLElement | null>>({
    ring: null,
    more: null,
    hidden: null,
  });

  type Drag = {
    id: ToolId;
    from: PillBucket;
    pointerId: number;
    sx: number;
    sy: number;
    moving: boolean;
    /** Fantasma, en coords de la raíz. */
    gx: number;
    gy: number;
    slot: DropSlot | null;
  };
  let drag = $state<Drag | null>(null);
  /** Ficha que no pudo ir donde se la soltó: rebota. */
  let refused = $state<ToolId | null>(null);
  let refusedTimer = 0;

  const lists = $derived({
    ring: layout.ring,
    more: layout.more,
    hidden: layout.hidden,
  });
  const ringLocked = $derived(layout.ring.length <= 1);
  const flipMs = $derived(prefersReducedMotion() ? 0 : ms(MOTION.islandOpen));
  const dragTool = $derived(drag?.moving ? localizeTool(toolOf(drag.id)) : null);

  const titles: Record<PillBucket, () => string> = {
    ring: () => t("pill.customizeRing"),
    more: () => t("pill.customizeMore"),
    hidden: () => t("pill.customizeHidden"),
  };

  function toolOf(id: ToolId): ToolDef {
    return WHEEL_TOOLS.find((tool) => tool.id === id) ?? WHEEL_TOOLS[0];
  }

  /** Sacar la última de la rueda la dejaría vacía. */
  function canGo(from: PillBucket, to: PillBucket): boolean {
    return !(from === "ring" && to !== "ring" && ringLocked);
  }

  type Cell = { key: string; tool: ToolDef | null };

  /** Lo que pinta una fila: sin la arrastrada y con el hueco donde caería. */
  function cellsFor(bucket: PillBucket): Cell[] {
    const d = drag;
    const tools = lists[bucket];
    if (!d?.moving) return tools.map((tool) => ({ key: tool.id, tool }));
    const cells: Cell[] = tools
      .filter((tool) => tool.id !== d.id)
      .map((tool) => ({ key: tool.id, tool }));
    if (d.slot?.bucket === bucket)
      cells.splice(d.slot.index, 0, { key: "slot", tool: null });
    return cells;
  }

  function rectOf(el: HTMLElement | null) {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.x, y: r.y, w: r.width, h: r.height };
  }

  /** Fuera no tiene orden propio: cae donde la pone el catálogo. */
  function hiddenIndex(id: ToolId): number {
    const at = WHEEL_TOOLS.findIndex((tool) => tool.id === id);
    return layout.hidden.filter(
      (tool) => tool.id !== id && WHEEL_TOOLS.findIndex((w) => w.id === tool.id) < at,
    ).length;
  }

  function slotUnder(d: Drag, x: number, y: number): DropSlot | null {
    const rows: DropRow[] = [];
    for (const bucket of BUCKETS) {
      const zone = rectOf(zoneEls[bucket]);
      const grid = rectOf(gridEls[bucket]);
      if (!zone || !grid) continue;
      const count = lists[bucket].filter((tool) => tool.id !== d.id).length;
      rows.push({ bucket, zone, grid, count });
    }
    const slot = dropSlot(rows, { x, y });
    if (!slot || !canGo(d.from, slot.bucket)) return null;
    if (slot.bucket === "hidden") return { bucket: "hidden", index: hiddenIndex(d.id) };
    return slot;
  }

  function sameIds(a: readonly ToolDef[], b: readonly ToolId[]): boolean {
    return a.length === b.length && a.every((tool, i) => tool.id === b[i]);
  }

  function refuse(id: ToolId): void {
    refused = id;
    window.clearTimeout(refusedTimer);
    refusedTimer = window.setTimeout(() => (refused = null), REFUSE_MS);
  }

  function place(id: ToolId, to: PillBucket, index?: number): boolean {
    const next = placePillTool(layout, id, to, index);
    if (!next) {
      refuse(id);
      return false;
    }
    if (!sameIds(layout.ring, next.ring) || !sameIds(layout.more, next.more)) {
      onplace(next);
    }
    return true;
  }

  function onChipDown(event: PointerEvent, tool: ToolDef, from: PillBucket): void {
    if (event.button !== 0 || drag) return;
    try {
      rootEl?.setPointerCapture(event.pointerId);
    } catch {
      // Puntero ya liberado: el arrastre no arranca.
      return;
    }
    drag = {
      id: tool.id,
      from,
      pointerId: event.pointerId,
      sx: event.clientX,
      sy: event.clientY,
      moving: false,
      gx: 0,
      gy: 0,
      slot: null,
    };
  }

  function onRootMove(event: PointerEvent): void {
    const d = drag;
    if (!d || event.pointerId !== d.pointerId) return;
    if (
      !d.moving &&
      Math.hypot(event.clientX - d.sx, event.clientY - d.sy) < DRAG_THRESHOLD
    ) {
      return;
    }
    d.moving = true;
    const root = rectOf(rootEl);
    if (root) {
      // El fantasma no sale de la cara: afuera el overlay deja de ser clicable.
      const half = CUSTOMIZE_CHIP / 2;
      d.gx = Math.max(
        0,
        Math.min(root.w - CUSTOMIZE_CHIP, event.clientX - root.x - half),
      );
      d.gy = Math.max(
        0,
        Math.min(root.h - CUSTOMIZE_CHIP, event.clientY - root.y - half),
      );
    }
    d.slot = slotUnder(d, event.clientX, event.clientY);
  }

  function onRootUp(event: PointerEvent): void {
    const d = drag;
    if (!d || event.pointerId !== d.pointerId) return;
    drag = null;
    if (rootEl?.hasPointerCapture(event.pointerId)) {
      rootEl.releasePointerCapture(event.pointerId);
    }
    if (!d.moving) return;
    if (!d.slot) {
      refuse(d.id);
      return;
    }
    place(d.id, d.slot.bucket, d.slot.index);
  }

  function onRootCancel(event: PointerEvent): void {
    if (drag && event.pointerId === drag.pointerId) drag = null;
  }

  async function onChipKey(
    event: KeyboardEvent,
    tool: ToolDef,
    bucket: PillBucket,
    index: number,
  ): Promise<void> {
    const row = BUCKETS.indexOf(bucket);
    let to = bucket;
    let at: number | undefined = index;
    if (event.key === "ArrowLeft") at = index - 1;
    else if (event.key === "ArrowRight") at = index + 1;
    else if (event.key === "ArrowUp") to = BUCKETS[row - 1] ?? bucket;
    else if (event.key === "ArrowDown") to = BUCKETS[row + 1] ?? bucket;
    else return;
    event.preventDefault();
    event.stopPropagation();
    if (to === bucket && (bucket === "hidden" || at < 0)) return;
    if (to !== bucket) at = undefined;
    if (!canGo(bucket, to)) {
      refuse(tool.id);
      return;
    }
    if (!place(tool.id, to, at)) return;
    await tick();
    rootEl?.querySelector<HTMLElement>(`[data-chip="${tool.id}"]`)?.focus();
  }

  $effect(() => {
    if (!focus || !rootEl) return;
    rootEl
      .querySelector<HTMLElement>(`[data-chip="${focus}"]`)
      ?.focus({ preventScroll: true });
  });

  $effect(() => () => window.clearTimeout(refusedTimer));
</script>

<div
  class="pc"
  role="group"
  aria-label={t("pill.customizeTitle")}
  class:is-dragging={drag?.moving}
  style="--pc-chip: {CUSTOMIZE_CHIP}px; --pc-gap: {CUSTOMIZE_GAP}px"
  bind:this={rootEl}
  onpointermove={onRootMove}
  onpointerup={onRootUp}
  onpointercancel={onRootCancel}
>
  <header class="pc-head">
    <div class="pc-titles">
      <h3 class="pc-title">{t("pill.customizeTitle")}</h3>
      <p class="pc-hint">{t("pill.customizeDrag")}</p>
    </div>
    <button type="button" class="pc-link" onclick={onreset}>
      {t("pill.customizeReset")}
    </button>
    <button type="button" class="pc-done" onclick={ondone}>
      {t("pill.customizeDone")}
    </button>
  </header>

  {#each BUCKETS as bucket (bucket)}
    {@const cells = cellsFor(bucket)}
    <section
      class="pc-row"
      class:is-hidden-row={bucket === "hidden"}
      class:is-target={drag?.moving && drag.slot?.bucket === bucket}
      class:is-refused={drag?.moving &&
        !drag.slot &&
        bucket !== "ring" &&
        drag.from === "ring" &&
        ringLocked}
      bind:this={zoneEls[bucket]}
    >
      <h4 class="pc-label">
        {titles[bucket]()}
        <span class="pc-count">{lists[bucket].length}</span>
      </h4>
      <ul class="pc-grid" bind:this={gridEls[bucket]}>
        {#each cells as cell, i (cell.key)}
          <li class="pc-cell" animate:flip={{ duration: flipMs }}>
            {#if cell.tool}
              {@const shown = localizeTool(cell.tool)}
              {@const tool = cell.tool}
              <button
                type="button"
                class="pc-chip"
                class:is-focus={focus === tool.id}
                class:is-refused={refused === tool.id}
                data-chip={tool.id}
                title={shown.label}
                aria-label={t("pill.customizeChip", { label: shown.label })}
                onpointerdown={(e) => onChipDown(e, tool, bucket)}
                onkeydown={(e) => void onChipKey(e, tool, bucket, i)}
              >
                <ToolIcon id={tool.id} size={18} strokeWidth={1.6} />
              </button>
            {:else}
              <span class="pc-slot" aria-hidden="true"></span>
            {/if}
          </li>
        {/each}
        {#if cells.length === 0}
          <li class="pc-empty">{t("pill.customizeDropHere")}</li>
        {/if}
      </ul>
    </section>
  {/each}

  {#if ringLocked}
    <p class="pc-note">{t("pill.customizeLastOne")}</p>
  {/if}

  {#if drag?.moving && dragTool}
    <span
      class="pc-ghost"
      style="left: {drag.gx}px; top: {drag.gy}px"
      aria-hidden="true"
    >
      <ToolIcon id={dragTool.id} size={18} strokeWidth={1.6} />
    </span>
  {/if}
</div>

<style>
  .pc {
    position: relative;
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 0.5rem;
    min-height: 0;
    padding: 0.625rem 0.75rem 0.75rem;
    user-select: none;
    touch-action: none;
  }

  .pc.is-dragging {
    cursor: grabbing;
  }

  .pc-head {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .pc-titles {
    flex: 1 1 auto;
    min-width: 0;
  }

  .pc-title {
    margin: 0;
    color: var(--text);
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .pc-hint {
    margin: 0;
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .pc-link,
  .pc-done {
    flex: 0 0 auto;
    border: 0;
    border-radius: 999px;
    font-size: 0.75rem;
    cursor: pointer;
    transition:
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  .pc-link {
    padding: 0.25rem 0.375rem;
    background: transparent;
    color: var(--faint);
  }

  .pc-link:hover {
    color: var(--text);
  }

  .pc-done {
    padding: 0.3125rem 0.75rem;
    background: color-mix(in sRGB, var(--text) 12%, transparent);
    color: var(--text);
    font-weight: 600;
  }

  .pc-done:hover {
    background: color-mix(in sRGB, var(--text) 18%, transparent);
  }

  .pc-link:focus-visible,
  .pc-done:focus-visible,
  .pc-chip:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .pc-row {
    display: flex;
    flex-direction: column;
    gap: 0.3125rem;
    padding: 0.3125rem 0.375rem 0.375rem;
    border-radius: var(--radius-sm);
    transition: background var(--duration-medium) var(--ease-smooth-out);
  }

  .pc-row.is-target {
    background: color-mix(in sRGB, var(--text) 6%, transparent);
  }

  .pc-row.is-refused {
    opacity: 0.45;
  }

  .pc-label {
    display: flex;
    align-items: baseline;
    gap: 0.375rem;
    margin: 0;
    color: var(--faint);
    font-size: 0.6875rem;
    font-weight: 500;
  }

  .pc-count {
    font-variant-numeric: tabular-nums;
    opacity: 0.7;
  }

  .pc-grid {
    display: flex;
    flex-wrap: wrap;
    gap: var(--pc-gap);
    min-height: var(--pc-chip);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pc-cell {
    display: block;
    width: var(--pc-chip);
    height: var(--pc-chip);
  }

  .pc-chip {
    display: grid;
    width: 100%;
    height: 100%;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: color-mix(in sRGB, var(--text) 8%, transparent);
    color: var(--muted);
    cursor: grab;
    place-items: center;
    transition:
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .pc-chip:hover {
    background: color-mix(in sRGB, var(--text) 14%, transparent);
    color: var(--text);
    transform: scale(1.08);
  }

  .is-hidden-row .pc-chip {
    background: transparent;
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--text) 14%, transparent);
    color: var(--faint);
  }

  /* La que abrió el editor: un pulso para que el ojo la encuentre. */
  .pc-chip.is-focus {
    animation: pc-found 900ms var(--ease-smooth-out) 1;
  }

  .pc-chip.is-refused {
    animation: pc-refuse 360ms var(--ease-smooth-out) 1;
  }

  .pc-slot {
    display: block;
    width: 100%;
    height: 100%;
    border-radius: 999px;
    box-shadow: inset 0 0 0 1.5px color-mix(in sRGB, var(--text) 32%, transparent);
    animation: pc-slot-in var(--duration-medium) var(--ease-smooth-out) backwards;
  }

  .pc-empty {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    justify-content: center;
    height: var(--pc-chip);
    border-radius: 999px;
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--text) 12%, transparent);
    color: var(--faint);
    font-size: 0.6875rem;
  }

  .pc-ghost {
    display: grid;
    position: absolute;
    z-index: 2;
    width: var(--pc-chip);
    height: var(--pc-chip);
    border-radius: 999px;
    background: color-mix(in sRGB, var(--text) 22%, var(--surface));
    box-shadow: 0 6px 18px rgb(0 0 0 / 28%);
    color: var(--text);
    pointer-events: none;
    place-items: center;
    transform: scale(1.12);
  }

  .pc-note {
    margin: 0;
    color: var(--warn);
    font-size: 0.6875rem;
  }

  @keyframes pc-found {
    0%,
    100% {
      transform: scale(1);
    }

    40% {
      transform: scale(1.18);
      background: color-mix(in sRGB, var(--text) 20%, transparent);
    }
  }

  @keyframes pc-refuse {
    0%,
    100% {
      transform: translateX(0);
    }

    25% {
      transform: translateX(-3px);
    }

    50% {
      transform: translateX(3px);
    }

    75% {
      transform: translateX(-2px);
    }
  }

  @keyframes pc-slot-in {
    from {
      transform: scale(0.6);
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .pc-chip,
    .pc-chip.is-focus,
    .pc-chip.is-refused,
    .pc-slot {
      transition: none;
      animation: none;
    }
  }
</style>
