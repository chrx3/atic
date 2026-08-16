/**
 * Cuándo la pill se acopla a un borde y en qué se convierte.
 *
 * Es la decisión, no el dibujo: entra dónde está la pill y cómo son los
 * monitores, sale a qué borde engancha. Sin DOM, para poder comprobar las
 * reglas raras —el borde interior entre dos pantallas, la histéresis— sin
 * abrir la app y arrastrar a mano.
 *
 * Dos cosas que parecen detalles y son las que hacen que se sienta bien o mal:
 *
 * 1. **Solo bordes exteriores.** Con dos monitores pegados, el canto derecho
 *    del izquierdo no es un borde de pantalla: es la mitad del escritorio.
 *    Acoplar ahí deja la isla flotando en el medio, que es exactamente lo que
 *    el usuario no pidió.
 * 2. **Enganchar cuesta menos que soltar.** Con un solo umbral la pill tiembla
 *    entre acoplada y suelta con un movimiento de un píxel. `RELEASE` es más
 *    grande que `SNAP` a propósito.
 *
 * El acople usa el **área útil** (`work`), no los bounds: contra los bounds, el
 * borde de abajo queda debajo de la barra de tareas en Windows y del Dock en
 * macOS.
 */

import { workAreaOf, type Area, type Rect } from "$ipc/overlay";

export type DockEdge = "left" | "right" | "top" | "bottom";

/** Hueco (px) al que el arrastre engancha al borde. */
export const DOCK_SNAP_PX = 28;

/**
 * Cuánto hay que alejarla del borde para soltarla.
 *
 * Mayor que `DOCK_SNAP_PX`: si fueran iguales, quedarse justo en el umbral
 * haría que acoplarse y soltarse se alternaran con el temblor de la mano.
 */
export const DOCK_RELEASE_PX = 64;

/**
 * Bordes donde se permite acoplar.
 *
 * Los cuatro. Antes de tener `work` esto no se podía: abajo caía bajo la barra
 * de tareas y arriba, en macOS, bajo la barra de menú. Con el área útil los
 * cuatro son posiciones legítimas.
 */
export const DOCK_EDGES: readonly DockEdge[] = ["left", "right", "top", "bottom"];

