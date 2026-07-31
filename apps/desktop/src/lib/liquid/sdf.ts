/**
 * El campo de distancia con signo que funde las formas.
 *
 * Es la otra forma de resolver la fusión, y la alternativa al filtro SVG de
 * `GooFilter.svelte`. En vez de difuminar el alfa y volver a endurecerlo, cada
 * forma se describe como una función que devuelve la distancia a su borde
 * —negativa adentro, 0 en la superficie, positiva afuera— y el grupo entero es
 * la UNIÓN SUAVE de esas funciones.
 *
 * La unión suave es todo el truco. Un `Math.min` da la unión dura: dos formas se
 * tocan con una costura afilada. El `smin` mezcla los dos campos cerca de la
 * costura, y esa mezcla ES el filete cóncavo. Subiendo `k` el filete crece hasta
 * volverse un cuello gomoso, así que una sola perilla va de "junta nítida" a
 * "metaball derretida".
 *
 * Frente al filtro tiene tres ventajas que importan acá:
 *
 *   - **No depende del motor.** Es aritmética en JS: el mismo contorno en
 *     WebView2, en Chrome y en un test sin DOM.
 *   - **No engorda.** El endurecido del filtro infla la silueta 0.28σ por lado y
 *     hay que compensarlo con `preFilter()` en cada forma exacta. Acá el
 *     contorno pasa por donde se pidió.
 *   - **No hay que dibujar el cuello.** `k` controla directamente cuánto se
 *     funde a distancia, así que el cuello emerge del campo en vez de ser una
 *     cápsula a mano con su penetración, su grosor y su piso.
 *
 * `contour.ts` muestrea `Field.eval` sobre una grilla y traza la silueta fundida
 * como un `<path>`.
 */

/** Distancia a una caja redondeada alineada a los ejes. `hw`/`hh` son
 *  semiejes, `r` el radio de esquina. */
export function sdRoundBox(
  px: number,
  py: number,
  cx: number,
  cy: number,
  hw: number,
  hh: number,
  r: number,
): number {
  // El radio no puede pasarse de la caja o la forma se da vuelta.
  const rr = Math.min(r, Math.min(hw, hh));
  const qx = Math.abs(px - cx) - hw + rr;
  const qy = Math.abs(py - cy) - hh + rr;
  const ax = Math.max(qx, 0);
  const ay = Math.max(qy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(qx, qy), 0) - rr;
}

/** Distancia a una cápsula: el segmento (ax,ay)→(bx,by) engordado `r`. */
export function sdCapsule(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
  r: number,
): number {
  const pax = px - ax;
  const pay = py - ay;
  const bax = bx - ax;
  const bay = by - ay;
  const denom = bax * bax + bay * bay || 1e-6;
  // Proyección de P sobre AB, recortada al segmento.
  const h = Math.max(0, Math.min(1, (pax * bax + pay * bay) / denom));
  return Math.hypot(pax - bax * h, pay - bay * h) - r;
}

/**
 * Mínimo suave polinómico: el que fabrica el filete.
 *
 * `k = 0` es el mínimo duro (costura afilada). Cuanto más alto, más ancha la
 * mezcla y más grande el filete. Es la forma con `mix()`, que no deja un quiebre
 * de derivada en la costura.
 */
export function smin(a: number, b: number, k: number): number {
  if (k <= 0) return Math.min(a, b);
  const h = Math.max(0, Math.min(1, 0.5 + (0.5 * (b - a)) / k));
  return b * (1 - h) + a * h - k * h * (1 - h);
}

/**
 * Cuánto se corre la superficie hacia afuera por culpa del `smin`.
 *
 * El campo baja como máximo `k/4` —con `a = b = d` el `smin` da `d − k/4`— y
 * como el gradiente es ~1, la superficie se mueve esa misma distancia. Es el
 * único margen que la grilla de muestreo necesita alrededor de las formas.
 */
export function sminBulge(k: number): number {
  return k / 4;
}

/**
 * El hueco más grande que el cuello todavía cruza: **k/2**.
 *
 * Es el equivalente del `1.72·σ` del filtro, y el número que decide si hace
 * falta dibujar un cuello o no. Sale de mirar el punto medio del hueco: ahí la
 * distancia a cada forma es `g/2`, así que el campo vale `g/2 − k/4`, y deja de
 * ser positivo —o sea, se llena— cuando `g ≤ k/2`.
 *
 * No confundir con `sminBulge`. Los dos salen de la misma cuenta pero miden
 * cosas distintas: `k/4` es cuánto engorda la silueta cerca de una junta, `k/2`
 * es hasta dónde alcanza a unir.
 */
export function sminReach(k: number): number {
  return k / 2;
}

export interface RoundBox {
  kind: "box";
  cx: number;
  cy: number;
  hw: number;
  hh: number;
  r: number;
}

export interface Capsule {
  kind: "capsule";
  ax: number;
  ay: number;
  bx: number;
  by: number;
  r: number;
}

export type Shape = RoundBox | Capsule;

export function shapeSD(s: Shape, px: number, py: number): number {
  return s.kind === "box"
    ? sdRoundBox(px, py, s.cx, s.cy, s.hw, s.hh, s.r)
    : sdCapsule(px, py, s.ax, s.ay, s.bx, s.by, s.r);
}

export type Bounds = { minX: number; minY: number; maxX: number; maxY: number };

/** El grupo entero como un solo campo: la unión suave de todas las formas. */
export class Field {
  constructor(
    public shapes: Shape[] = [],
    public k = 12,
  ) {}

  eval(x: number, y: number): number {
    const { shapes, k } = this;
    if (shapes.length === 0) return Infinity;
    let d = shapeSD(shapes[0], x, y);
    for (let i = 1; i < shapes.length; i++) {
      d = smin(d, shapeSD(shapes[i], x, y), k);
    }
    return d;
  }

  /**
   * La caja del grupo, con el margen justo para que la grilla no recorte el
   * bulto del `smin`.
   *
   * El margen es `k/4`, no `k`: ver `sminBulge`. La diferencia no es cosmética
   * —el costo del muestreo va con el área, así que pasarse de margen multiplica
   * el trabajo por cuadro sin cambiar el dibujo.
   */
  bounds(pad = 0): Bounds {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    const grow = pad + sminBulge(this.k);
    for (const s of this.shapes) {
      const [x0, y0, x1, y1] =
        s.kind === "box"
          ? [s.cx - s.hw, s.cy - s.hh, s.cx + s.hw, s.cy + s.hh]
          : [
              Math.min(s.ax, s.bx) - s.r,
              Math.min(s.ay, s.by) - s.r,
              Math.max(s.ax, s.bx) + s.r,
              Math.max(s.ay, s.by) + s.r,
            ];
      minX = Math.min(minX, x0 - grow);
      minY = Math.min(minY, y0 - grow);
      maxX = Math.max(maxX, x1 + grow);
      maxY = Math.max(maxY, y1 + grow);
    }
    if (!isFinite(minX)) return { minX: 0, minY: 0, maxX: 0, maxY: 0 };
    return { minX, minY, maxX, maxY };
  }
}
