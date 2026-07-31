/**
 * Traza la silueta fundida de un `Field` como un `<path>` de SVG.
 *
 * Se muestrea la distancia con signo sobre una grilla y se corre marching
 * squares en la isolínea 0, que es la superficie. Cada celda mira el signo de
 * sus 4 esquinas (16 casos) y emite 0, 1 o 2 tramos; cada extremo se coloca por
 * INTERPOLACIÓN LINEAL sobre el lado de la celda, donde el campo cruza el 0 de
 * verdad, y no en el punto medio — eso es lo que convierte una escalera en una
 * curva. Después los tramos sueltos se cosen en lazos cerrados y se suavizan con
 * Chaikin, para que la silueta se lea fluida y no tiemble entre cuadros al
 * deformarse.
 *
 * El límite que manda sobre todo lo demás es `cell`: marching squares NO PUEDE
 * ver nada más chico que su celda. Si el cuello más fino de la escena mide 6 px,
 * una celda de 6 lo hace aparecer y desaparecer entre cuadros. Y el costo va con
 * el cuadrado de la resolución, así que `cell` es la perilla que decide a la vez
 * la fidelidad y el precio.
 */

import { Field } from "./sdf";

const fmt = (v: number) => v.toFixed(2);

/**
 * Techo de muestras por cuadro.
 *
 * Pasado ese punto se agranda la celda en vez de devolver nada: una silueta
 * menos fina se nota, una silueta que desaparece rompe la app. El overlay cubre
 * el escritorio entero, así que el caso de la escena enorme no es hipotético.
 */
const MAX_SAMPLES = 400_000;

export interface ContourOptions {
  /** Lado de la celda, en las unidades del campo (px). */
  cell?: number;
  /** Pasadas de Chaikin. 0 deja la poligonal cruda. */
  smooth?: number;
  /** Descartar en bloque lo que está lejos del contorno. Ver `BLOCK`. */
  narrowBand?: boolean;
}

/**
 * Celdas finas por lado de bloque, para el descarte en banda estrecha.
 *
 * El contorno es una curva: ocupa una franja delgada de la caja, y en la escena
 * de la pill el interior del globo son más de 300.000 px² de "esto está
 * clarísimamente adentro" que no aportan un solo tramo. Muestrear el campo una
 * vez por bloque y descartar el bloque entero baja el trabajo casi un orden de
 * magnitud.
 *
 * Es exacto, no una aproximación. Como el gradiente del campo nunca supera 1
 * —el `smin` solo lo achata—, la distancia real a la superficie es al menos
 * `|d|`. Si en el centro del bloque `|d| > lado · 1.71`, entonces en TODO el
 * bloque `|d| > lado`, así que no puede haber cruce ahí ni en las celdas que lo
 * tocan: sobre un paso de celda el campo cambia como mucho un paso de celda, y
 * el bloque tiene cuatro.
 */
const BLOCK = 4;
/** Media diagonal del bloque (0.7071) más un bloque de margen. */
const BLOCK_REACH = 1.71;

export interface LiquidPath {
  d: string;
  minX: number;
  minY: number;
  width: number;
  height: number;
  /** Celda realmente usada: puede ser mayor que la pedida si se degradó. */
  cell: number;
  samples: number;
  evals: number;
  points: number;
}

type Pt = { x: number; y: number };

/**
 * Qué lados cruza el contorno en cada uno de los 16 casos.
 *
 * Lados: 0=arriba, 1=derecha, 2=abajo, 3=izquierda. Bits de esquina:
 * 8=arriba-izq, 4=arriba-der, 2=abajo-der, 1=abajo-izq; el bit está prendido
 * cuando esa esquina está ADENTRO (campo < 0). Los casos 5 y 10 son sillas de
 * montar y se resuelven aparte, mirando el centro de la celda.
 */
const EDGES: number[][][] = [
  [],
  [[3, 2]],
  [[2, 1]],
  [[3, 1]],
  [[0, 1]],
  [], // silla
  [[0, 2]],
  [[3, 0]],
  [[3, 0]],
  [[0, 2]],
  [], // silla
  [[0, 1]],
  [[3, 1]],
  [[2, 1]],
  [[3, 2]],
  [],
];

/** Dónde cruza el 0 entre dos muestras vecinas. */
function lerpEdge(
  x0: number,
  y0: number,
  v0: number,
  x1: number,
  y1: number,
  v1: number,
): Pt {
  const denom = v0 - v1;
  const t = Math.abs(denom) < 1e-6 ? 0.5 : v0 / denom;
  const tc = Math.max(0, Math.min(1, t));
  return { x: x0 + (x1 - x0) * tc, y: y0 + (y1 - y0) * tc };
}

