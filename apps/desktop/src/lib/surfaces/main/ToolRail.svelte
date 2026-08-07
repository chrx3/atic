<script lang="ts">
  /**
   * Picker de herramientas: arco que sale del borde izquierdo + cards a la derecha.
   *
   * El cuerpo es un círculo con centro fuera de pantalla (sale del borde). Las
   * cards flotan aparte (hueco > REACH). El hover que muestra ±2 solo cuenta
   * cuando el mouse está sobre la rueda, no sobre las cards.
   */
  import { untrack } from "svelte";
  import { runToolAction, toolAction } from "$core/toolActions";
  import { TOOLS, type ToolId } from "$core/tools";
  import { playWheelTick } from "$core/uiSound";
  import { PICKER_CELL_PROD_MIN, pickerLab } from "$lib/dev/pickerLab.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import { SquareArrowOutUpRight } from "$lib/icons";
  import { boxShape, pillShape, type Rect } from "$liquid/geometry";
  import { RectTracker } from "$liquid/measure.svelte";
  import Skin from "$liquid/Skin.svelte";
  import Button from "$ui/Button.svelte";
  import Icon from "$ui/Icon.svelte";

  let {
    activeTool,
    onSelect,
    onOpenDetail,
  }: {
    activeTool: ToolId;
    onSelect: (tool: ToolId) => void;
    onOpenDetail: (tool: ToolId) => void;
  } = $props();

  const N = TOOLS.length;
  const SPAN_IDLE = 1;
  const SPAN_HOVER = 2;
  const SPAN_MAX = SPAN_HOVER;
  const DROP = 40;
  const PILL_W = 80;
  const PILL_H = 40;
  const CARD_RADIUS = 16;
  /** Cuánto se acercan a la rueda las cards secundarias (px). */
  const COLD_NEST = 52;
  const DRAG_STEP_PX = 48;
  const CLICK_SLOP = 6;

  function lerp(a: number, b: number, t: number): number {
    return a + (b - a) * t;
  }

  /** 1 en el centro, 0 lejos — curva suave para morph de tamaño/opacidad. */
  function prominenceOf(dist: number): number {
    const t = Math.max(0, Math.min(1, 1 - dist));
    return t * t * (3 - 2 * t);
  }

  /** Alto de la ventana de contenido sobre el que se afinaron las perillas. */
  const REF_CONTENT_H = 560;

  // Perillas del picker lab (persistidas en localStorage; defaults = producción).
  // Con el lab cerrado nunca se usa cell < PICKER_CELL_PROD_MIN: el costo SDF
  // va con 1/cell² y con blend/cards actuales un cell de 2 congela la UI.
  const tune = $derived({
    blend: pickerLab.blend,
    cell: pickerLab.open
      ? pickerLab.cell
      : Math.max(pickerLab.cell, PICKER_CELL_PROD_MIN),
    cardFloat: pickerLab.cardFloat,
    pitchPad: pickerLab.pitchPad,
    heightFill: pickerLab.heightFill,
    hotX: pickerLab.hotX,
    hotXExpanded: pickerLab.hotXExpanded,
    stepDeg: pickerLab.stepDeg,
    stepDegExpanded: pickerLab.stepDegExpanded,
    cardHotW: pickerLab.cardHotW,
    cardHotH: pickerLab.cardHotH,
    cardColdW: pickerLab.cardColdW,
    cardColdH: pickerLab.cardColdH,
  });
  const CARD_HOT_W = $derived(tune.cardHotW);
  const CARD_HOT_H = $derived(tune.cardHotH);
  const CARD_COLD_W = $derived(tune.cardColdW);
  const CARD_COLD_H = $derived(tune.cardColdH);
  const CARD_FLOAT = $derived(tune.cardFloat);
  const CARD_MIN_PITCH = $derived(CARD_HOT_H / 2 + CARD_COLD_H / 2 + tune.pitchPad);

  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  let height = $state(560);
  /** Objetivo booleano del hover sobre la rueda. */
  let expanded = $state(false);
  /**
   * 0 = idle, 1 = expandido. Anima aparte del `visual` del scroll.
   * Abrir ≈ `--duration-fast` (invita); cerrar ≈ `--duration-quick` (se aparta).
   */
  let expandT = $state(0);
  let expandRaf = 0;

  let target = $state(0);
  let visual = $state(0);
  let raf = 0;
  let gate = false;

  let dragging = $state(false);
  let dragMoved = false;
  let dragPointerId = -1;
  let dragOriginY = 0;
  let dragOriginVisual = 0;
  let pendingClickDelta: number | null = null;
  /** Índice redondeado que ya sonó; evita ticks al montar o al re-sincronizar. */
  let soundedIndex = 0;
  let soundReady = false;

  /**
   * Escala del conjunto según el alto disponible.
   *
   * Las perillas del lab se afinaron sobre una ventana de contenido de ~560px.
   * En pantalla completa sin esto el radio crece (huecos enormes entre slots) y
   * la card hot se ve chica en el centro; en ventana baja se apretaba todo.
   * Acá: el radio se ancla a la altura de referencia y el contenido (cards)
   * escala con el alto, así la escena se ve igual en cualquier tamaño.
   */
  const layoutScale = $derived(
    Math.min(1.5, Math.max(0.62, Math.max(height, 320) / REF_CONTENT_H)),
  );

  const tracker = new RectTracker();
  /** Siempre ±2 en el DOM; los extremos se revelan con `expandT`. */
  const span = SPAN_MAX;

  function mod(n: number): number {
    return ((n % N) + N) % N;
  }

  /** Un click por paso de herramienta (scroll, drag o clic en drop/card). */
  function noteStep(v: number) {
    const i = mod(Math.round(v));
    if (!soundReady) {
      soundedIndex = i;
      return;
    }
    if (i === soundedIndex) return;
    soundedIndex = i;
    playWheelTick();
  }

  function toolIndex(id: ToolId): number {
    const i = TOOLS.findIndex((tool) => tool.id === id);
    return i < 0 ? 0 : i;
  }

  const geometry = $derived.by(() => {
    const h = Math.max(height, 320);
    const t = tune;
    const stepDeg = lerp(t.stepDeg, t.stepDegExpanded, expandT);
    const step = (stepDeg * Math.PI) / 180;
    // Radio anclado a la ventana de referencia, no al alto real: en pantalla
    // completa el arco no se estira; solo se centra verticalmente.
    const designPitch = Math.max(
      CARD_MIN_PITCH,
      (REF_CONTENT_H * t.heightFill) / (2 * SPAN_MAX),
    );
    const R = Math.max(designPitch / Math.sin(step), REF_CONTENT_H * 0.42);
    // Centro fuera a la izquierda: el arco sale del borde.
    const hotX = lerp(t.hotX, t.hotXExpanded, expandT);
    const wheelCx = hotX - R;
    const wheelCy = h / 2;
    const columnLeft = hotX + PILL_W / 2 + CARD_FLOAT * layoutScale;
    // Zona hit sigue el hotX animado (crece/encoge con la rueda).
    const railHitW = hotX + PILL_W / 2 + 16;
    return { R, step, wheelCx, wheelCy, hotX, columnLeft, railHitW };
  });

  type Spot = {
    key: string;
    cardKey: string;
    delta: number;
    tool: (typeof TOOLS)[number];
    x: number;
    y: number;
    hot: boolean;
    prominence: number;
    opacity: number;
    blur: number;
    dropW: number;
    dropH: number;
    cardW: number;
    cardH: number;
    cardX: number;
    cardY: number;
  };

  const spots = $derived.by((): Spot[] => {
    const { R, step, wheelCx, wheelCy, columnLeft } = geometry;
    const base = Math.round(visual);
    const frac = visual - base;
    const out: Spot[] = [];
    for (let delta = -span; delta <= span; delta++) {
      const outer = Math.abs(delta) > SPAN_IDLE;
      // Idle: no pintar ±2. En el morph, revelar con expandT (fade + blur).
      if (outer && expandT < 0.001) continue;
      const a = (delta - frac) * step;
      const dist = Math.abs(delta - frac);
      const prominence = prominenceOf(dist);
      const hot = dist < 0.5;
      const reveal = outer ? expandT : 1;
      const dropW = lerp(DROP, PILL_W, prominence);
      const dropH = lerp(DROP, PILL_H, prominence);
      const x = wheelCx + R * Math.cos(a);
      const y = wheelCy + R * Math.sin(a);
      const cardW = lerp(CARD_COLD_W, CARD_HOT_W, prominence) * layoutScale;
      const cardH = lerp(CARD_COLD_H, CARD_HOT_H, prominence) * layoutScale;
      // Secundarias se acercan a la rueda; la principal queda en la columna.
      const cardX = columnLeft - (1 - prominence) * COLD_NEST * layoutScale;
      const cardY = y - cardH / 2;
      out.push({
        key: `d${delta}`,
        cardKey: `c${delta}`,
        delta,
        tool: TOOLS[mod(base + delta)],
        x,
        y,
        hot,
        prominence,
        opacity: lerp(0.42, 1, prominence) * reveal,
        blur: reduceMotion
          ? 0
          : (1 - prominence) * 2.5 + (outer ? (1 - expandT) * 2 : 0),
        dropW,
        dropH,
        cardW,
        cardH,
        cardX,
        cardY,
      });
    }
    return out;
  });

  const trackBody = (el: HTMLElement) => tracker.track("body", el);
  const trackFns: Record<string, (el: HTMLElement) => () => void> = {};
  for (let delta = -SPAN_MAX; delta <= SPAN_MAX; delta++) {
    const dropId = `d${delta}`;
    const cardId = `c${delta}`;
    trackFns[dropId] = (el: HTMLElement) => tracker.track(dropId, el);
    trackFns[cardId] = (el: HTMLElement) => tracker.track(cardId, el);
  }
  function trackOf(key: string) {
    return trackFns[key];
  }

  const shapes = $derived.by(() => {
    void tracker.rects;
    const out = [];
    const body = tracker.rects.body as Rect | undefined;
    if (body) out.push(pillShape(body));
    for (const spot of spots) {
      const drop = tracker.rects[spot.key] as Rect | undefined;
      if (drop) out.push(pillShape(drop));
      const card = tracker.rects[spot.cardKey] as Rect | undefined;
      if (card) out.push(boxShape(card, Math.min(CARD_RADIUS, card.h / 2)));
    }
    return out;
  });

  function syncSelect() {
    noteStep(visual);
    const id = TOOLS[mod(Math.round(visual))].id;
    if (id !== activeTool) onSelect(id);
  }

  function stopAnim() {
    if (raf) {
      cancelAnimationFrame(raf);
      raf = 0;
    }
  }

  function stopExpandAnim() {
    if (expandRaf) {
      cancelAnimationFrame(expandRaf);
      expandRaf = 0;
    }
  }

  function tickAnim() {
    raf = 0;
    const diff = target - visual;
    if (reduceMotion || Math.abs(diff) < 0.002) {
      visual = target;
      syncSelect();
      tracker.wake();
      return;
    }
    visual += diff * 0.22;
    // Tick al cruzar el medio del paso (mismo instante que el flip hot),
    // no al pedir el paso: evita beep desfasado del morph.
    noteStep(visual);
    tracker.wake();
    if (Math.abs(target - visual) < 0.002) {
      visual = target;
      syncSelect();
    } else {
      raf = requestAnimationFrame(tickAnim);
    }
  }

  function kick() {
    if (!raf) raf = requestAnimationFrame(tickAnim);
  }

  /**
   * Morph idle↔hover de la rueda.
   *
   * Asimetría transitions-polish: abrir invita (`--duration-fast` ≈ 0.18/frame
   * a 60 Hz), cerrar se aparta (`--duration-quick` ≈ 0.28/frame).
   */
  function tickExpand() {
    expandRaf = 0;
    const want = expanded ? 1 : 0;
    if (reduceMotion) {
      expandT = want;
      tracker.wake();
      return;
    }
    const diff = want - expandT;
    if (Math.abs(diff) < 0.002) {
      expandT = want;
      tracker.wake();
      return;
    }
    expandT += diff * (expanded ? 0.18 : 0.28);
    tracker.wake();
    if (Math.abs(want - expandT) < 0.002) {
      expandT = want;
    } else {
      expandRaf = requestAnimationFrame(tickExpand);
    }
  }

  function kickExpand() {
    if (!expandRaf) expandRaf = requestAnimationFrame(tickExpand);
  }

  function goBy(delta: number) {
    if (!delta) return;
    target = Math.round(target) + delta;
    kick();
  }

  function regionStillScrolls(start: EventTarget | null, dy: number): boolean {
    let node = start instanceof Element ? start : null;
    while (node && node !== document.documentElement) {
      if (node instanceof HTMLElement) {
        const style = getComputedStyle(node);
        const oy = style.overflowY;
        if (
          (oy === "auto" || oy === "scroll" || oy === "overlay") &&
          node.scrollHeight > node.clientHeight + 1
        ) {
          const top = node.scrollTop <= 0;
          const bottom = node.scrollTop + node.clientHeight >= node.scrollHeight - 1;
          if (dy < 0 && !top) return true;
          if (dy > 0 && !bottom) return true;
        }
      }
      node = node.parentElement;
    }
    return false;
  }

  function onWindowWheel(event: WheelEvent) {
    if (document.querySelector("dialog[open]")) return;
    const el = document.activeElement;
    if (el instanceof HTMLElement && el.closest("input, textarea, [contenteditable]")) {
      return;
    }
    const dy = event.deltaY;
    if (!dy && !event.deltaX) return;
    if (regionStillScrolls(event.target, dy || event.deltaX)) return;

    event.preventDefault();
    if (gate) return;
    const dir = Math.sign(dy) || Math.sign(event.deltaX);
    if (!dir) return;
    gate = true;
    goBy(dir);
    window.setTimeout(
      () => {
        gate = false;
      },
      reduceMotion ? 0 : 120,
    );
  }

  function overRail(event: PointerEvent, root: HTMLElement): boolean {
    const x = event.clientX - root.getBoundingClientRect().left;
    return x < geometry.railHitW;
  }

  function onPointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    const t = event.target as Element | null;
    if (t?.closest?.("[data-card-action], button.card-config")) return;

    const hit = t?.closest?.("button.drop") ?? t?.closest?.("[data-delta].tool-card");
    pendingClickDelta = hit ? Number((hit as HTMLElement).dataset.delta) : null;

    dragging = true;
    dragMoved = false;
    dragPointerId = event.pointerId;
    dragOriginY = event.clientY;
    dragOriginVisual = visual;
    stopAnim();
    target = visual;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }

  function onPointerMove(event: PointerEvent) {
    const root = event.currentTarget as HTMLElement;
    if (!dragging) {
      const next = overRail(event, root);
      if (next !== expanded) {
        expanded = next;
        kickExpand();
      }
    }

    if (!dragging || event.pointerId !== dragPointerId) return;
    const dy = event.clientY - dragOriginY;
    if (!dragMoved && Math.abs(dy) < CLICK_SLOP) return;
    dragMoved = true;
    pendingClickDelta = null;
    // Arrastrar hacia abajo mueve el contenido hacia abajo (el de arriba entra al centro).
    const next = dragOriginVisual - dy / DRAG_STEP_PX;
    visual = next;
    target = next;
    noteStep(next);
    tracker.wake();
  }

  function onPointerUp(event: PointerEvent) {
    if (!dragging || event.pointerId !== dragPointerId) return;
    dragging = false;
    dragPointerId = -1;
    try {
      (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
    } catch {
      /* ya liberado */
    }

    const root = event.currentTarget as HTMLElement;
    const next = root.matches(":hover") && overRail(event, root);
    if (next !== expanded) {
      expanded = next;
      kickExpand();
    }

    if (!dragMoved && pendingClickDelta != null && !Number.isNaN(pendingClickDelta)) {
      goBy(pendingClickDelta);
      pendingClickDelta = null;
      return;
    }
    pendingClickDelta = null;
    target = Math.round(visual);
    kick();
  }

  function onPointerLeave() {
    if (!dragging && expanded) {
      expanded = false;
      kickExpand();
    }
  }

  async function onAction(id: ToolId, event: MouseEvent) {
    event.stopPropagation();
    try {
      const result = await runToolAction(id);
      if (result === "openedDetail") onOpenDetail(id);
    } catch (error) {
      toastError(error);
    }
  }

  function onConfig(id: ToolId, event: MouseEvent) {
    event.stopPropagation();
    onOpenDetail(id);
  }

  $effect(() => {
    const want = toolIndex(activeTool);
    untrack(() => {
      if (dragging) return;
      const now = mod(Math.round(target));
      if (want === now) return;
      let diff = want - now;
      if (diff > N / 2) diff -= N;
      if (diff < -N / 2) diff += N;
      target = Math.round(target) + diff;
      kick();
    });
  });

  function bindRoot(el: HTMLElement) {
    tracker.origin = el;
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height;
      if (h && h > 0) {
        height = h;
        tracker.wake();
      }
    });
    ro.observe(el);
    height = el.clientHeight || 560;
    const start = toolIndex(activeTool);
    target = start;
    visual = start;
    soundedIndex = start;
    soundReady = true;
    tracker.wake();

    window.addEventListener("wheel", onWindowWheel, { passive: false });
    return () => {
      soundReady = false;
      ro.disconnect();
      tracker.stop();
      window.removeEventListener("wheel", onWindowWheel);
      stopAnim();
      stopExpandAnim();
    };
  }

  $effect(() => {
    void spots;
    void expandT;
    tracker.wake();
  });
