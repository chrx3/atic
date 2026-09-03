/**
 * De rectángulos a formas del campo.
 *
 * Las superficies miden rectángulos —`getBoundingClientRect`, la geometría de
 * la pill, lo que manda Rust— y el campo pide centro y semiejes. Traducir eso a
 * mano en cada sitio es cómo se cuela un `/2` de menos.
 */

import type { Shape } from "./sdf";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Caja redondeada a partir de su esquina superior izquierda. */
export function boxShape(rect: Rect, radius: number): Shape {
  return {
    kind: "box",
    cx: rect.x + rect.w / 2,
    cy: rect.y + rect.h / 2,
    hw: rect.w / 2,
    hh: rect.h / 2,
    r: radius,
  };
}

/** Una pastilla: la caja con el radio al máximo que admite. */
export function pillShape(rect: Rect): Shape {
  return boxShape(rect, Math.min(rect.w, rect.h) / 2);
}

/**
 * Separación entre dos rectángulos por el eje que de verdad los separa.
 *
 * Negativa si se solapan. Sirve para decidir si dos superficies todavía se
 * leen como una —comparándola contra `REACH`— sin tener que saber de qué lado
 * está una de la otra.
 */
/**
 * El lomo que cose varias gotas en un solo cuerpo.
 *
 * Una fila de círculos unida solo por el `smin` se lee como una oruga: cada
 * gota aporta su arco entero y el contorno sube y baja tanto como el radio.
 * Metiendo una cápsula más fina que las gotas por el eje que las alinea, el
 * cuerpo pasa a ser una tira con ondulaciones suaves —el bulto de cada gota
 * asomando sobre el lomo— en vez de una cadena de bolas.
 *
 * El radio se pasa medido, no fijo: cuando las gotas encogen, el lomo tiene que
 * encoger con ellas o al cerrarse quedaría más gordo que lo que une.
 */
export function capsuleShape(
  from: { x: number; y: number },
  to: { x: number; y: number },
  radius: number,
): Shape {
  return { kind: "capsule", ax: from.x, ay: from.y, bx: to.x, by: to.y, r: radius };
}

/**
 * Hilo entre un ancla (la silueta de la pill) y el panel.
 *
 * `side` es el lado del panel que mira al ancla —el mismo `data-side` del
 * morph. La cápsula entra un poco en cada cuerpo para que el `smin` filetee
 * la junta en vez de dejar un tope plano. Si el hueco ya es más corto que el
 * radio, no hay hilo que dibujar: las dos formas ya se tocan.
 */
/**
 * Dónde nace el hilo sobre el eje paralelo al borde.
 *
 * El centro del ancla, pero solo mientras caiga sobre la otra silueta. Colgado
 * de una esquina —el panel de cupos, que nace del canto de la isla— ese centro
 * queda al costado del panel y el hilo terminaba en el aire, a su lado. `null`
 * si no se solapan lo bastante como para que quepa.
 */
function stemAlong(
  fromStart: number,
  fromSize: number,
  toStart: number,
  toSize: number,
  radius: number,
): number | null {
  const lo = Math.max(fromStart, toStart) + radius;
  const hi = Math.min(fromStart + fromSize, toStart + toSize) - radius;
  if (hi < lo) return null;
  return Math.min(Math.max(fromStart + fromSize / 2, lo), hi);
}

/**
 * El cuerpo del que cuelga el hilo tiene que ser más gordo que el cuello.
 *
 * Una pestaña de 10 px en el borde de arriba no filetea con un panel de 200:
 * el `smin` deja un pezón en el techo del panel, no una fusión. En el canto
 * la isla abierta mide ~34 px y el cuello sí se lee.
 */
export function stemBodyFits(
  body: Rect,
  side: "top" | "bottom" | "left" | "right",
  radius: number,
): boolean {
  if (radius <= 0) return false;
  const thickness = side === "top" || side === "bottom" ? body.h : body.w;
  return thickness >= radius * 3;
}

/**
 * El cuerpo más cercano al panel que todavía filetea.
 *
 * La piel viva suele ganar, pero a veces es un disco fantasma —con la rueda
 * abierta, `pill-skin` sigue midiendo el stack arriba-izquierda del root— y
 * un hilo a ese disco deja el palo vertical que se ve al lado de la flor.
 * `maxGap` corta esos hilos: un cuello de 200 px ya no es cuello.
 */
export function nearestStemBody(
  candidates: readonly (Rect | null | undefined)[],
  panel: Rect,
  side: "top" | "bottom" | "left" | "right",
  radius: number,
  maxGap: number,
): Rect | null {
  let best: Rect | null = null;
  let bestGap = Infinity;
  for (const body of candidates) {
    if (!body || !stemBodyFits(body, side, radius)) continue;
    if (stemBetween(body, panel, side, radius) == null) continue;
    const g = gapBetween(body, panel);
    if (g > maxGap) continue;
    if (g < bestGap) {
      bestGap = g;
      best = body;
    }
  }
  return best;
}

export function stemBetween(
  from: Rect,
  to: Rect,
  side: "top" | "bottom" | "left" | "right",
  radius: number,
): Shape | null {
  if (radius <= 0) return null;
  const overlap = Math.min(radius * 1.5, 6);
  let ax: number;
  let ay: number;
  let bx: number;
  let by: number;
  if (side === "top" || side === "bottom") {
    const along = stemAlong(from.x, from.w, to.x, to.w, radius);
    if (along === null) return null;
    ax = bx = along;
    if (side === "top") {
      ay = from.y + from.h - overlap;
      by = to.y + overlap;
      if (by - ay < radius) return null;
    } else {
      ay = from.y + overlap;
      by = to.y + to.h - overlap;
      if (ay - by < radius) return null;
    }
  } else {
    const along = stemAlong(from.y, from.h, to.y, to.h, radius);
    if (along === null) return null;
    ay = by = along;
    if (side === "left") {
      ax = from.x + from.w - overlap;
      bx = to.x + overlap;
      if (bx - ax < radius) return null;
    } else {
      ax = from.x + overlap;
      bx = to.x + to.w - overlap;
      if (ax - bx < radius) return null;
    }
  }
  return capsuleShape({ x: ax, y: ay }, { x: bx, y: by }, radius);
}

export function gapBetween(a: Rect, b: Rect): number {
  const gapX = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const gapY = Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h));
  return Math.max(gapX, gapY);
}
