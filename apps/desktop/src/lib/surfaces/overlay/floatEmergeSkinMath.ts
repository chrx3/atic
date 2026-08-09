/** Helpers puros del idle-stop del skin (testeables sin DOM / liquid). */

export const IDLE_FRAMES = 3;
/** ~2s a 60fps: morph + stagger de favs caben; jitter eterno se corta. */
export const MAX_TRACK_FRAMES = 120;
const EPSILON = 0.5;

export type SkinRect = { x: number; y: number; w: number; h: number };

export function sameRect(a: SkinRect, b: SkinRect): boolean {
  return (
    Math.abs(a.x - b.x) < EPSILON &&
    Math.abs(a.y - b.y) < EPSILON &&
    Math.abs(a.w - b.w) < EPSILON &&
    Math.abs(a.h - b.h) < EPSILON
  );
}

/** Clave estable ante jitter subpíxel (cuantiza a 0.5px). */
export function rectKey(r: SkinRect): string {
  const q = (n: number) => (Math.round(n * 2) / 2).toFixed(1);
  return `${q(r.x)},${q(r.y)},${q(r.w)},${q(r.h)}`;
}