</script>

<nav
  class="wheel"
  class:is-dragging={dragging}
  aria-label="Herramientas"
  {@attach bindRoot}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
  onpointerleave={onPointerLeave}
>
  <div
    class="wheel-body"
    style:width="{geometry.R * 2}px"
    style:height="{geometry.R * 2}px"
    style:left="{geometry.wheelCx - geometry.R}px"
    style:top="{geometry.wheelCy - geometry.R}px"
    {@attach trackBody}
    aria-hidden="true"
  ></div>

  <Skin {shapes} blend={tune.blend} cell={tune.cell} />

  <div class="ink-layer">
    {#each spots as spot (spot.key)}
      {@const action = toolAction(spot.tool.id)}
      <button
        type="button"
        class="drop"
        class:is-hot={spot.hot}
        data-delta={spot.delta}
        style:left="{spot.x - spot.dropW / 2}px"
        style:top="{spot.y - spot.dropH / 2}px"
        style:width="{spot.dropW}px"
        style:height="{spot.dropH}px"
        style:opacity={spot.opacity}
        style:--p={spot.prominence}
        title={spot.tool.label}
        aria-label={spot.tool.label}
        aria-current={spot.hot ? "page" : undefined}
        {@attach trackOf(spot.key)}
      >
        <span class="glyph" class:is-hot={spot.hot}>
          <ToolIcon id={spot.tool.id} size={20} />
        </span>
      </button>

      <div
        class="tool-card"
        class:is-hot={spot.hot}
        data-delta={spot.delta}
        style:left="{spot.cardX}px"
        style:top="{spot.cardY}px"
        style:width="{spot.cardW}px"
        style:height="{spot.cardH}px"
        style:opacity={spot.opacity}
        style:filter="blur({spot.blur}px)"
        style:--p={spot.prominence}
        style:--ls={layoutScale}
        {@attach trackOf(spot.cardKey)}
      >
        <div class="card-head">
          <span class="card-title">{spot.tool.label}</span>
          <div
            class="card-chrome"
            style:opacity={spot.prominence}
            style:pointer-events={spot.prominence > 0.55 ? "auto" : "none"}
          >
            <Button
              variant={action.danger ? "danger-solid" : "primary"}
              size="sm"
              loading={action.busy}
              data-card-action=""
              onclick={(e) => void onAction(spot.tool.id, e)}
            >
              {action.label}
            </Button>
          </div>
        </div>
        <div class="card-foot">
          <div class="card-copy">
            <p class="card-blurb is-short" style:opacity={1 - spot.prominence}>
              {spot.tool.short}
            </p>
            <p class="card-blurb is-full" style:opacity={spot.prominence}>
              {spot.tool.blurb}
            </p>
          </div>
          <button
            type="button"
            class="card-config"
            style:opacity={spot.prominence}
            style:pointer-events={spot.prominence > 0.55 ? "auto" : "none"}
            aria-label="Detalle y ajustes de {spot.tool.label}"
            title="Detalle y ajustes"
            tabindex={spot.prominence > 0.55 ? 0 : -1}
            onclick={(e) => onConfig(spot.tool.id, e)}
          >
            <Icon icon={SquareArrowOutUpRight} size={16} />
          </button>
        </div>
      </div>
    {/each}
  </div>
</nav>

<style>
  .wheel {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    height: 100%;
    overflow: hidden;
    isolation: isolate;
    touch-action: none;
    cursor: grab;
    user-select: none;
  }

  .wheel.is-dragging,
  .wheel:active {
    cursor: grabbing;
  }

  .wheel-body {
    position: absolute;
    border-radius: 50%;
    background: transparent;
    pointer-events: none;
  }

  .ink-layer {
    position: absolute;
    inset: 0;
    z-index: 1;
  }

  .drop {
    position: absolute;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: inherit;
    cursor: inherit;
    touch-action: none;
    /*
     * width/height/opacity los conduce el spring de `visual` cada frame.
     * Una transition CSS aquí desincroniza left/top (instantáneos) del tamaño
     * (atrasado) y se lee como salto justo al cruzar el paso / beep.
     */
  }

  .glyph {
    display: grid;
    place-items: center;
    color: color-mix(
      in oklab,
      var(--rb-muted) calc((1 - var(--p, 0)) * 100%),
      var(--rb-text) calc(var(--p, 0) * 100%)
    );
    pointer-events: none;
    /* 17/20 ≈ 0.85 → 1.05 en el centro; continuo con prominence. */
    transform: scale(calc(0.85 + 0.2 * var(--p, 0)));
  }

  .tool-card {
    position: absolute;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: calc(0.35rem * var(--ls, 1));
    /* Padding continuo por --p: evita flip cold→hot de layout al beep. */
    padding: calc((0.75rem + 0.2rem * var(--p, 0)) * var(--ls, 1))
      calc((0.9rem + 0.15rem * var(--p, 0)) * var(--ls, 1));
    border-radius: calc(16px * var(--ls, 1));
    background: transparent;
    color: inherit;
    cursor: inherit;
    box-sizing: border-box;
    /* Igual que .drop: geometría por JS; no pelear con transition de layout. */
  }

  .card-head,
  .card-foot {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .card-foot {
    align-items: flex-end;
    min-height: 0;
    flex: 1;
  }

  .card-chrome {
    flex-shrink: 0;
    /* --p cambia cada frame con el spring; sin transition de layout/filter. */
    transform: scale(calc(0.92 + 0.08 * var(--p, 0)));
    filter: blur(calc((1 - var(--p, 0)) * var(--blur-small, 2px)));
  }

  .card-title {
    font-size: calc((0.9rem + 0.15rem * var(--p, 0)) * var(--ls, 1));
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--rb-text);
    line-height: 1.2;
    text-wrap: balance;
  }

  .card-copy {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 2.4em;
  }

  .card-blurb {
    margin: 0;
    font-size: calc(0.8rem * var(--ls, 1));
    line-height: 1.35;
    color: var(--rb-muted);
    text-wrap: pretty;
  }

  .card-blurb.is-short {
    position: absolute;
    inset: 0;
    font-size: calc(0.75rem * var(--ls, 1));
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-blurb.is-full {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-config {
    position: relative;
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 2rem;
    height: 2rem;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 0.5rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .card-config::before {
    content: "";
    position: absolute;
    inset: -6px;
  }

  .card-config:hover {
    color: var(--rb-text);
    background: color-mix(in oklab, var(--rb-text) 8%, transparent);
  }

  .card-config:active {
    transform: scale(0.96);
  }

  @media (prefers-reduced-motion: reduce) {
    .card-config {
      transition: none;
    }

    .glyph {
      transform: none;
      color: var(--rb-muted);
    }

    .drop.is-hot .glyph,
    .glyph.is-hot {
      color: var(--rb-text);
    }

    .tool-card {
      filter: none !important;
    }

    .card-chrome {
      filter: none;
      transform: none;
    }
  }
</style>
