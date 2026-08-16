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
import type { Area } from "$ipc/overlay";
import { BUBBLE_GAP, MARGIN } from "./contract";

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

function resolveWork(pill: PillRect, panel: { w: number; h: number }, work?: Area[]): Area {
  const acx = pill.x + pill.w / 2;
  const acy = pill.y + pill.h / 2;
  return (
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
    } satisfies Area)
  );
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
  return near;
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

  const maxX = Math.max(workRight - bw - MARGIN, work.x + MARGIN);
  const maxY = Math.max(workBottom - bh - MARGIN, work.y + MARGIN);
  if (side === "top" || side === "bottom") {
    x = Math.min(Math.max(x, work.x + MARGIN), maxX);
  } else {
    y = Math.min(Math.max(y, work.y + MARGIN), maxY);
  }

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

  const side: BubbleOpen["side"] = fitsBelow
    ? "top"
    : fitsAbove
      ? "bottom"
      : fitsRight
        ? "left"
        : "right";

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
    /** @deprecated usá `overlap`; gap positivo nace “al lado”. */
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
