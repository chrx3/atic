/**
 * Coloca un float junto a la pill, unidos por una esquina (cuello líquido).
 *
 * Espejo de `bubble_rect` en `floating.rs`, pero en px CSS del overlay y con
 * la silueta VIVA de la pill — Rust a veces ancla con un `pill_rect` viejo
 * (rueda a medio cerrar, hit-rects en vuelo) y el panel queda lejos o encima.
 *
 * También helpers del acto fused grow → separate (paneles clipboard/snippets):
 * seed **solapado** (gap negativo / SEED_OVERLAP) → crecer con borde clavado →
 * reposo (gap > REACH). Nunca seed con gap≥0: se lee elemento externo.
 */

import type { BubbleOpen } from "$core/types";
import { workAreaOf, type Area } from "$ipc/overlay";
import { BUBBLE_GAP, MARGIN } from "./contract";
import { BOTTOM_SLOT_INSET } from "./toolSlots";

export type PillRect = { x: number; y: number; w: number; h: number };

export type PlaceResult = Pick<
  BubbleOpen,
  "side" | "offset" | "x" | "y" | "w" | "h"
>;

/** Fusionado a la pill al nacer: ≪ REACH → un solo blob. */
export const FUSED_GAP_PX = 2;
/**
 * Cuánto solapa la semilla sobre la pill al nacer / al encoger.
 * Gap positivo al lado se lee como “elemento externo” aunque el goo una;
 * overlap negativo hace que parezca un solo disco que se estira.
 */
export const SEED_OVERLAP_PX = 20;
/**
 * Idle tras separate: 16 px.
 *
 * Por encima de `REACH` (12) → el cuello corta, quedan dos siluetas. Pero por
 * debajo de `INFLUENCE` (24), así que siguen compartiendo campo y se estiran
 * ~0.7 px una hacia la otra: se leen como dos gotas que se saben cerca, no como
 * dos cajas. Acercándolas, ese estiramiento crece hasta 1.5 px y recién ahí
 * nace el cuello, desde ancho cero.
 */
export const PANEL_RESTING_GAP_PX = 16;
/** Semilla = disco ~pill (40px), no stadium truncado. */
export const PANEL_GROW_SEED = 40;

/**
 * Aire del panel contra el área útil. `MARGIN` de la pill es 0 (puede
 * solapar taskbar); el float no: el título se recorta si se pega al canto.
 */
const PANEL_MARGIN = 8;

function viewportBox(): Area | null {
  if (typeof window === "undefined") return null;
  const w = window.innerWidth;
  const h = window.innerHeight;
  if (!(w > 1 && h > 1)) return null;
  return { x: 0, y: 0, w, h };
}