/** Los bordes de la unión de todos los monitores. */
export function desktopBounds(areas: readonly Area[]): Rect | null {
  if (areas.length === 0) return null;
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const a of areas) {
    x0 = Math.min(x0, a.x);
    y0 = Math.min(y0, a.y);
    x1 = Math.max(x1, a.x + a.w);
    y1 = Math.max(y1, a.y + a.h);
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** El monitor bajo el centro del rect; si cae entre dos, el primero. */
export function areaFor(rect: Rect, areas: readonly Area[]): Area | null {
  if (areas.length === 0) return null;
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  return (
    areas.find(
      (a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h,
    ) ?? areas[0]
  );
}

/**
 * ¿Este borde del monitor da al vacío?
 *
 * Se compara contra la unión y no contra los vecinos: alcanza con saber si el
 * canto coincide con el del escritorio. La tolerancia cubre el redondeo de
 * pasar físicos a CSS con escalas fraccionarias (1.25 deja restos de 0.4 px).
 */
export function isOuterEdge(
  edge: DockEdge,
  area: Area,
  areas: readonly Area[],
  tolerance = 1,
): boolean {
  const desk = desktopBounds(areas);
  if (!desk) return false;
  switch (edge) {
    case "left":
      return Math.abs(area.x - desk.x) <= tolerance;
    case "right":
      return Math.abs(area.x + area.w - (desk.x + desk.w)) <= tolerance;
    case "top":
      return Math.abs(area.y - desk.y) <= tolerance;
    case "bottom":
      return Math.abs(area.y + area.h - (desk.y + desk.h)) <= tolerance;
  }
}

/** Distancia del rect a cada borde del área útil (negativa si se pasa). */
export function edgeGaps(
  rect: Rect,
  work: Rect,
): Record<DockEdge, number> {
  return {
    left: rect.x - work.x,
    right: work.x + work.w - (rect.x + rect.w),
    top: rect.y - work.y,
    bottom: work.y + work.h - (rect.y + rect.h),
  };
}

/**
 * Dónde queda la pill al acoplarse: pegada al borde, sin moverla a lo largo.
 *
 * Solo se fija el eje perpendicular. Corregir también el otro haría que la
 * isla saltara a un lugar que el usuario no eligió — soltó a esa altura, tiene
 * que quedarse a esa altura.
 */
export function dockedPoint(
  edge: DockEdge,
  rect: Rect,
  work: Rect,
): { x: number; y: number } {
  const x = Math.min(Math.max(rect.x, work.x), work.x + work.w - rect.w);
  const y = Math.min(Math.max(rect.y, work.y), work.y + work.h - rect.h);
  switch (edge) {
    case "left":
      return { x: work.x, y };
    case "right":
      return { x: work.x + work.w - rect.w, y };
    case "top":
      return { x, y: work.y };
    case "bottom":
      return { x, y: work.y + work.h - rect.h };
  }
}

export type DockCandidate = {
  edge: DockEdge;
  /** Monitor al que pertenece el borde. */
  area: Area;
  /** Posición ya pegada. */
  at: { x: number; y: number };
};

/**
 * ¿A qué borde engancha la pill que quedó acá?
 *
 * `null` = se queda flotando. Gana el borde más cercano, y solo si es exterior
 * y está dentro de `snapPx`.
 */
export function dockCandidate(
  rect: Rect,
  areas: readonly Area[],
  opts: {
    snapPx?: number;
    edges?: readonly DockEdge[];
  } = {},
): DockCandidate | null {
  const snapPx = opts.snapPx ?? DOCK_SNAP_PX;
  const edges = opts.edges ?? DOCK_EDGES;
  const area = areaFor(rect, areas);
  if (!area) return null;
  const work = workAreaOf(area);
  const gaps = edgeGaps(rect, work);

  let best: DockEdge | null = null;
  let bestGap = Infinity;
  for (const edge of edges) {
    if (!isOuterEdge(edge, area, areas)) continue;
    const gap = gaps[edge];
    // Negativo = ya se pasó del borde; cuenta como pegada, no como lejos.
    const dist = Math.max(gap, 0);
    if (dist <= snapPx && dist < bestGap) {
      best = edge;
      bestGap = dist;
    }
  }
  if (!best) return null;
  return { edge: best, area, at: dockedPoint(best, rect, work) };
}

/**
 * ¿Ya se alejó lo suficiente como para soltarla del borde?
 *
 * Se mide solo contra el borde donde está acoplada: alejarse en paralelo al
 * canto no es querer soltarla, es moverla a lo largo del mismo borde.
 */
export function shouldUndock(
  rect: Rect,
  edge: DockEdge,
  areas: readonly Area[],
  releasePx = DOCK_RELEASE_PX,
): boolean {
  const area = areaFor(rect, areas);
  if (!area) return true;
  const gap = edgeGaps(rect, workAreaOf(area))[edge];
  return gap > releasePx;
}

/**
 * ¿La posición guardada estaba acoplada?
 *
 * Sirve para restaurar el estado al arrancar sin guardar un campo nuevo: si el
 * hogar quedó a ras de un borde exterior, es que estaba acoplada ahí. Evita
 * migrar el formato de `pill_home`, que es solo un punto.
 */
export function dockedEdgeAt(
  rect: Rect,
  areas: readonly Area[],
  tolerance = 2,
): DockEdge | null {
  return dockCandidate(rect, areas, { snapPx: tolerance })?.edge ?? null;
}

/** El eje contra el que la isla se aplana. */
export function dockAxis(edge: DockEdge): "x" | "y" {
  return edge === "left" || edge === "right" ? "x" : "y";
}
