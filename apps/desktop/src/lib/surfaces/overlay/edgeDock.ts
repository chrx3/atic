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
    areas.find((a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h) ??
    areas[0]
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
export function edgeGaps(rect: Rect, work: Rect): Record<DockEdge, number> {
  return {
    left: rect.x - work.x,
    right: work.x + work.w - (rect.x + rect.w),
    top: rect.y - work.y,
    bottom: work.y + work.h - (rect.y + rect.h),
  };
}

/**
 * ¿Este canto del área útil deja un recorte de SO (barra de tareas, Dock,
 * menú)? Ahí la pared SDF queda DENTRO del overlay y se lee como un segundo
 * blob. El truco del techo funciona porque `work.y === area.y` y la pared
 * cae fuera de la ventana.
 */
export function edgeHasReservedInset(edge: DockEdge, area: Area, eps = 2): boolean {
  const work = workAreaOf(area);
  switch (edge) {
    case "top":
      return work.y - area.y > eps;
    case "bottom":
      return area.y + area.h - (work.y + work.h) > eps;
    case "left":
      return work.x - area.x > eps;
    case "right":
      return area.x + area.w - (work.x + work.w) > eps;
  }
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

/** Hueco (px) al que el arrastre engancha un imán (centro o centro de un canto). */
export const MAGNET_SNAP_PX = 96;

/** Monitor principal, o el primero si el SO no lo marcó. */
export function primaryArea(areas: readonly Area[]): Area | null {
  if (areas.length === 0) return null;
  return areas.find((a) => a.primary) ?? areas[0];
}

/** Punto de la pill pegada al centro de un canto del área útil. */
export function edgeCenterPoint(
  edge: DockEdge,
  size: { w: number; h: number },
  work: Rect,
): { x: number; y: number } {
  const x = Math.round(work.x + (work.w - size.w) / 2);
  const y = Math.round(work.y + (work.h - size.h) / 2);
  switch (edge) {
    case "left":
      return { x: work.x, y };
    case "right":
      return { x: work.x + work.w - size.w, y };
    case "top":
      return { x, y: work.y };
    case "bottom":
      return { x, y: work.y + work.h - size.h };
  }
}

/** Centro geométrico del área útil (flotante, no acoplada). */
export function screenCenterPoint(
  size: { w: number; h: number },
  work: Rect,
): { x: number; y: number } {
  return {
    x: Math.round(work.x + (work.w - size.w) / 2),
    y: Math.round(work.y + (work.h - size.h) / 2),
  };
}

/**
 * Hogar de la pill: canto de arriba, en el medio del monitor principal.
 */
export function defaultPillHome(
  size: { w: number; h: number },
  areas: readonly Area[],
): { at: { x: number; y: number }; edge: "top" } | null {
  const area = primaryArea(areas);
  if (!area) return null;
  return {
    at: edgeCenterPoint("top", size, workAreaOf(area)),
    edge: "top",
  };
}

/**
 * Hogar elegido: el canto donde el usuario acopló la pill, en qué monitor y a
 * qué altura de ese canto (`along`: dónde cae su CENTRO, 0 = inicio del área
 * útil, 1 = final).
 *
 * Se guarda así y no como un punto: el largo de la isla cambia con los avisos
 * y el origen del overlay cambia al enchufar o sacar un monitor. Y va por el
 * centro y no por la esquina porque al soltarla la caja todavía mide lo de la
 * barra, no lo de la pestaña: con la esquina, el hogar se corría medio largo.
 */
export type PillHome = { edge: DockEdge; along: number; area: Rect };

/** Cuánto puede moverse un monitor (redondeo de escala) y seguir siendo el mismo. */
const HOME_AREA_TOLERANCE = 2;

function sameRect(a: Rect, b: Rect, tolerance: number): boolean {
  return (
    Math.abs(a.x - b.x) <= tolerance &&
    Math.abs(a.y - b.y) <= tolerance &&
    Math.abs(a.w - b.w) <= tolerance &&
    Math.abs(a.h - b.h) <= tolerance
  );
}

/** El hogar que deja una pill acoplada en `edge` con esta caja. */
export function pillHomeFrom(
  edge: DockEdge,
  rect: Rect,
  areas: readonly Area[],
): PillHome | null {
  const area = areaFor(rect, areas);
  if (!area) return null;
  const work = workAreaOf(area);
  const alongX = dockAxis(edge) === "y";
  const length = alongX ? work.w : work.h;
  const center = alongX ? rect.x + rect.w / 2 - work.x : rect.y + rect.h / 2 - work.y;
  const along = length > 0 ? Math.min(1, Math.max(0, center / length)) : 0.5;
  return { edge, along, area: { x: area.x, y: area.y, w: area.w, h: area.h } };
}

/**
 * Dónde queda ese hogar hoy, para una caja de `size`.
 *
 * `null` si el monitor ya no está o si ese canto dejó de dar al vacío (se
 * pegó otra pantalla al lado): ahí manda el hogar por defecto.
 */
export function pillHomePoint(
  home: PillHome,
  size: { w: number; h: number },
  areas: readonly Area[],
): { at: { x: number; y: number }; edge: DockEdge } | null {
  const area = areas.find((a) => sameRect(a, home.area, HOME_AREA_TOLERANCE));
  if (!area || !isOuterEdge(home.edge, area, areas)) return null;
  const work = workAreaOf(area);
  const along = Math.min(1, Math.max(0, home.along));
  // El centro en su fracción, sin que la caja se salga del área útil.
  const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));
  const x = Math.round(
    clamp(work.x + along * work.w - size.w / 2, work.x, work.x + work.w - size.w),
  );
  const y = Math.round(
    clamp(work.y + along * work.h - size.h / 2, work.y, work.y + work.h - size.h),
  );
  switch (home.edge) {
    case "left":
      return { at: { x: work.x, y }, edge: "left" };
    case "right":
      return { at: { x: work.x + work.w - size.w, y }, edge: "right" };
    case "top":
      return { at: { x, y: work.y }, edge: "top" };
    case "bottom":
      return { at: { x, y: work.y + work.h - size.h }, edge: "bottom" };
  }
}

