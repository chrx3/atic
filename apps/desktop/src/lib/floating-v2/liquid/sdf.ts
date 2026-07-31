/** Signed-distance primitives used by the liquid surface. Distances are
 * negative inside a shape, zero at its edge, and positive outside. */

export function sdRoundBox(
  px: number,
  py: number,
  cx: number,
  cy: number,
  hw: number,
  hh: number,
  radius: number,
): number {
  const r = Math.min(radius, Math.min(hw, hh));
  const qx = Math.abs(px - cx) - hw + r;
  const qy = Math.abs(py - cy) - hh + r;
  const ax = Math.max(qx, 0);
  const ay = Math.max(qy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(qx, qy), 0) - r;
}

export function sdCapsule(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
  radius: number,
): number {
  const pax = px - ax;
  const pay = py - ay;
  const bax = bx - ax;
  const bay = by - ay;
  const denominator = bax * bax + bay * bay || 1e-6;
  const projection = Math.max(0, Math.min(1, (pax * bax + pay * bay) / denominator));
  return Math.hypot(pax - bax * projection, pay - bay * projection) - radius;
}

/** Polynomial smooth minimum. `k=0` is a hard union; larger values create a
 * wider inverse fillet and eventually an organic neck. */
export function smoothMin(a: number, b: number, k: number): number {
  if (k <= 0) return Math.min(a, b);
  const h = Math.max(0, Math.min(1, 0.5 + (0.5 * (b - a)) / k));
  return b * (1 - h) + a * h - k * h * (1 - h);
}

export type RoundBoxShape = {
  kind: "box";
  cx: number;
  cy: number;
  hw: number;
  hh: number;
  r: number;
};

export type BridgeShape = {
  kind: "bridge";
  ax: number;
  ay: number;
  bx: number;
  by: number;
  r: number;
};

export type LiquidShape = RoundBoxShape | BridgeShape;

function shapeDistance(shape: LiquidShape, x: number, y: number): number {
  return shape.kind === "box"
    ? sdRoundBox(x, y, shape.cx, shape.cy, shape.hw, shape.hh, shape.r)
    : sdCapsule(x, y, shape.ax, shape.ay, shape.bx, shape.by, shape.r);
}

export class DistanceField {
  constructor(
    readonly shapes: LiquidShape[] = [],
    readonly k = 12,
  ) {}

  evaluate(x: number, y: number): number {
    if (this.shapes.length === 0) return Number.POSITIVE_INFINITY;
    let distance = shapeDistance(this.shapes[0], x, y);
    for (let index = 1; index < this.shapes.length; index += 1) {
      distance = smoothMin(distance, shapeDistance(this.shapes[index], x, y), this.k);
    }
    return distance;
  }

  bounds(padding = 0) {
    let minX = Number.POSITIVE_INFINITY;
    let minY = Number.POSITIVE_INFINITY;
    let maxX = Number.NEGATIVE_INFINITY;
    let maxY = Number.NEGATIVE_INFINITY;
    const grow = padding + this.k;

    for (const shape of this.shapes) {
      if (shape.kind === "box") {
        minX = Math.min(minX, shape.cx - shape.hw - grow);
        minY = Math.min(minY, shape.cy - shape.hh - grow);
        maxX = Math.max(maxX, shape.cx + shape.hw + grow);
        maxY = Math.max(maxY, shape.cy + shape.hh + grow);
      } else {
        minX = Math.min(minX, Math.min(shape.ax, shape.bx) - shape.r - grow);
        minY = Math.min(minY, Math.min(shape.ay, shape.by) - shape.r - grow);
        maxX = Math.max(maxX, Math.max(shape.ax, shape.bx) + shape.r + grow);
        maxY = Math.max(maxY, Math.max(shape.ay, shape.by) + shape.r + grow);
      }
    }

    if (!Number.isFinite(minX)) return { minX: 0, minY: 0, maxX: 0, maxY: 0 };
    return { minX, minY, maxX, maxY };
  }
}
