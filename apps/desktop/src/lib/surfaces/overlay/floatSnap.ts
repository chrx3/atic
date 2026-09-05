/**
 * Snap tipo Windows para un float arrastrado.
 *
 * Manda el cursor, no el marco. Un globo alto toca el techo mucho antes
 * de que el mouse llegue: si engancháramos por bordes de ventana, agrandar
 * y partir se dispararían a mitad de camino. Un canto exterior engancha
 * cuando el cursor lo toca o se cuelga. Un canto compartido (junta entre
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

function workOf(area: { x: number; y: number; w: number; h: number; work?: Rect }): Rect {
  return area.work ?? area;
}

function pointInRect(p: Point, r: Rect): boolean {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

/** Distancia² al rectángulo: 0 si está adentro. */
function dist2ToRect(p: Point, r: Rect): number {
  const dx = p.x < r.x ? r.x - p.x : p.x > r.x + r.w ? p.x - (r.x + r.w) : 0;
  const dy = p.y < r.y ? r.y - p.y : p.y > r.y + r.h ? p.y - (r.y + r.h) : 0;
  return dx * dx + dy * dy;
}

/** Monitor que contiene el cursor, o el más cercano si se colgó del canto. */
function areaAtCursor(
  cursor: Point,
  areas: readonly { x: number; y: number; w: number; h: number }[],
): { x: number; y: number; w: number; h: number } | null {
  if (areas.length === 0) return null;
  const inside = areas.find((a) => pointInRect(cursor, a));
  if (inside) return inside;
  let best = areas[0];
  let bestD = Infinity;
  for (const a of areas) {
    const d = dist2ToRect(cursor, a);
    if (d < bestD) {
      bestD = d;
      best = a;
    }
  }
  return best;
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
 * Destino según el cursor contra un área.
 */
export function snapKindAt(
  cursor: Point,
  work: Rect,
  edgePx: number = SNAP_EDGE_PX,
  cornerPx: number = SNAP_CORNER_PX,
  edges: SnapEdges = ALL_OUTER,
): SnapKind | null {
  const left = cursor.x - work.x;
  const right = work.x + work.w - cursor.x;
  const top = cursor.y - work.y;
  const bottom = work.y + work.h - cursor.y;
  let nearL = nearEdge(left, edges.left, cornerPx);
  let nearR = nearEdge(right, edges.right, cornerPx);
  let nearT = nearEdge(top, edges.top, cornerPx);
  let nearB = nearEdge(bottom, edges.bottom, cornerPx);
  // Un área más estrecha que dos zonas de esquina no es un snap de canto.
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

/** Monitor cuyo canto está bajo el cursor, si hay. */
export function snapTarget(
  cursor: Point,
  areas: readonly { x: number; y: number; w: number; h: number; work?: Rect }[],
  edgePx: number = SNAP_EDGE_PX,
  cornerPx: number = SNAP_CORNER_PX,
): SnapHit | null {
  if (areas.length === 0) return null;
  const bounds = areas.map((a) => ({ x: a.x, y: a.y, w: a.w, h: a.h }));
  const area = areaAtCursor(cursor, bounds);
  if (!area) return null;
  const index = bounds.indexOf(area);
  const work = workOf(areas[index] ?? area);
  const kind = snapKindAt(
    cursor,
    work,
    edgePx,
    cornerPx,
    monitorEdges(area, bounds),
  );
  return kind ? { kind, work } : null;
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
