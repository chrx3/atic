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

function unionAreas(areas: Area[]): Area {
  let x0 = areas[0].x;
  let y0 = areas[0].y;
  let x1 = areas[0].x + areas[0].w;
  let y1 = areas[0].y + areas[0].h;
  for (let i = 1; i < areas.length; i++) {
    const a = areas[i];
    x0 = Math.min(x0, a.x);
    y0 = Math.min(y0, a.y);
    x1 = Math.max(x1, a.x + a.w);
    y1 = Math.max(y1, a.y + a.h);
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

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
  const hit = workAreas.find(
    (a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h,
  );
  const area = hit ?? unionAreas(workAreas);
  const maxX = Math.max(area.x + area.w - w - MARGIN, area.x + MARGIN);
  const maxY = Math.max(area.y + area.h - h - MARGIN, area.y + MARGIN);
  return {
    x: Math.min(Math.max(x, area.x + MARGIN), maxX),
    y: Math.min(Math.max(y, area.y + MARGIN), maxY),
  };
}

function shouldSkip(event: PointerEvent, skip: string): boolean {
  // `closest` basta en el caso normal; `composedPath` cubre SVG/shadow por si
  // el target no sube al `button` / `[data-no-drag]` como esperamos.
  const path = event.composedPath();
  for (const node of path) {
    if (node instanceof Element && node.matches(skip)) return true;
  }
  const t = event.target;
  return t instanceof Element && !!t.closest(skip);
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
    window.removeEventListener("pointerup", endDrag, true);
    window.removeEventListener("pointercancel", endDrag, true);
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
    if (shouldSkip(event, skip)) return;
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
    window.addEventListener("pointerup", endDrag, true);
    window.addEventListener("pointercancel", endDrag, true);
    surfaces.dragging = true;
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
