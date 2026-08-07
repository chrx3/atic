/**
 * Arrastre de un globo overlay anclado al cursor de Rust.
 *
 * Cerca del borde la barra de tareas (u otra ventana always-on-top) se queda
 * con el mouse: el webview deja de recibir eventos y el globo se “corta”
 * a mitad de camino. `overlayCursor` sigue leyendo la posición real.
 */
import {
  overlayCursor,
  overlayWorkAreas,
  type Area,
} from "$ipc/overlay";
import { MARGIN } from "./contract";
import type { Bubble } from "./bubble.svelte";
import { surfaces } from "./surfaces.svelte";

const DRAG_THRESHOLD = 4;

const DEFAULT_SKIP =
  "button, a, input, textarea, select, label, [data-no-drag], [data-selectable], [role='listbox'], [role='menu']";

export type BubbleDrag = {
  startDrag: (event: PointerEvent) => void;
  endDrag: () => void;
};

function clampToWork(
  workAreas: Area[],
  x: number,
  y: number,
  w: number,
  h: number,
): { x: number; y: number } {
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

export function createBubbleDrag(
  bubble: Bubble,
  getEl: () => HTMLElement | null,
  options?: { skip?: string },
): BubbleDrag {
  const skip = options?.skip ?? DEFAULT_SKIP;

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
    const el = getEl();
    try {
      if (el?.hasPointerCapture(pointerId)) {
        el.releasePointerCapture(pointerId);
      }
    } catch {
      /* ignore */
    }
    window.removeEventListener("pointerup", endDrag);
    window.removeEventListener("pointercancel", endDrag);
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
          const next = clampToWork(workAreas, d.ax + dx, d.ay + dy, a.w, a.h);
          bubble.moveTo(next.x, next.y);
        }
      }
    }

    if (drag) {
      dragRaf = requestAnimationFrame(() => void tickDrag());
    }
  }

  function startDrag(event: PointerEvent) {
    if (event.button !== 0 || !bubble.anchor) return;
    if ((event.target as HTMLElement).closest(skip)) return;
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
    const el = getEl();
    try {
      el?.setPointerCapture(event.pointerId);
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

  return { startDrag, endDrag };
}
