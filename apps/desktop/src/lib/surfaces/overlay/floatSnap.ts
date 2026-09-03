/**
 * Snap tipo Windows para un float arrastrado.
 *
 * Manda el marco de la ventana, no el cursor. Un canto exterior engancha
 * apenas el globo lo toca o se cuelga. Un canto compartido (junta entre
 * monitores) engancha solo en una franja estrecha: así puedes partir a la
 * mitad contra la otra pantalla o dejar la ventana a caballo si ya cruzaste.
 */

export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; w: number; h: number };

export type SnapKind =
  | "max"
  | "left"
  | "right"
  | "bottom"
  | "tl"
  | "tr"
  | "bl"
  | "br";

export type SnapHit = { kind: SnapKind; work: Rect };

export type EdgeRole = "outer" | "inner";

export type SnapEdges = {
  left: EdgeRole;
  right: EdgeRole;
  top: EdgeRole;
  bottom: EdgeRole;
};

export type SnapSides = {
  left: boolean;
  right: boolean;
  top: boolean;
  bottom: boolean;
};

/** Distancia al canto para mitades / maximizar. */
export const SNAP_EDGE_PX = 28;
/** Zona de esquina: las dos distancias tienen que entrar. */
export const SNAP_CORNER_PX = 48;
/** Franja de la junta entre monitores: un poco más ancha que el canto exterior. */
export const SNAP_INNER_PX = 48;
/** Dos monitores se tocan si sus cantos caen dentro de esto. */
export const SNAP_SEAM_PX = 24;

const ALL_OUTER: SnapEdges = {
  left: "outer",
  right: "outer",
  top: "outer",
  bottom: "outer",
};

function rangesOverlap(a0: number, a1: number, b0: number, b1: number): boolean {
  return a0 < b1 && b0 < a1;
}

function rectsOverlap(a: Rect, b: Rect): boolean {
  return (
    rangesOverlap(a.x, a.x + a.w, b.x, b.x + b.w) &&
    rangesOverlap(a.y, a.y + a.h, b.y, b.y + b.h)
  );
}

function overlapArea(a: Rect, b: Rect): number {
  const x0 = Math.max(a.x, b.x);
  const y0 = Math.max(a.y, b.y);
  const x1 = Math.min(a.x + a.w, b.x + b.w);
  const y1 = Math.min(a.y + a.h, b.y + b.h);
  return Math.max(0, x1 - x0) * Math.max(0, y1 - y0);
}

function workOf(area: { x: number; y: number; w: number; h: number; work?: Rect }): Rect {
  return area.work ?? area;
}

/** Qué cantos de un monitor no tienen otro pegado. */
export function outerSides(
  area: Rect,
  all: readonly Rect[],
  seamPx: number = SNAP_SEAM_PX,
): SnapSides {
  let left = true;
  let right = true;
  let top = true;
  let bottom = true;
  for (const other of all) {
    if (other === area) continue;
    if (
      Math.abs(other.x + other.w - area.x) <= seamPx &&
      rangesOverlap(area.y, area.y + area.h, other.y, other.y + other.h)
    ) {
      left = false;
    }
    if (
      Math.abs(other.x - (area.x + area.w)) <= seamPx &&
      rangesOverlap(area.y, area.y + area.h, other.y, other.y + other.h)
    ) {
      right = false;
    }
    if (
      Math.abs(other.y + other.h - area.y) <= seamPx &&
      rangesOverlap(area.x, area.x + area.w, other.x, other.x + other.w)
    ) {
      top = false;
    }
    if (
      Math.abs(other.y - (area.y + area.h)) <= seamPx &&
      rangesOverlap(area.x, area.x + area.w, other.x, other.x + other.w)
    ) {
      bottom = false;
    }
  }
  return { left, right, top, bottom };
}

export function monitorEdges(
  area: Rect,
  all: readonly Rect[],
  seamPx: number = SNAP_SEAM_PX,
): SnapEdges {
  const outer = outerSides(area, all, seamPx);
  return {
    left: outer.left ? "outer" : "inner",
    right: outer.right ? "outer" : "inner",
    top: outer.top ? "outer" : "inner",
    bottom: outer.bottom ? "outer" : "inner",
  };
}

/** Exterior: toca o se cuelga. Interior: franja ±px alrededor de la junta. */
function nearEdge(dist: number, role: EdgeRole, px: number): boolean {
  if (role === "outer") return dist <= px;
  const band = Math.max(px, SNAP_INNER_PX);
  return dist >= -band && dist <= band;
}

/**
 * Destino según el marco contra un área.
 */