/** ¿Lo leído del almacenamiento tiene forma de hogar? Puede venir de otra versión. */
export function isPillHome(value: unknown): value is PillHome {
  if (!value || typeof value !== "object") return false;
  const v = value as Record<string, unknown>;
  const area = v.area as Record<string, unknown> | undefined;
  return (
    DOCK_EDGES.includes(v.edge as DockEdge) &&
    typeof v.along === "number" &&
    Number.isFinite(v.along) &&
    !!area &&
    ["x", "y", "w", "h"].every((k) => typeof area[k] === "number" && Number.isFinite(area[k]))
  );
}

/**
 * Monitor donde recentrar una isla acoplada tras un cambio de viewport.
 *
 * El recuadro chico de WebView2 deja la pill en coordenadas que, contra las
 * áreas reales, ya no caen en el canto. Si el monitor bajo la posición actual
 * no tiene ese borde exterior, se busca uno que sí: p.ej. `right` va al
 * monitor más a la derecha, no al hogar de arriba.
 */
function areaWithOuterEdge(
  edge: DockEdge,
  current: Rect,
  areas: readonly Area[],
): Area | null {
  const containing = areaFor(current, areas);
  if (containing && isOuterEdge(edge, containing, areas)) return containing;
  const primary = primaryArea(areas);
  if (primary && isOuterEdge(edge, primary, areas)) return primary;
  return areas.find((a) => isOuterEdge(edge, a, areas)) ?? primary;
}

/**
 * Dónde debe quedar la pill cuando el viewport CSS acaba de agrandarse.
 *
 * Al boot WebView2 pinta un recuadro chico: los imanes se calculan contra
 * ese recuadro y la isla queda en una esquina. Recalcular contra las áreas
 * reales: acoplada, el centro de ESE canto; si no, el hogar de arriba.
 */
