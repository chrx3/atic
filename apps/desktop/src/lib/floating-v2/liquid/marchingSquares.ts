import { DistanceField } from "./sdf";

type Point = { x: number; y: number };
type Segment = [Point, Point];

export type MarchOptions = {
  cell?: number;
  smooth?: number;
};

const CASE_EDGES: number[][][] = [
  [],
  [[3, 2]],
  [[2, 1]],
  [[3, 1]],
  [[0, 1]],
  [[3, 2], [0, 1]],
  [[0, 2]],
  [[3, 0]],
  [[3, 0]],
  [[0, 2]],
  [[3, 0], [2, 1]],
  [[0, 1]],
  [[3, 1]],
  [[2, 1]],
  [[3, 2]],
  [],
];

function interpolate(
  x0: number,
  y0: number,
  value0: number,
  x1: number,
  y1: number,
  value1: number,
): Point {
  const denominator = value0 - value1;
  const raw = Math.abs(denominator) < 1e-6 ? 0.5 : value0 / denominator;
  const t = Math.max(0, Math.min(1, raw));
  return { x: x0 + (x1 - x0) * t, y: y0 + (y1 - y0) * t };
}

function stitch(segments: Segment[], cell: number): Point[][] {
  const epsilon = cell * 0.5;
  const key = (point: Point) => `${Math.round(point.x / epsilon)},${Math.round(point.y / epsilon)}`;
  const adjacency = new Map<string, { segment: number; end: 0 | 1 }[]>();

  segments.forEach((segment, index) => {
    for (const end of [0, 1] as const) {
      const endpoint = key(segment[end]);
      const entries = adjacency.get(endpoint);
      if (entries) entries.push({ segment: index, end });
      else adjacency.set(endpoint, [{ segment: index, end }]);
    }
  });

  const used = new Array<boolean>(segments.length).fill(false);
  const loops: Point[][] = [];
  for (let start = 0; start < segments.length; start += 1) {
    if (used[start]) continue;
    const loop: Point[] = [];
    let current = start;
    let end: 0 | 1 = 0;
    let guard = 0;

    while (!used[current] && guard < segments.length + 2) {
      guard += 1;
      used[current] = true;
      const segment = segments[current];
      const opposite = (end === 0 ? 1 : 0) as 0 | 1;
      loop.push(segment[end]);
      const candidates = adjacency.get(key(segment[opposite])) ?? [];
      const next = candidates.find((candidate) => !used[candidate.segment]);
      if (!next) break;
      current = next.segment;
      end = next.end;
    }

    if (loop.length >= 3) loops.push(loop);
  }
  return loops;
}

function contour(field: DistanceField, options: MarchOptions): Point[][] {
  const cell = Math.max(2, options.cell ?? 6);
  const bounds = field.bounds();
  const width = bounds.maxX - bounds.minX;
  const height = bounds.maxY - bounds.minY;
  if (width <= 0 || height <= 0) return [];

  const columns = Math.ceil(width / cell) + 1;
  const rows = Math.ceil(height / cell) + 1;
  if (columns * rows > 400_000) return [];

  const samples = new Float32Array(columns * rows);
  for (let row = 0; row < rows; row += 1) {
    const y = bounds.minY + row * cell;
    for (let column = 0; column < columns; column += 1) {
      samples[row * columns + column] = field.evaluate(bounds.minX + column * cell, y);
    }
  }
  const sample = (column: number, row: number) => samples[row * columns + column];
  const segments: Segment[] = [];

  for (let row = 0; row < rows - 1; row += 1) {
    for (let column = 0; column < columns - 1; column += 1) {
      const x0 = bounds.minX + column * cell;
      const y0 = bounds.minY + row * cell;
      const x1 = x0 + cell;
      const y1 = y0 + cell;
      const topLeft = sample(column, row);
      const topRight = sample(column + 1, row);
      const bottomRight = sample(column + 1, row + 1);
      const bottomLeft = sample(column, row + 1);

      let mask = 0;
      if (topLeft < 0) mask |= 8;
      if (topRight < 0) mask |= 4;
      if (bottomRight < 0) mask |= 2;
      if (bottomLeft < 0) mask |= 1;
      if (mask === 0 || mask === 15) continue;

      const pointOnEdge = (edge: number): Point => {
        if (edge === 0) return interpolate(x0, y0, topLeft, x1, y0, topRight);
        if (edge === 1) return interpolate(x1, y0, topRight, x1, y1, bottomRight);
        if (edge === 2) return interpolate(x0, y1, bottomLeft, x1, y1, bottomRight);
        return interpolate(x0, y0, topLeft, x0, y1, bottomLeft);
      };

      let cases = CASE_EDGES[mask];
      if (mask === 5 || mask === 10) {
        const centerInside = field.evaluate((x0 + x1) / 2, (y0 + y1) / 2) < 0;
        if (mask === 5) cases = centerInside ? [[3, 0], [2, 1]] : [[3, 2], [0, 1]];
        else cases = centerInside ? [[3, 2], [0, 1]] : [[3, 0], [2, 1]];
      }
      for (const [first, second] of cases) segments.push([pointOnEdge(first), pointOnEdge(second)]);
    }
  }

  return stitch(segments, cell);
}

function chaikin(points: Point[], passes: number): Point[] {
  let result = points;
  for (let pass = 0; pass < passes; pass += 1) {
    const next: Point[] = [];
    for (let index = 0; index < result.length; index += 1) {
      const a = result[index];
      const b = result[(index + 1) % result.length];
      next.push({ x: a.x * 0.75 + b.x * 0.25, y: a.y * 0.75 + b.y * 0.25 });
      next.push({ x: a.x * 0.25 + b.x * 0.75, y: a.y * 0.25 + b.y * 0.75 });
    }
    result = next;
  }
  return result;
}

export function fieldToPath(field: DistanceField, options: MarchOptions = {}) {
  const bounds = field.bounds();
  const smoothing = options.smooth ?? 2;
  const format = (value: number) => value.toFixed(2);
  const paths = contour(field, options).map((loop) => {
    const points = smoothing > 0 ? chaikin(loop, smoothing) : loop;
    if (points.length < 3) return "";
    return `M ${format(points[0].x)} ${format(points[0].y)} ${points
      .slice(1)
      .map((point) => `L ${format(point.x)} ${format(point.y)}`)
      .join(" ")} Z`;
  });

  return {
    d: paths.filter(Boolean).join(" "),
    minX: bounds.minX,
    minY: bounds.minY,
    width: bounds.maxX - bounds.minX,
    height: bounds.maxY - bounds.minY,
  };
}
