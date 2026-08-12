/**
 * Trazado con atajo de traslado rígido.
 *
 * Remuestrear el campo es lo caro. Si las formas solo se movieron juntas, el
 * `<path>` ya es correcto: se desplaza con CSS (`tx`/`ty`) y no se toca `d`.
 */

import { fieldToPath, type LiquidPath } from "./contour";
import { Field, type Shape } from "./sdf";
import { rigidShift } from "./motion";

const EMPTY: LiquidPath = {
  d: "",
  minX: 0,
  minY: 0,
  width: 0,
  height: 0,
  cell: 0,
  samples: 0,
  evals: 0,
  points: 0,
};

export type TraceOpts = {
  blend: number;
  cell: number;
  smooth: number;
};

export type Traced = {
  path: LiquidPath;
  tx: number;
  ty: number;
  ms: number;
};

export class PathTracer {
  #shapes: Shape[] = [];
  #path: LiquidPath = EMPTY;
  #tx = 0;
  #ty = 0;
  #blend = NaN;
  #cell = NaN;
  #smooth = NaN;

  next(shapes: Shape[], opts: TraceOpts): Traced {
    if (shapes.length === 0) {
      this.#shapes = [];
      this.#path = EMPTY;
      this.#tx = 0;
      this.#ty = 0;
      this.#blend = opts.blend;
      this.#cell = opts.cell;
      this.#smooth = opts.smooth;
      return { path: EMPTY, tx: 0, ty: 0, ms: 0 };
    }

    const qualitySame =
      this.#blend === opts.blend &&
      this.#cell === opts.cell &&
      this.#smooth === opts.smooth;
    const shift = this.#path.d ? rigidShift(this.#shapes, shapes) : null;

    // Traslado de verdad: conservar el path (aunque cambie la celda al entrar
    // en drag). Calidad distinta SIN moverse: remeshear (soltar el gesto).
    if (shift && (qualitySame || shift.dx !== 0 || shift.dy !== 0)) {
      this.#shapes = shapes.slice();
      this.#tx += shift.dx;
      this.#ty += shift.dy;
      return { path: this.#path, tx: this.#tx, ty: this.#ty, ms: 0 };
    }

    const t0 = performance.now();
    const path = fieldToPath(new Field(shapes, opts.blend), {
      cell: opts.cell,
      smooth: opts.smooth,
    });
    this.#shapes = shapes.slice();
    this.#path = path;
    this.#tx = 0;
    this.#ty = 0;
    this.#blend = opts.blend;
    this.#cell = opts.cell;
    this.#smooth = opts.smooth;
    return {
      path,
      tx: 0,
      ty: 0,
      ms: Math.round((performance.now() - t0) * 100) / 100,
    };
  }
}