export function geometryReseat(
  state: {
    docked: DockEdge | null;
    size: { w: number; h: number };
    current: Rect;
  },
  areas: readonly Area[],
): { at: { x: number; y: number }; edge: DockEdge | null } | null {
  if (state.docked) {
    const current = {
      x: state.current.x,
      y: state.current.y,
      w: state.size.w,
      h: state.size.h,
    };
    const area = areaWithOuterEdge(state.docked, current, areas);
    if (!area) return null;
    if (!isOuterEdge(state.docked, area, areas)) {
      return defaultPillHome(state.size, areas);
    }
    return {
      at: edgeCenterPoint(state.docked, state.size, workAreaOf(area)),
      edge: state.docked,
    };
  }
  return defaultPillHome(state.size, areas);
}

export type MagnetHit = {
  at: { x: number; y: number };
  /** `null` = centro de la pantalla, flotante. */
  edge: DockEdge | null;
};

/**
 * Imanes de reposo: centro de la pantalla y centro de cada canto exterior.
 *
 * Si está cerca de un canto pero no de su centro, igual se va al centro de
 * ese canto (no se queda a una altura arbitraria).
 */
export function snapMagnet(
  rect: Rect,
  areas: readonly Area[],
  snapPx = MAGNET_SNAP_PX,
): MagnetHit | null {
  const area = areaFor(rect, areas);
  if (!area) return null;
  const work = workAreaOf(area);
  const size = { w: rect.w, h: rect.h };
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const magnets: MagnetHit[] = [{ at: screenCenterPoint(size, work), edge: null }];
  for (const edge of DOCK_EDGES) {
    if (!isOuterEdge(edge, area, areas)) continue;
    magnets.push({
      at: edgeCenterPoint(edge, size, work),
      edge,
    });
  }

  let best: MagnetHit | null = null;
  let bestD = Infinity;
  for (const m of magnets) {
    const mx = m.at.x + size.w / 2;
    const my = m.at.y + size.h / 2;
    const d = Math.hypot(cx - mx, cy - my);
    if (d < bestD) {
      bestD = d;
      best = m;
    }
  }
  if (best && bestD <= snapPx) return best;

  const dock = dockCandidate(rect, areas);
  if (!dock) return null;
  return {
    at: edgeCenterPoint(dock.edge, size, workAreaOf(dock.area)),
    edge: dock.edge,
  };
}

/**
 * Al soltar un arrastre: engancha un canto si quedó contra él.
 *
 * El techo es el notch: siempre al centro de ese canto, como el recorte de
 * Apple. En laterales y abajo el eje libre se conserva —ahí sí es un
 * parking, no una isla de hardware.
 */
export function snapDrop(rect: Rect, areas: readonly Area[]): MagnetHit | null {
  const dock = dockCandidate(rect, areas);
  if (!dock) return null;
  if (dock.edge === "top") {
    return {
      at: edgeCenterPoint("top", { w: rect.w, h: rect.h }, workAreaOf(dock.area)),
      edge: "top",
    };
  }
  return { at: dock.at, edge: dock.edge };
}

/**
 * Pared SDF local en un canto, para que la pill se funda con el borde.
 *
 * Vive casi toda fuera del viewport: el clip recorta y se lee como gota
 * pegada. No es una tira a lo ancho de la pantalla — solo un tramo cerca
 * de la pill. Acoplada, la isla ya no usa esta pared: su silueta es
 * `notchShape` (lados verticales al bisel, sin menisco).
 *
 * `DEPTH` es hacia afuera. `OVERLAP` es cuánto asoma al viewport: un poco
 * adentro para que el `smin` fusione dintel y pestaña en un solo cuerpo
 * (0 dejaba un cuello y se leía bola pegada). `FLARE` es el filete a cada
 * lado: 0 para que no nazcan alas. Un filete ancho + `smin` era lo que
 * abría los costados hacia afuera en vez de seguir derecho al techo.
 */