export interface ContourResult {
  loops: Pt[][];
  cell: number;
  /** Vértices de la grilla. */
  samples: number;
  /** Evaluaciones del campo que de verdad se hicieron. */
  evals: number;
}

export function contour(field: Field, opts: ContourOptions = {}): ContourResult {
  const b = field.bounds();
  const w = b.maxX - b.minX;
  const h = b.maxY - b.minY;
  if (w <= 0 || h <= 0)
    return { loops: [], cell: opts.cell ?? 6, samples: 0, evals: 0 };

  // Degradar antes que desaparecer: se agranda la celda hasta entrar en el techo.
  let cell = Math.max(1, opts.cell ?? 6);
  let cols = Math.ceil(w / cell) + 1;
  let rows = Math.ceil(h / cell) + 1;
  while (cols * rows > MAX_SAMPLES) {
    cell *= 1.5;
    cols = Math.ceil(w / cell) + 1;
    rows = Math.ceil(h / cell) + 1;
  }

  const val = new Float32Array(cols * rows);
  let evals = 0;

  if (opts.narrowBand === false) {
    for (let j = 0; j < rows; j++) {
      const y = b.minY + j * cell;
      for (let i = 0; i < cols; i++) {
        val[j * cols + i] = field.eval(b.minX + i * cell, y);
      }
    }
    evals = cols * rows;
  } else {
    // Pasada gruesa: un muestreo por bloque decide si vale la pena mirarlo.
    const side = cell * BLOCK;
    const bCols = Math.ceil(cols / BLOCK);
    const bRows = Math.ceil(rows / BLOCK);
    /** 0 = hay que muestrearlo; 1 = todo afuera; -1 = todo adentro. */
    const state = new Int8Array(bCols * bRows);
    for (let bj = 0; bj < bRows; bj++) {
      for (let bi = 0; bi < bCols; bi++) {
        const d = field.eval(
          b.minX + (bi * BLOCK + BLOCK / 2) * cell,
          b.minY + (bj * BLOCK + BLOCK / 2) * cell,
        );
        evals++;
        state[bj * bCols + bi] =
          Math.abs(d) > side * BLOCK_REACH ? (d < 0 ? -1 : 1) : 0;
      }
    }

    // Pasada fina, solo donde el bloque quedó marcado. Los descartados reciben
    // un valor enorme con el signo correcto: nunca entran en una interpolación
    // —no puede haber cruce en una celda que los toque— pero sí definen bien el
    // adentro/afuera de la celda.
    for (let j = 0; j < rows; j++) {
      const y = b.minY + j * cell;
      const bjRow = Math.floor(j / BLOCK) * bCols;
      for (let i = 0; i < cols; i++) {
        const st = state[bjRow + Math.floor(i / BLOCK)];
        if (st === 0) {
          val[j * cols + i] = field.eval(b.minX + i * cell, y);
          evals++;
        } else {
          val[j * cols + i] = st * 1e6;
        }
      }
    }
  }

  const segs: [Pt, Pt][] = [];

  for (let j = 0; j < rows - 1; j++) {
    for (let i = 0; i < cols - 1; i++) {
      const vTL = val[j * cols + i];
      const vTR = val[j * cols + i + 1];
      const vBR = val[(j + 1) * cols + i + 1];
      const vBL = val[(j + 1) * cols + i];

      let mask = 0;
      if (vTL < 0) mask |= 8;
      if (vTR < 0) mask |= 4;
      if (vBR < 0) mask |= 2;
      if (vBL < 0) mask |= 1;
      if (mask === 0 || mask === 15) continue;

      const x0 = b.minX + i * cell;
      const y0 = b.minY + j * cell;
      const x1 = x0 + cell;
      const y1 = y0 + cell;

      const edgePt = (edge: number): Pt => {
        switch (edge) {
          case 0:
            return lerpEdge(x0, y0, vTL, x1, y0, vTR);
          case 1:
            return lerpEdge(x1, y0, vTR, x1, y1, vBR);
          case 2:
            return lerpEdge(x0, y1, vBL, x1, y1, vBR);
          default:
            return lerpEdge(x0, y0, vTL, x0, y1, vBL);
        }
      };

      let cases = EDGES[mask];
      if (mask === 5 || mask === 10) {
        // Silla: las dos esquinas de una diagonal están adentro y las otras dos
        // afuera. El centro decide si el istmo pasa por una diagonal o la otra.
        const center = field.eval((x0 + x1) / 2, (y0 + y1) / 2);
        evals++;
        const unida: number[][] = [
          [3, 0],
          [2, 1],
        ];
        const separada: number[][] = [
          [3, 2],
          [0, 1],
        ];
        if (mask === 5) cases = center < 0 ? unida : separada;
        else cases = center < 0 ? separada : unida;
      }
      for (const [ea, eb] of cases) segs.push([edgePt(ea), edgePt(eb)]);
    }
  }

  return { loops: stitch(segs, cell), cell, samples: cols * rows, evals };
}