function intersectAreas(a: Area, b: Area): Area | null {
  const x0 = Math.max(a.x, b.x);
  const y0 = Math.max(a.y, b.y);
  const x1 = Math.min(a.x + a.w, b.x + b.w);
  const y1 = Math.min(a.y + a.h, b.y + b.h);
  if (x1 <= x0 || y1 <= y0) return null;
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/**
 * Monitor de la pill: `work` (rcWork) si vino, si no bounds. Recorta contra
 * el viewport CSS — el overlay pinta desde (0,0) y `overflow: hidden` recorta
 * lo que se salga, aunque Win32 diga que hay más pantalla.
 *
 * Si `work` no excluye la taskbar (ausente o igual a bounds), aplica el mismo
 * espíritu que `BOTTOM_SLOT_INSET` y un margen lateral, para no montar el
 * panel sobre la barra ni recortar el título en el canto.
 */
function resolveWork(pill: PillRect, panel: { w: number; h: number }, work?: Area[]): Area {
  const acx = pill.x + pill.w / 2;
  const acy = pill.y + pill.h / 2;
  const area =
    work?.find(
      (a) =>
        acx >= a.x &&
        acx <= a.x + a.w &&
        acy >= a.y &&
        acy <= a.y + a.h,
    ) ??
    work?.[0] ??
    ({
      x: 0,
      y: 0,
      w: typeof window !== "undefined" ? window.innerWidth : panel.w,
      h: typeof window !== "undefined" ? window.innerHeight : panel.h,
    } satisfies Area);

  const usable = workAreaOf(area);
  const bottomEx = Math.max(0, area.y + area.h - (usable.y + usable.h));
  const extraBottom = Math.max(0, BOTTOM_SLOT_INSET - bottomEx);

  let box: Area = {
    x: usable.x + PANEL_MARGIN,
    y: usable.y + PANEL_MARGIN,
    w: Math.max(0, usable.w - PANEL_MARGIN * 2),
    h: Math.max(0, usable.h - PANEL_MARGIN - extraBottom),
  };
  const view = viewportBox();
  if (view) {
    box = intersectAreas(box, view) ?? box;
  }
  return box;
}

function clampToWork(
  x: number,
  y: number,
  bw: number,
  bh: number,
  work: Area,
): { x: number; y: number } {
  const maxX = Math.max(work.x + work.w - bw, work.x);
  const maxY = Math.max(work.y + work.h - bh, work.y);
  return {
    x: Math.min(Math.max(x, work.x), maxX),
    y: Math.min(Math.max(y, work.y), maxY),
  };
}

function alongAxis(
  near: number,
  far: number,
  size: number,
  lo: number,
  hi: number,
): number {
  if (near >= lo + MARGIN && near + size + MARGIN <= hi) return near;
  if (far >= lo + MARGIN && far + size + MARGIN <= hi) return far;
  return Math.min(Math.max(near, lo + MARGIN), Math.max(hi - size - MARGIN, lo + MARGIN));
}

/**
 * Coloca `panel` en un lado fijo de la pill (cuello por esquina).
 */
export function placeOnSide(
  pill: PillRect,
  side: BubbleOpen["side"],
  panel: { w: number; h: number },
  opts: { gap?: number; corner?: number; work?: Area[] } = {},
): PlaceResult {
  const gap = opts.gap ?? BUBBLE_GAP;
  const corner = opts.corner ?? 18;
  const bw = panel.w;
  const bh = panel.h;
  const { x: ax, y: ay, w: aw, h: ah } = pill;
  const acx = ax + aw / 2;
  const acy = ay + ah / 2;
  const work = resolveWork(pill, panel, opts.work);
  const workRight = work.x + work.w;
  const workBottom = work.y + work.h;

  let x: number;
  let y: number;

  if (side === "top") {
    x = alongAxis(ax + aw - corner, ax - bw + corner, bw, work.x, workRight);
    y = ay + ah + gap;
  } else if (side === "bottom") {
    x = alongAxis(ax + aw - corner, ax - bw + corner, bw, work.x, workRight);
    y = ay - gap - bh;
  } else if (side === "left") {
    y = alongAxis(ay + ah - corner, ay - bh + corner, bh, work.y, workBottom);
    x = ax + aw + gap;
  } else {
    y = alongAxis(ay + ah - corner, ay - bh + corner, bh, work.y, workBottom);
    x = ax - gap - bw;
  }

  // Los dos ejes: el paralelo ya lo intenta `alongAxis`, pero el “hacia
  // afuera” (y si top/bottom, x si left/right) se iba del monitor — recorte
  // contra la taskbar o el canto. Flip en `placeBesidePill`; esto es la red.
  const clamped = clampToWork(x, y, bw, bh, work);
  x = clamped.x;
  y = clamped.y;

  const along =
    side === "top" || side === "bottom"
      ? Math.min(Math.max(acx - x, corner), Math.max(bw - corner, corner))
      : Math.min(Math.max(acy - y, corner), Math.max(bh - corner, corner));

  return { side, offset: along, x, y, w: bw, h: bh };
}

/**
 * Ancla `panel` a una esquina de `pill`.
 *
 * Preferencia: abajo → arriba → derecha → izquierda. En el lado elegido el
 * panel crece “hacia afuera” desde el borde de la pill (no desde su centro).
 */
export function placeBesidePill(
  pill: PillRect,
  panel: { w: number; h: number },
  opts: { gap?: number; corner?: number; work?: Area[] } = {},
): PlaceResult {
  const gap = opts.gap ?? BUBBLE_GAP;
  const { x: ax, y: ay, w: aw, h: ah } = pill;
  const bw = panel.w;
  const bh = panel.h;
  const work = resolveWork(pill, panel, opts.work);
  const workRight = work.x + work.w;
  const workBottom = work.y + work.h;

  const fitsBelow = ay + ah + gap + bh + MARGIN <= workBottom;
  const fitsAbove = ay - gap - bh - MARGIN >= work.y;
  const fitsRight = ax + aw + gap + bw + MARGIN <= workRight;
  const fitsLeft = ax - gap - bw - MARGIN >= work.x;

  let side: BubbleOpen["side"];
  if (fitsBelow) side = "top";
  else if (fitsAbove) side = "bottom";
  else if (fitsRight) side = "left";
  else if (fitsLeft) side = "right";
  else {
    // Ningún lado entra entero: el de más hueco, y `placeOnSide` clampea.
    const spaceBelow = workBottom - (ay + ah + gap);
    const spaceAbove = ay - gap - work.y;
    const spaceRight = workRight - (ax + aw + gap);
    const spaceLeft = ax - gap - work.x;
    const best = Math.max(spaceBelow, spaceAbove, spaceRight, spaceLeft);
    side =
      best === spaceBelow
        ? "top"
        : best === spaceAbove
          ? "bottom"
          : best === spaceRight
            ? "left"
            : "right";
  }

  return placeOnSide(pill, side, panel, { ...opts, gap });
}

/** Reposo idle: gap > REACH para cortar el cuello. */
export function placePanelResting(
  pill: PillRect,
  panel: { w: number; h: number },
  opts: { corner?: number; work?: Area[]; gap?: number } = {},
): PlaceResult {
  return placeBesidePill(pill, panel, {
    ...opts,
    gap: opts.gap ?? PANEL_RESTING_GAP_PX,
  });
}

/**
 * Semilla disco **solapada** en el mismo lado que el reposo final.
 * Borde hacia la pill clavado → el grow estira hacia afuera.
 * Usa `SEED_OVERLAP_PX` (gap negativo), no `FUSED_GAP_PX`.
 */
export function placePanelFusedSeed(
  pill: PillRect,
  full: { w: number; h: number },
  opts: {
    corner?: number;
    work?: Area[];
    seed?: number;
    /** @deprecated usa `overlap`; gap positivo nace “al lado”. */
    fusedGap?: number;
    overlap?: number;
    restingGap?: number;
  } = {},
): PlaceResult {
  const seed = opts.seed ?? PANEL_GROW_SEED;
  const overlap = opts.overlap ?? SEED_OVERLAP_PX;
  // Gap negativo = solapa la pill. fusedGap legacy solo si overlap explícito 0.
  const gap =
    opts.overlap === 0 && opts.fusedGap != null
      ? opts.fusedGap
      : -overlap;
  const resting = placePanelResting(pill, full, {
    corner: opts.corner,
    work: opts.work,
    gap: opts.restingGap,
  });
  return placeOnSide(pill, resting.side, { w: seed, h: seed }, {
    gap,
    corner: Math.min(opts.corner ?? 20, seed / 2),
    work: opts.work,
  });
}

/**
 * Tamaño completo aún fused (tras grow, antes de separate).
 * Conserva el lado; reclava el borde hacia la pill.
 */
export function placePanelFusedFull(
  pill: PillRect,
  full: { w: number; h: number },
  side: BubbleOpen["side"],
  opts: { corner?: number; work?: Area[]; fusedGap?: number } = {},
): PlaceResult {
  return placeOnSide(pill, side, full, {
    gap: opts.fusedGap ?? FUSED_GAP_PX,
    corner: opts.corner ?? 18,
    work: opts.work,
  });
}

/**
 * Crece la semilla al tamaño final bloqueando el borde que mira a la pill.
 * (Misma idea que `Bubble.resize`: el cuello no se mueve.)
 */
export function expandPanelFromSeed(
  current: PlaceResult,
  full: { w: number; h: number },
): PlaceResult {
  const { side, offset, x, y, w, h } = current;
  if (side === "bottom") {
    return {
      side,
      offset,
      x,
      y: y + h - full.h,
      w: full.w,
      h: full.h,
    };
  }
  if (side === "right") {
    return {
      side,
      offset,
      x: x + w - full.w,
      y,
      w: full.w,
      h: full.h,
    };
  }
  return { side, offset, x, y, w: full.w, h: full.h };
}