export const EDGE_WALL_DEPTH = 40;
export const EDGE_WALL_OVERLAP = 8;
/**
 * Filete a cada lado de la pill, a lo largo del canto.
 *
 * 0 (default): la pared mide lo mismo que la pill y el canto queda derecho,
 * que es lo que la fusión suelta busca. Un filete chico se lee como menisco;
 * uno ancho abre los costados y se lee como alas, no como notch — por eso el
 * acople usa un valor medido (`MENISCUS_FLARE`).
 */
export const EDGE_WALL_FLARE = 0;

/**
 * Filete del menisco acoplado: el canto que toca la pantalla se lee líquido.
 *
 * Chico a propósito: con `BLEND` el `smin` cierra la junta solo. Más ancho
 * que esto eran las alas que el notch evitaba.
 */
export const MENISCUS_FLARE = 12;
/**
 * Si el otro canto está a menos de esto, es una esquina: se emite también
 * esa pared y el dintel llega hasta el vértice.
 */
export const EDGE_CORNER_PX = 48;

/** Empate de distancias: techo/suelo antes que un costado. */
function preferTiedHit<T extends { edge: DockEdge; gap: number }>(a: T, b: T): T {
  if (a.gap < b.gap) return a;
  if (b.gap < a.gap) return b;
  const horiz = (e: DockEdge) => e === "top" || e === "bottom";
  if (horiz(a.edge) !== horiz(b.edge)) return horiz(a.edge) ? a : b;
  return a;
}

/** El borde exterior más cercano, si está a menos de `maxGap`. */
export function nearestOuterWorkEdge(
  rect: Rect,
  areas: readonly Area[],
  maxGap: number,
): { edge: DockEdge; area: Area; work: Rect; gap: number } | null {
  const nearby = nearbyOuterWorkEdges(rect, areas, maxGap);
  if (nearby.length === 0) return null;
  return nearby.reduce(preferTiedHit);
}

/** Todos los cantos exteriores a menos de `maxGap` (esquinas: dos). */
export function nearbyOuterWorkEdges(
  rect: Rect,
  areas: readonly Area[],
  maxGap: number,
): { edge: DockEdge; area: Area; work: Rect; gap: number }[] {
  const area = areaFor(rect, areas);
  if (!area) return [];
  const work = workAreaOf(area);
  const gaps = edgeGaps(rect, work);
  const out: { edge: DockEdge; area: Area; work: Rect; gap: number }[] = [];
  for (const edge of DOCK_EDGES) {
    if (!isOuterEdge(edge, area, areas)) continue;
    if (edgeHasReservedInset(edge, area)) continue;
    const dist = Math.max(gaps[edge], 0);
    if (dist <= maxGap) out.push({ edge, area, work, gap: dist });
  }
  return out;
}

/** Rectángulo de la pared, en las mismas coordenadas que `pill` y `work`. */
export function edgeWallRect(
  edge: DockEdge,
  pill: Rect,
  work: Rect,
  opts?: {
    depth?: number;
    overlap?: number;
    flare?: number;
    /** Filete “antes” (izquierda en top/bottom, arriba en left/right). */
    flareBefore?: number;
    /** Filete “después” (derecha / abajo). */
    flareAfter?: number;
  },
): Rect {
  const depth = opts?.depth ?? EDGE_WALL_DEPTH;
  const overlap = opts?.overlap ?? EDGE_WALL_OVERLAP;
  const flare = opts?.flare ?? EDGE_WALL_FLARE;
  const before = opts?.flareBefore ?? flare;
  const after = opts?.flareAfter ?? flare;
  switch (edge) {
    case "top":
      return {
        x: pill.x - before,
        y: work.y - depth + overlap,
        w: pill.w + before + after,
        h: depth,
      };
    case "bottom":
      return {
        x: pill.x - before,
        y: work.y + work.h - overlap,
        w: pill.w + before + after,
        h: depth,
      };
    case "left":
      return {
        x: work.x - depth + overlap,
        y: pill.y - before,
        w: depth,
        h: pill.h + before + after,
      };
    case "right":
      return {
        x: work.x + work.w - overlap,
        y: pill.y - before,
        w: depth,
        h: pill.h + before + after,
      };
  }
}