/** Cose los tramos sueltos en lazos, pegando extremos casi iguales. */
function stitch(segs: [Pt, Pt][], cell: number): Pt[][] {
  const eps = cell * 0.5;
  const key = (p: Pt) => `${Math.round(p.x / eps)},${Math.round(p.y / eps)}`;
  const map = new Map<string, { seg: number; end: 0 | 1 }[]>();
  segs.forEach((s, idx) => {
    for (const end of [0, 1] as const) {
      const k = key(s[end]);
      const arr = map.get(k);
      if (arr) arr.push({ seg: idx, end });
      else map.set(k, [{ seg: idx, end }]);
    }
  });

  const used: boolean[] = new Array(segs.length).fill(false);
  const loops: Pt[][] = [];

  for (let start = 0; start < segs.length; start++) {
    if (used[start]) continue;
    const loop: Pt[] = [];
    let cur = start;
    let end: 0 | 1 = 0;
    let guard = 0;
    while (!used[cur] && guard++ < segs.length + 2) {
      used[cur] = true;
      loop.push(segs[cur][end]);
      // Anotado a mano: `end` se reasigna al final del ciclo desde un valor que
      // sale de este mismo punto, y sin el tipo explícito TypeScript entra en
      // una inferencia circular y lo da por `any`.
      const bPt: Pt = segs[cur][end === 0 ? 1 : 0];
      let next = -1;
      let nextEnd: 0 | 1 = 0;
      for (const c of map.get(key(bPt)) ?? []) {
        if (!used[c.seg]) {
          next = c.seg;
          nextEnd = c.end;
          break;
        }
      }
      if (next === -1) break;
      cur = next;
      end = nextEnd;
    }
    if (loop.length >= 3) loops.push(loop);
  }
  return loops;
}

/** Chaikin: cada pasada corta las esquinas a 1/4 y 3/4 de cada lado. */
function chaikin(pts: Pt[], passes: number): Pt[] {
  let out = pts;
  for (let p = 0; p < passes; p++) {
    const next: Pt[] = [];
    const n = out.length;
    for (let i = 0; i < n; i++) {
      const a = out[i];
      const b = out[(i + 1) % n];
      next.push({ x: a.x * 0.75 + b.x * 0.25, y: a.y * 0.75 + b.y * 0.25 });
      next.push({ x: a.x * 0.25 + b.x * 0.75, y: a.y * 0.25 + b.y * 0.75 });
    }
    out = next;
  }
  return out;
}

/**
 * Campo → lazos → suavizado → un solo `d`.
 *
 * Las coordenadas van en el espacio del campo; quien dibuje pone el `viewBox`
 * con los límites que se devuelven. Si el grupo tiene islas sueltas salen como
 * subtrazos, así que el `<path>` conviene pintarlo con `fill-rule="evenodd"`:
 * los lazos no se orientan de forma consistente y con la regla por defecto un
 * agujero se rellenaría.
 */
export function fieldToPath(field: Field, opts: ContourOptions = {}): LiquidPath {
  const b = field.bounds();
  const { loops, cell, samples, evals } = contour(field, opts);
  const smooth = opts.smooth ?? 2;
  const parts: string[] = [];
  let points = 0;
  for (const loop of loops) {
    const s = smooth > 0 ? chaikin(loop, smooth) : loop;
    if (s.length < 3) continue;
    points += s.length;
    parts.push(
      `M ${fmt(s[0].x)} ${fmt(s[0].y)} ` +
        s
          .slice(1)
          .map((p) => `L ${fmt(p.x)} ${fmt(p.y)}`)
          .join(" ") +
        " Z",
    );
  }
  return {
    d: parts.join(" "),
    minX: b.minX,
    minY: b.minY,
    width: b.maxX - b.minX,
    height: b.maxY - b.minY,
    cell,
    samples,
    evals,
    points,
  };
}
