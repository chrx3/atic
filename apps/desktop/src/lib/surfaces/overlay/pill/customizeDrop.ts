/**
 * Dónde cae una ficha arrastrada en el editor de la pill.
 *
 * Se calcula con la grilla y no midiendo las fichas: al abrir el hueco en la
 * fila destino, las fichas se corren, y medir sus posiciones movería el hueco
 * otra vez — el índice oscilaría bajo el cursor. Las fichas son todas del
 * mismo tamaño, así que la grilla basta y no depende de lo que se ve.
 */
import type { PillBucket } from "$core/pillTools";

/** Lado de una ficha y hueco entre fichas (px). El CSS del editor usa estos. */
export const CUSTOMIZE_CHIP = 34;
export const CUSTOMIZE_GAP = 6;

export type DropRect = { x: number; y: number; w: number; h: number };
export type DropPoint = { x: number; y: number };

export type DropRow = {
  bucket: PillBucket;
  /** Toda la sección (rótulo + fichas): el blanco para elegir fila. */
  zone: DropRect;
  /** Caja de las fichas, donde empieza la grilla. */
  grid: DropRect;
  /** Fichas en la fila SIN la arrastrada. */
  count: number;
};

export type DropSlot = { bucket: PillBucket; index: number };

/** Posición de inserción dentro de una fila, por grilla. */
export function slotIndex(grid: DropRect, count: number, p: DropPoint): number {
  const pitch = CUSTOMIZE_CHIP + CUSTOMIZE_GAP;
  const perLine = Math.max(1, Math.floor((grid.w + CUSTOMIZE_GAP) / pitch));
  const line = Math.max(0, Math.floor((p.y - grid.y) / pitch));
  // Cuántas fichas de esa línea tienen el centro antes del cursor.
  const col = Math.max(
    0,
    Math.min(perLine, Math.floor((p.x - grid.x - CUSTOMIZE_CHIP / 2) / pitch) + 1),
  );
  return Math.max(0, Math.min(count, line * perLine + col));
}

/** Fila bajo el cursor (o la más cercana en vertical) y su índice. */
export function dropSlot(rows: readonly DropRow[], p: DropPoint): DropSlot | null {
  if (rows.length === 0) return null;
  let best = rows[0];
  let bestDist = Infinity;
  for (const row of rows) {
    const { y, h } = row.zone;
    const dist = p.y < y ? y - p.y : p.y > y + h ? p.y - (y + h) : 0;
    if (dist < bestDist) {
      best = row;
      bestDist = dist;
    }
  }
  return { bucket: best.bucket, index: slotIndex(best.grid, best.count, p) };
}
