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
export function gapBetween(a: Rect, b: Rect): number {
  const gapX = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const gapY = Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h));
  return Math.max(gapX, gapY);
}
