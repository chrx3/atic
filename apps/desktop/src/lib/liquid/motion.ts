/**
 * Movimiento rígido e islas del grupo líquido.
 *
 * Arrastrar no tiene por qué remuestrear el campo: si un conjunto de formas
 * se traslada entero, el contorno ya trazado se mueve con un `translate`.
 * Y si dos superficies están más lejos que el alcance, no se funden — trazarlo
 * como un solo campo obliga a una grilla del AABB conjunto, que en el overlay
 * (pill en una esquina, launcher en la otra) es el escritorio entero.
 */

import type { Bounds, Shape } from "./sdf";

/** Bajo esto, dos deltas son el mismo traslado. El subpíxel no deforma. */
const SHIFT_EPS = 0.5;

export type Shift = { dx: number; dy: number };

export type Island = { id: string; shapes: Shape[] };

export function shapeAabb(s: Shape): Bounds {
  if (s.kind === "box") {
    return {
      minX: s.cx - s.hw,
      minY: s.cy - s.hh,
      maxX: s.cx + s.hw,
      maxY: s.cy + s.hh,
    };
  }
  return {
    minX: Math.min(s.ax, s.bx) - s.r,
    minY: Math.min(s.ay, s.by) - s.r,
    maxX: Math.max(s.ax, s.bx) + s.r,
    maxY: Math.max(s.ay, s.by) + s.r,
  };
}

export function unionAabb(shapes: Shape[]): Bounds | null {
  if (shapes.length === 0) return null;
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const s of shapes) {
    const b = shapeAabb(s);
    minX = Math.min(minX, b.minX);
    minY = Math.min(minY, b.minY);
    maxX = Math.max(maxX, b.maxX);
    maxY = Math.max(maxY, b.maxY);
  }
  return { minX, minY, maxX, maxY };
}

/** Hueco entre AABBs; negativo si se solapan. Misma idea que `gapBetween`. */
export function aabbGap(a: Bounds, b: Bounds): number {
  const gapX = Math.max(b.minX - a.maxX, a.minX - b.maxX);
  const gapY = Math.max(b.minY - a.maxY, a.minY - b.maxY);
  return Math.max(gapX, gapY);
}

/**
 * Traslado rígido común, o `null` si cambió el tamaño, el orden o la geometría
 * relativa (cuello que se estira).
 */
export function rigidShift(prev: Shape[], next: Shape[]): Shift | null {
  if (prev.length !== next.length || prev.length === 0) return null;
  let dx: number | null = null;
  let dy: number | null = null;
  for (let i = 0; i < prev.length; i++) {
    const a = prev[i];
    const b = next[i];
    if (!a || !b || a.kind !== b.kind) return null;
    let dxi: number;
    let dyi: number;
    if (a.kind === "box" && b.kind === "box") {
      if (a.hw !== b.hw || a.hh !== b.hh || a.r !== b.r) return null;
      dxi = b.cx - a.cx;
      dyi = b.cy - a.cy;
    } else if (a.kind === "capsule" && b.kind === "capsule") {
      if (a.r !== b.r) return null;
      dxi = b.ax - a.ax;
      dyi = b.ay - a.ay;
      if (Math.abs(b.bx - a.bx - dxi) > SHIFT_EPS) return null;
      if (Math.abs(b.by - a.by - dyi) > SHIFT_EPS) return null;
    } else {
      return null;
    }
    if (dx === null || dy === null) {
      dx = dxi;
      dy = dyi;
    } else if (Math.abs(dxi - dx) > SHIFT_EPS || Math.abs(dyi - dy) > SHIFT_EPS) {
      return null;
    }
  }
  return dx === null || dy === null ? null : { dx, dy };
}

/**
 * Agrupa superficies que todavía pueden fundirse (hueco AABB ≤ alcance).
 *
 * El `id` es estable entre cuadros: las mismas partes, ordenadas, unidas con
 * `+`. Así el Skin de cada isla sobrevive al drag y puede trasladar el path.
 *
 * `affinity` es el grupo de fusión: dos partes solo se unen si lo comparten.
 * Sin eso el clipboard pegado a la consola se volvía un solo charco — y la
 * regla del sistema es fundir solo cuando una forma **sale** de la otra (la pill).
 */
export function clusterParts(
  parts: Record<string, Shape[]>,
  reach: number,
  affinity?: Record<string, string>,
): Island[] {
  const ids = Object.keys(parts)
    .filter((id) => (parts[id]?.length ?? 0) > 0)
    .sort();
  if (ids.length === 0) return [];
  if (ids.length === 1) {
    const id = ids[0];
    return [{ id, shapes: parts[id] ?? [] }];
  }

  const bounds = ids.map((id) => unionAabb(parts[id] ?? [])!);
  const parent = ids.map((_, i) => i);
  const find = (i: number): number =>
    parent[i] === i ? i : (parent[i] = find(parent[i]!));
  const unite = (i: number, j: number) => {
    const a = find(i);
    const b = find(j);
    if (a !== b) parent[a] = b;
  };

  for (let i = 0; i < ids.length; i++) {
    for (let j = i + 1; j < ids.length; j++) {
      if (affinity) {
        const a = affinity[ids[i]!] ?? ids[i]!;
        const b = affinity[ids[j]!] ?? ids[j]!;
        if (a !== b) continue;
      }
      if (aabbGap(bounds[i]!, bounds[j]!) <= reach) unite(i, j);
    }
  }

  const groups = new Map<number, string[]>();
  for (let i = 0; i < ids.length; i++) {
    const root = find(i);
    const g = groups.get(root);
    const id = ids[i]!;
    if (g) g.push(id);
    else groups.set(root, [id]);
  }

  return [...groups.values()].map((memberIds) => {
    const sorted = memberIds.sort();
    return {
      id: sorted.join("+"),
      shapes: sorted.flatMap((id) => parts[id] ?? []),
    };
  });
}