export function snapKindAt(
  frame: Rect,
  work: Rect,
  edgePx: number = SNAP_EDGE_PX,
  cornerPx: number = SNAP_CORNER_PX,
  edges: SnapEdges = ALL_OUTER,
): SnapKind | null {
  const left = frame.x - work.x;
  const right = work.x + work.w - (frame.x + frame.w);
  const top = frame.y - work.y;
  const bottom = work.y + work.h - (frame.y + frame.h);
  let nearL = nearEdge(left, edges.left, cornerPx);
  let nearR = nearEdge(right, edges.right, cornerPx);
  let nearT = nearEdge(top, edges.top, cornerPx);
  let nearB = nearEdge(bottom, edges.bottom, cornerPx);
  // Un globo más ancho/alto que el monitor toca los dos cantos: no es snap.
  if (nearL && nearR) {
    nearL = false;
    nearR = false;
  }
  if (nearT && nearB) {
    nearT = false;
    nearB = false;
  }
  if (nearT && nearL) return "tl";
  if (nearT && nearR) return "tr";
  if (nearB && nearL) return "bl";
  if (nearB && nearR) return "br";
  if (nearEdge(left, edges.left, edgePx)) return "left";
  if (nearEdge(right, edges.right, edgePx)) return "right";
  if (nearEdge(top, edges.top, edgePx)) return "max";
  if (nearEdge(bottom, edges.bottom, edgePx)) return "bottom";
  return null;
}

/** Monitor cuyo canto está chocando el marco, si hay. */
export function snapTarget(
  frame: Rect,
  areas: readonly { x: number; y: number; w: number; h: number; work?: Rect }[],
  edgePx: number = SNAP_EDGE_PX,
  cornerPx: number = SNAP_CORNER_PX,
): SnapHit | null {
  if (areas.length === 0) return null;
  const bounds = areas.map((a) => ({ x: a.x, y: a.y, w: a.w, h: a.h }));
  const cx = frame.x + frame.w / 2;
  const cy = frame.y + frame.h / 2;
  let best: (SnapHit & { score: number }) | null = null;
  for (let i = 0; i < areas.length; i++) {
    const area = areas[i];
    const work = workOf(area);
    if (!rectsOverlap(frame, work) && !rectsOverlap(frame, bounds[i])) continue;
    const kind = snapKindAt(
      frame,
      work,
      edgePx,
      cornerPx,
      monitorEdges(bounds[i], bounds),
    );
    if (!kind) continue;
    const overlap = overlapArea(frame, work);
    const home =
      cx >= work.x &&
      cx <= work.x + work.w &&
      cy >= work.y &&
      cy <= work.y + work.h;
    const score = overlap + (home ? 1_000_000 : 0);
    if (!best || score > best.score) best = { kind, work, score };
  }
  return best ? { kind: best.kind, work: best.work } : null;
}

/** Marco de destino dentro del área útil, con el mismo aire que el maximize. */
export function snapFrame(kind: SnapKind, work: Rect, margin: number): Rect {
  const inner = {
    x: work.x + margin,
    y: work.y + margin,
    w: Math.max(0, work.w - margin * 2),
    h: Math.max(0, work.h - margin * 2),
  };
  const hw = Math.max(0, (inner.w - margin) / 2);
  const hh = Math.max(0, (inner.h - margin) / 2);
  switch (kind) {
    case "max":
      return inner;
    case "left":
      return { x: inner.x, y: inner.y, w: hw, h: inner.h };
    case "right":
      return { x: inner.x + inner.w - hw, y: inner.y, w: hw, h: inner.h };
    case "bottom":
      return { x: inner.x, y: inner.y + inner.h - hh, w: inner.w, h: hh };
    case "tl":
      return { x: inner.x, y: inner.y, w: hw, h: hh };
    case "tr":
      return { x: inner.x + inner.w - hw, y: inner.y, w: hw, h: hh };
    case "bl":
      return { x: inner.x, y: inner.y + inner.h - hh, w: hw, h: hh };
    case "br":
      return {
        x: inner.x + inner.w - hw,
        y: inner.y + inner.h - hh,
        w: hw,
        h: hh,
      };
  }
}

/** Área útil que contiene el cursor, o la primera. */
export function workUnderCursor(
  cursor: Point,
  areas: readonly { x: number; y: number; w: number; h: number; work?: Rect }[],
): Rect | null {
  if (areas.length === 0) return null;
  const hit =
    areas.find(
      (a) =>
        cursor.x >= a.x &&
        cursor.x <= a.x + a.w &&
        cursor.y >= a.y &&
        cursor.y <= a.y + a.h,
    ) ?? areas[0];
  if (!hit) return null;
  return hit.work ?? hit;
}
