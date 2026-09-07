import type { NoteBlock } from "$core/types";

export const PAPEL_ANCHO = 640;
export const PAPEL_MIN_ALTO = 520;
export const MARGEN = 24;
export const ZOOM_MIN = 0.45;
export const ZOOM_MAX = 2.4;
export const CLIP_MIME = "application/x-atic-clip";

const TEXTO_W = 280;
const TEXTO_H = 96;
const LISTA_W = 260;
const LISTA_H = 120;
const IMAGEN_MAX = PAPEL_ANCHO - MARGEN * 2;

export type Herramienta = "select" | "draw" | "text" | "check";

export function marcoDe(bloque: NoteBlock): { x: number; y: number; w: number; h: number } {
  return {
    x: bloque.x ?? 0,
    y: bloque.y ?? 0,
    w: bloque.w ?? 0,
    h: bloque.h ?? 0,
  };
}

export function tieneMarco(bloque: NoteBlock): boolean {
  return (bloque.w ?? 0) > 0 && (bloque.h ?? 0) > 0;
}

export function clampZoom(valor: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, valor));
}

export function puntoEnPapel(
  papel: DOMRect,
  clientX: number,
  clientY: number,
  anchoLogico: number,
): { x: number; y: number } {
  const escala = papel.width / anchoLogico;
  if (escala <= 0) return { x: 0, y: 0 };
  return {
    x: (clientX - papel.left) / escala,
    y: (clientY - papel.top) / escala,
  };
}

export function tamanoImagen(anchoNat: number, altoNat: number): { w: number; h: number } {
  const w = Math.min(IMAGEN_MAX, Math.max(80, anchoNat));
  const ratio = altoNat <= 0 ? 1 : altoNat / Math.max(1, anchoNat);
  return { w, h: Math.max(48, w * ratio) };
}

/** Notas viejas (todo en 0) se apilan. Las que ya tienen marco se dejan. */
export function colocarSiHaceFalta(lista: NoteBlock[]): NoteBlock[] {
  const objetos = lista.filter((b) => b.kind !== "ink");
  if (objetos.length === 0 || objetos.every(tieneMarco)) return lista;
  let y = MARGEN;
  return lista.map((bloque) => {
    if (bloque.kind === "ink" || tieneMarco(bloque)) return bloque;
    if (bloque.kind === "image") {
      const { w, h } = tamanoImagen(bloque.width, bloque.height);
      const next = { ...bloque, x: MARGEN, y, w, h };
      y += h + 16;
      return next;
    }
    if (bloque.kind === "check") {
      const next = { ...bloque, x: MARGEN, y, w: LISTA_W, h: LISTA_H };
      y += LISTA_H + 16;
      return next;
    }
    const next = { ...bloque, x: MARGEN, y, w: TEXTO_W, h: TEXTO_H };
    y += TEXTO_H + 16;
    return next;
  });
}

export function altoPapel(lista: NoteBlock[]): number {
  let max = PAPEL_MIN_ALTO;
  for (const bloque of lista) {
    if (bloque.kind === "ink") {
      for (const trazo of bloque.strokes) {
        for (const [, py] of trazo.points) max = Math.max(max, py + 48);
      }
      continue;
    }
    const { y, h } = marcoDe(bloque);
    max = Math.max(max, y + h + MARGEN);
  }
  return max;
}

export function centrar(w: number, h: number, enX: number, enY: number): { x: number; y: number } {
  return {
    x: Math.max(8, Math.min(PAPEL_ANCHO - w - 8, enX - w / 2)),
    y: Math.max(8, enY - h / 2),
  };
}