function roomAlong(
  edge: DockEdge,
  pill: Rect,
  work: Rect,
): { before: number; after: number } {
  if (edge === "top" || edge === "bottom") {
    return {
      before: pill.x - work.x,
      after: work.x + work.w - (pill.x + pill.w),
    };
  }
  return {
    before: pill.y - work.y,
    after: work.y + work.h - (pill.y + pill.h),
  };
}

function flaresAlong(
  edge: DockEdge,
  pill: Rect,
  work: Rect,
  nearby: ReadonlySet<DockEdge>,
  flare: number,
): { flareBefore: number; flareAfter: number } {
  const room = roomAlong(edge, pill, work);
  const f = flare;
  const cornerBefore =
    edge === "top" || edge === "bottom"
      ? nearby.has("left") || room.before <= EDGE_CORNER_PX
      : nearby.has("top") || room.before <= EDGE_CORNER_PX;
  const cornerAfter =
    edge === "top" || edge === "bottom"
      ? nearby.has("right") || room.after <= EDGE_CORNER_PX
      : nearby.has("bottom") || room.after <= EDGE_CORNER_PX;
  if (!cornerBefore && !cornerAfter) {
    const eq = Math.max(0, Math.min(f, room.before, room.after));
    return { flareBefore: eq, flareAfter: eq };
  }
  return {
    flareBefore: Math.max(0, cornerBefore ? room.before : Math.min(f, room.before)),
    flareAfter: Math.max(0, cornerAfter ? room.after : Math.min(f, room.after)),
  };
}

/**
 * Paredes SDF para fundirse con el canto (y con la esquina, si aplica).
 *
 * A lo largo de un solo borde el dintel es simétrico: el mismo filete a
 * ambos lados, recortado al área útil para no dejar masa fuera que tire
 * del blob. En una esquina se emiten las dos paredes y el dintel llega
 * al vértice.
 */
export function edgeWallsFor(
  pill: Rect,
  areas: readonly Area[],
  opts?: {
    maxGap?: number;
    prefer?: DockEdge | null;
    cornerPx?: number;
    /** Filete a cada lado del dintel (menisco). Default `EDGE_WALL_FLARE`. */
    flare?: number;
  },
): Rect[] {
  const area = areaFor(pill, areas);
  if (!area) return [];
  const work = workAreaOf(area);
  const maxGap = opts?.maxGap ?? 24;
  const cornerPx = opts?.cornerPx ?? EDGE_CORNER_PX;
  const flare = opts?.flare ?? EDGE_WALL_FLARE;
  const nearby = nearbyOuterWorkEdges(pill, areas, Math.max(maxGap, cornerPx));
  const edges: DockEdge[] = [];
  if (
    opts?.prefer &&
    isOuterEdge(opts.prefer, area, areas) &&
    !edgeHasReservedInset(opts.prefer, area)
  ) {
    edges.push(opts.prefer);
  } else {
    const close = nearby.filter((h) => h.gap <= maxGap);
    if (close.length > 0) {
      edges.push(close.reduce(preferTiedHit).edge);
    }
  }
  for (const hit of nearby) {
    if (hit.gap <= cornerPx && !edges.includes(hit.edge)) edges.push(hit.edge);
  }
  if (edges.length === 0) return [];
  const set = new Set(edges);
  return edges.map((edge) =>
    edgeWallRect(edge, pill, work, flaresAlong(edge, pill, work, set, flare)),
  );
}
