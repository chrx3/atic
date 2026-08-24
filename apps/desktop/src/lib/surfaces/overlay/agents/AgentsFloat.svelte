<script lang="ts">
  /**
   * Shell del float: bubble + liquid + surfaces + drag.
   * Por ahora hospeda la misma demo visual que la ventana principal.
   */
  import { onMount } from "svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import {
    agentsAlwaysOnTop,
    hideAgentsWindow,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
    saveAgentsBubbleSize,
  } from "$ipc/agents";
  import { onOverlayDismiss, overlayWorkAreas, workAreaOf } from "$ipc/overlay";
  import type { Area } from "$ipc/overlay";
  import type { BubbleOpen } from "$core/types";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { publishEmergeSkin } from "$surfaces/overlay/floatEmergeSkin";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import { Bubble, BUBBLE_MIN_H, BUBBLE_MIN_W } from "$surfaces/overlay/bubble.svelte";
  import { createBubbleDrag } from "$surfaces/overlay/bubbleDrag";
  import { placeBesidePill } from "$surfaces/overlay/floatPlace";
  import AgentLauncher from "$features/agents/AgentLauncher.svelte";
  import {
    isAgentsDismissSuppressed,
  } from "$surfaces/overlay/agents/dismissGuard";
  import { toasts } from "$domain/toasts.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { MOTION, ms } from "$lib/motion";
  import { armOpenDismissGrace, isOpenDismissGrace } from "$surfaces/overlay/openDismissGrace";

  const BUBBLE_CORNER = 26;
  const POSITION_STORAGE_KEY = "atic.agents.consolePosition";
  const POSITION_MARGIN = 12;
  let workAreas = $state<Area[]>([]);

  type SavedPosition = { x: number; y: number };

  function readSavedPosition(): SavedPosition | null {
    try {
      const value = JSON.parse(
        localStorage.getItem(POSITION_STORAGE_KEY) ?? "null",
      ) as Partial<SavedPosition> | null;
      if (!value || !Number.isFinite(value.x) || !Number.isFinite(value.y)) return null;
      return { x: value.x!, y: value.y! };
    } catch {
      return null;
    }
  }

  function savePosition() {
    const a = bubble.anchor;
    if (!a) return;
    try {
      localStorage.setItem(
        POSITION_STORAGE_KEY,
        JSON.stringify({ x: Math.round(a.x), y: Math.round(a.y) }),
      );
    } catch {
      /* El float sigue funcionando aunque el storage esté bloqueado. */
    }
  }

  function positionInWorkspace(
    pill: { x: number; y: number; w: number; h: number },
    panel: { w: number; h: number },
    preferred: SavedPosition | null,
  ): SavedPosition {
    const point = preferred
      ? { x: preferred.x + panel.w / 2, y: preferred.y + panel.h / 2 }
      : { x: pill.x + pill.w / 2, y: pill.y + pill.h / 2 };
    const pillPoint = { x: pill.x + pill.w / 2, y: pill.y + pill.h / 2 };
    const contains = (area: Area, p: { x: number; y: number }) =>
      p.x >= area.x &&
      p.x <= area.x + area.w &&
      p.y >= area.y &&
      p.y <= area.y + area.h;
    const area =
      workAreas.find((candidate) => contains(candidate, point)) ??
      workAreas.find((candidate) => contains(candidate, pillPoint)) ??
      workAreas[0] ??
      ({ x: 0, y: 0, w: window.innerWidth, h: window.innerHeight } satisfies Area);
    const work = workAreaOf(area);
    const centered = {
      x: work.x + (work.w - panel.w) / 2,
      y: work.y + (work.h - panel.h) / 2,
    };
    const wanted = preferred ?? centered;
    const minX = work.x + POSITION_MARGIN;
    const minY = work.y + POSITION_MARGIN;
    const maxX = Math.max(minX, work.x + work.w - panel.w - POSITION_MARGIN);
    const maxY = Math.max(minY, work.y + work.h - panel.h - POSITION_MARGIN);
    return {
      x: Math.round(Math.min(Math.max(wanted.x, minX), maxX)),
      y: Math.round(Math.min(Math.max(wanted.y, minY), maxY)),
    };
  }

  function placeFromPill(a: BubbleOpen) {
    if (!bubble.alive || !bubble.shown) armOpenDismissGrace();
    const pill = surfaces.live["pill-skin"] ?? surfaces.live["pill"];
    if (!pill) {
      bubble.place({
        ...a,
        ...positionInWorkspace(
          { x: a.x, y: a.y, w: 1, h: 1 },
          { w: a.w, h: a.h },
          readSavedPosition(),
        ),
      });
      return;
    }
    const beside = placeBesidePill(
      pill,
      { w: a.w, h: a.h },
      { corner: BUBBLE_CORNER, work: workAreas },
    );
    const current =
      bubble.shown && bubble.anchor
        ? { x: bubble.anchor.x, y: bubble.anchor.y }
        : readSavedPosition();
    bubble.place({
      ...a,
      ...beside,
      ...positionInWorkspace(pill, { w: a.w, h: a.h }, current),
    });
  }

  type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

  const bubble = new Bubble();

  let bubEl = $state<HTMLElement | null>(null);
  const { startDrag, endDrag } = createBubbleDrag(bubble, () => bubEl, {
    onEnd: savePosition,
  });

  /** Estirar el globo desde cualquier borde o esquina. */
  let resize: {
    edge: ResizeEdge;
    ox: number;
    oy: number;
    ax: number;
    ay: number;
    ow: number;
    oh: number;
    pointerId: number;
  } | null = null;

  function startResize(edge: ResizeEdge, event: PointerEvent) {
    if (event.button !== 0 || !bubble.anchor) return;
    event.preventDefault();
    event.stopPropagation();
    const a = bubble.anchor;
    resize = {
      edge,
      ox: event.clientX,
      oy: event.clientY,
      ax: a.x,
      ay: a.y,
      ow: a.w,
      oh: a.h,
      pointerId: event.pointerId,
    };
    try {
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    } catch {
      /* ignore */
    }
    window.addEventListener("pointermove", onResizeMove);
    window.addEventListener("pointerup", endResize);
    window.addEventListener("pointercancel", endResize);
  }

  function onResizeMove(event: PointerEvent) {
    const r = resize;
    if (!r || !bubble.anchor) return;
    const dx = event.clientX - r.ox;
    const dy = event.clientY - r.oy;
    const east = r.edge.includes("e");
    const west = r.edge.includes("w");
    const south = r.edge.includes("s");
    const north = r.edge.includes("n");

    let x = r.ax;
    let y = r.ay;
    let w = r.ow;
    let h = r.oh;

    if (east) w = Math.max(BUBBLE_MIN_W, r.ow + dx);
    if (west) {
      w = Math.max(BUBBLE_MIN_W, r.ow - dx);
      x = r.ax + r.ow - w;
    }
    if (south) h = Math.max(BUBBLE_MIN_H, r.oh + dy);
    if (north) {
      h = Math.max(BUBBLE_MIN_H, r.oh - dy);
      y = r.ay + r.oh - h;
    }

    bubble.setFrame(x, y, w, h);
  }

  function endResize() {
    const r = resize;
    if (!r) return;
    resize = null;
    window.removeEventListener("pointermove", onResizeMove);
    window.removeEventListener("pointerup", endResize);
    window.removeEventListener("pointercancel", endResize);
    const a = bubble.anchor;
    if (a) {
      void saveAgentsBubbleSize(a.w, a.h);
      savePosition();
    }
  }

  function close() {
    if (!bubble.shown) return;
    endDrag();
    endResize();
    bubble.hide();
    void hideAgentsWindow();
    agents.watch(null);
  }

  /**
   * El contenido NO se desmonta al cerrar el float: las PTYs viven en Rust y
   * el xterm conserva su scrollback mientras el componente exista. Cerrar la
   * ventana solo la oculta (`is-off`); reabrir desde la pill muestra las
   * consolas tal como estaban. Se pierden al cerrar cada pestaña o al apagar
   * la app — no al esconder la ventana.
   */
  let everAlive = $state(false);
  $effect(() => {
    if (bubble.alive) everAlive = true;
  });

  /** Cierre por intención (clic afuera / Esc). Respeta pin y diálogos nativos. */
  function tryAutoClose() {
    if (!bubble.shown || isAgentsDismissSuppressed() || isOpenDismissGrace()) return;
    void agentsAlwaysOnTop()
      .then((pinned) => {
        if (pinned || isAgentsDismissSuppressed() || !bubble.shown) return;
        close();
      })
      .catch(() => {
        /* sin lectura del pin, no cerrar */
      });
  }

  $effect(() => {
    if (!bubble.alive || !bubEl) {
      liquid.publish("agents", []);
      return;
    }
    // Seguir el morph visual: el ancla lógica no escala al cerrar.
    void bubble.shown;
    void bubble.anchor;
    return publishEmergeSkin("agents", bubEl, BUBBLE_CORNER);
  });

  $effect(() => {
    // Registrar en cuanto hay DOM (`alive`), no esperar `.is-shown`: sin
    // hit-rect el overlay sigue click-through (clics al app de atrás).
    // No depender de `shown` acá: re-add al morph reinicia el registro y
    // puede publicar un frame sin `agents` en la lista.
    if (!bubEl || !bubble.alive) return;
    const stop = surfaces.add("agents", bubEl);
    void surfaces.flush();
    return stop;
  });

  $effect(() => {
    if (!bubble.alive || !bubble.shown) return;
    void bubble.anchor;
    void surfaces.recoverHits();
    const t = window.setTimeout(() => {
      void surfaces.recoverHits();
    }, ms(MOTION.floatOpen) + 48);
    return () => window.clearTimeout(t);
  });

  $effect(() => {
    void bubble.anchor;
    void surfaces.dragging;
    if (surfaces.dragging) return;
    surfaces.schedule();
  });

  onMount(() => {
    applyTheme(readCachedTheme());
    void overlayWorkAreas()
      .then((areas) => {
        workAreas = areas;
      })
      .catch(() => {
        workAreas = [];
      });
    const un: Promise<() => void>[] = [
      onAgentsBubbleAnchor((a) => placeFromPill(a)),
      onAgentsBubbleDismiss(() => {
        bubble.hide();
      }),
      // Clic afuera (Raw Input → overlay-dismiss). Pin / diálogo nativo → no.
      onOverlayDismiss(() => {
        tryAutoClose();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !bubble.shown) return;
      // Esc: cierre explícito solo si no está fijada (panel sticky).
      if (isAgentsDismissSuppressed()) return;
      // Consola PTY / xterm: AgentsDemo maneja Esc (cerrar consola); no cerrar el float.
      const t = e.target as HTMLElement | null;
      if (t?.closest?.(".console, .xterm")) return;
      e.preventDefault();
      void agentsAlwaysOnTop()
        .then((pinned) => {
          if (!pinned && bubble.shown) close();
        })
        .catch(() => {
          /* sin pin, no cerrar */
        });
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      endDrag();
      endResize();
      for (const p of un) void p.then((fn) => fn());
      liquid.publish("agents", []);
      agents.watch(null);
    };
  });
</script>

{#if bubble.alive || everAlive}
  <div
    class="af float-emerge"
    class:is-shown={bubble.shown}
    class:is-off={!bubble.alive}
    data-agents-float
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    bind:this={bubEl}
  >
    <AgentLauncher onHeaderPointerDown={startDrag} onClose={close} />
    <!-- local: sin popover/viewport; el overlay es fullscreen y el toast
         quedaría abajo de toda la pantalla, lejos del bubble. -->
    <ToastStack
      placement="local"
      items={toasts.items}
      onDismiss={(id) => toasts.dismiss(id)}
    />
    <!-- Agarraderas: los 4 bordes y las 4 esquinas. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-n" data-no-drag onpointerdown={(e) => startResize("n", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-s" data-no-drag onpointerdown={(e) => startResize("s", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-e" data-no-drag onpointerdown={(e) => startResize("e", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-w" data-no-drag onpointerdown={(e) => startResize("w", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-ne" data-no-drag onpointerdown={(e) => startResize("ne", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-nw" data-no-drag onpointerdown={(e) => startResize("nw", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-se" data-no-drag onpointerdown={(e) => startResize("se", e)}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="grip grip-sw" data-no-drag onpointerdown={(e) => startResize("sw", e)}></div>
  </div>
{/if}

<style>
  .af {
    position: absolute;
    z-index: var(--z-overlay-float);
    left: var(--x);
    top: var(--y);
    width: var(--w);
    height: var(--h);
    box-sizing: border-box;
    /* Columna: el contenido (launcher / consola) llena con flex:1. Sin esto
       la consola colapsa a la altura de su barra (~34px). */
    display: flex;
    flex-direction: column;
    border-radius: 1.625rem;
    /* visible: el PickerMenu del composer abre hacia arriba */
    overflow: visible;
  }

  /* Oculto pero vivo: las PTYs siguen corriendo. Sin pointer-events ni
     visibilidad, el overlay no arma clics sobre una ventana que no está. */
  .af.is-off {
    visibility: hidden;
    pointer-events: none;
  }

  .grip {
    position: absolute;
    /* Bajo el header (.top-acts z 9): no robar pin/cerrar. */
    z-index: 7;
    background: transparent;
  }

  .grip-n,
  .grip-s {
    left: 10px;
    right: 10px;
    height: 6px;
    cursor: ns-resize;
  }

  .grip-n {
    top: 0;
  }

  .grip-s {
    bottom: 0;
  }

  .grip-e,
  .grip-w {
    /* Debajo del header (~top-ctrl + padding): no tapar pin / Bypass / X. */
    top: 40px;
    bottom: 10px;
    width: 6px;
    cursor: ew-resize;
  }

  .grip-e {
    right: 0;
  }

  .grip-w {
    left: 0;
  }

  .grip-ne,
  .grip-nw,
  .grip-se,
  .grip-sw {
    width: 14px;
    height: 14px;
  }

  .grip-nw {
    top: 0;
    left: 0;
    cursor: nwse-resize;
  }

  .grip-ne {
    top: 0;
    right: 0;
    cursor: nesw-resize;
  }

  .grip-sw {
    bottom: 0;
    left: 0;
    cursor: nesw-resize;
  }

  .grip-se {
    bottom: 0;
    right: 0;
    cursor: nwse-resize;
  }
</style>
