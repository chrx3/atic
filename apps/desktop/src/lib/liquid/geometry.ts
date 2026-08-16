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

export function gapBetween(a: Rect, b: Rect): number {
  const gapX = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const gapY = Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h));
  return Math.max(gapX, gapY);
}
