/**
 * Letras en LRC: una marca `[mm:ss.xx]` por línea, a veces varias si el verso
 * se repite. Se ignoran las etiquetas de cabecera (`[ar:…]`, `[offset:…]`).
 */

export type LyricLine = { at: number; text: string };

const STAMP = /\[(\d+):(\d{1,2}(?:[.:]\d{1,3})?)\]/g;

export function parseLrc(lrc: string): LyricLine[] {
  const lines: LyricLine[] = [];
  for (const raw of lrc.split(/\r?\n/)) {
    const stamps = [...raw.matchAll(STAMP)];
    if (stamps.length === 0) continue;
    const text = raw.replace(STAMP, "").trim();
    for (const [, min, sec] of stamps) {
      const at = (Number(min) * 60 + Number(sec.replace(":", "."))) * 1000;
      if (Number.isFinite(at)) lines.push({ at: Math.round(at), text });
    }
  }
  return lines.sort((a, b) => a.at - b.at);
}

/** La línea que suena en `position` (ms), o -1 antes de la primera. */
export function lyricIndex(lines: LyricLine[], position: number): number {
  let lo = 0;
  let hi = lines.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].at <= position) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}
