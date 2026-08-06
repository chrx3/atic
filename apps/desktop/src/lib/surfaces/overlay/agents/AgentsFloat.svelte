<script lang="ts">
  /**
   * Shell del float: bubble + liquid + surfaces + drag.
   * Por ahora hospeda la misma demo visual que la ventana principal.
   */
  import { onMount } from "svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import {
    hideAgentsWindow,
    onAgentsBubbleAnchor,
    onAgentsBubbleDismiss,
  } from "$ipc/agents";
  import {
    onOverlayDismiss,
    overlayCursor,
    overlayWorkAreas,
    type Area,
  } from "$ipc/overlay";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import { boxShape } from "$lib/liquid/geometry";
  import { liquid } from "$surfaces/overlay/group.svelte";
  import { surfaces } from "$surfaces/overlay/surfaces.svelte";
  import { Bubble } from "$surfaces/overlay/bubble.svelte";
  import { MARGIN } from "$surfaces/overlay/contract";
  import AgentsDemo from "$features/agents/AgentsDemo.svelte";

  const BUBBLE_CORNER = 26;
  const DRAG_THRESHOLD = 4;

  const bubble = new Bubble();

  let bubEl = $state<HTMLElement | null>(null);
  /**
   * Arrastre anclado al cursor global (Rust), no solo a `pointermove`.
   *
   * Cerca del borde la barra de tareas (u otra ventana always-on-top) se queda
   * con el mouse: el webview deja de recibir eventos y el globo se “corta”
   * a mitad de camino. `overlayCursor` sigue leyendo la posición real.
   */
  let drag: {
    /** Null hasta el primer tick: lo siembra el cursor de Rust, no el evento. */
    cx: number | null;
    cy: number | null;
    ax: number;
    ay: number;
    pointerId: number;
  } | null = null;
  let dragMoved = false;
  let dragRaf = 0;
  let workAreas: Area[] = [];

  function clampToWork(x: number, y: number, w: number, h: number): {
    x: number;
    y: number;
  } {
    if (workAreas.length === 0) return { x, y };
    const cx = x + w / 2;
    const cy = y + h / 2;
    const area =
      workAreas.find(
        (a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h,
      ) ?? workAreas[0];
    if (!area) return { x, y };
    const maxX = Math.max(area.x + area.w - w - MARGIN, area.x + MARGIN);
    const maxY = Math.max(area.y + area.h - h - MARGIN, area.y + MARGIN);
    return {
      x: Math.min(Math.max(x, area.x + MARGIN), maxX),
      y: Math.min(Math.max(y, area.y + MARGIN), maxY),
    };
  }

  function startDrag(event: PointerEvent) {
    if (event.button !== 0 || !bubble.anchor) return;
    if (
      (event.target as HTMLElement).closest(
        "button, a, input, textarea, select, label, [data-no-drag], [role='listbox'], [role='menu']",
      )
    ) {
      return;
    }
    event.preventDefault();
    const a = bubble.anchor;
    // El origen NO sale del evento del DOM. `clientX` mide contra la ventana, y
    // traducirlo al espacio del overlay obliga a confiar en dónde cree el CSS
    // que está `.ov` — un dato que llega por evento desde Rust y que llega
    // tarde justo cuando la ventana se acaba de reencuadrar. Arrastrar en ese
    // hueco mandaba la consola al borde del monitor izquierdo.
    //
    // Lo siembra el primer tick con el cursor de Rust: el mismo reloj y el
    // mismo espacio con los que se sigue el resto del gesto.
    drag = {
      cx: null,
      cy: null,
      ax: a.x,
      ay: a.y,
      pointerId: event.pointerId,
    };
    dragMoved = false;
    try {
      bubEl?.setPointerCapture(event.pointerId);
    } catch {
      /* ignore */
    }
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
    void overlayWorkAreas()
      .then((areas) => {
        workAreas = areas;
      })
      .catch(() => {
        workAreas = [];
      });
    if (!dragRaf) dragRaf = requestAnimationFrame(() => void tickDrag());
  }

  async function tickDrag() {
    dragRaf = 0;
    const d = drag;
    const a = bubble.anchor;
    if (!d || !a) return;

    const cur = await overlayCursor().catch(() => null);
    if (cur && drag === d) {
      // Primer cuadro: es la semilla, no un movimiento.
      if (d.cx === null || d.cy === null) {
        d.cx = cur.x;
        d.cy = cur.y;
      } else {
        const dx = cur.x - d.cx;
        const dy = cur.y - d.cy;
        if (!dragMoved && Math.hypot(dx, dy) > DRAG_THRESHOLD) {
          dragMoved = true;
          // Arma el overlay entero al tiro (flush), antes del siguiente move.
          surfaces.dragging = true;
        }
        if (dragMoved) {
          const next = clampToWork(d.ax + dx, d.ay + dy, a.w, a.h);
          bubble.moveTo(next.x, next.y);
        }
      }
    }

    if (drag) {
      dragRaf = requestAnimationFrame(() => void tickDrag());
    }
  }

  function endDrag() {
    if (!drag) return;
    const pointerId = drag.pointerId;
    drag = null;
    dragMoved = false;
    if (dragRaf) {
      cancelAnimationFrame(dragRaf);
      dragRaf = 0;
    }
    surfaces.dragging = false;
    try {
      if (bubEl?.hasPointerCapture(pointerId)) {
        bubEl.releasePointerCapture(pointerId);
      }
    } catch {
      /* ignore */
    }
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointercancel", endDrag);
  }

  function close() {
    if (!bubble.shown) return;
    endDrag();
    bubble.hide();
    void hideAgentsWindow();
    agents.watch(null);
  }

  $effect(() => {
    if (!bubble.alive || !bubble.anchor) {
      liquid.publish("agents", []);
      return;
    }
    liquid.publish("agents", [boxShape(bubble.anchor, BUBBLE_CORNER)]);
  });

  $effect(() =>
    bubEl && bubble.shown ? surfaces.add("agents", bubEl) : undefined,
  );

  $effect(() => {
    void bubble.anchor;
    surfaces.schedule();
  });

  onMount(() => {
    applyTheme(readCachedTheme());
    const un: Promise<() => void>[] = [
      onAgentsBubbleAnchor((a) => bubble.place(a)),
      onAgentsBubbleDismiss(() => {
        bubble.hide();
      }),
      onOverlayDismiss(() => {
        if (bubble.shown) close();
      }),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && bubble.shown) {
        e.preventDefault();
        close();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      endDrag();
      for (const p of un) void p.then((fn) => fn());
      liquid.publish("agents", []);
      agents.watch(null);
    };
  });
</script>

{#if bubble.alive}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="af float-emerge"
    class:is-shown={bubble.shown}
    data-side={bubble.anchor?.side ?? "top"}
    style={bubble.vars}
    bind:this={bubEl}
    onpointerdown={startDrag}
  >
    <button
      type="button"
      class="close"
      data-no-drag
      onclick={close}
      aria-label="Cerrar"
      title="Cerrar · Esc"
    >
      <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
        <path
          d="M6 6l12 12M18 6L6 18"
          fill="none"
          stroke="currentColor"
          stroke-width="2.2"
          stroke-linecap="round"
        />
      </svg>
    </button>
    <AgentsDemo variant="float" />
  </div>
{/if}

<style>
  .af {
    position: absolute;
    z-index: 2;
    left: var(--x);
    top: var(--y);
    width: var(--w);
    height: var(--h);
    box-sizing: border-box;
    /* visible: el PickerMenu del composer abre hacia arriba */
    overflow: visible;
  }

  .close {
    position: absolute;
    z-index: 8;
    top: 0.5rem;
    right: 0.5rem;
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: color-mix(in srgb, var(--rb-surface) 88%, transparent);
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .close:hover {
    color: var(--rb-text);
    background: var(--rb-surface-2);
  }

  .close:active {
    transform: scale(0.96);
  }

  @media (prefers-reduced-motion: reduce) {
    .close:active {
      transform: none;
    }
  }
</style>
