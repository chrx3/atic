import type { InkStroke, NoteBlock } from "$core/types";

export const MARGEN = 24;
/**
 * Piso del zoom.
 *
 * Con la celda en 900 de alto, una ventana baja necesita alejar bastante para
 * que la página entre entera; en 0.45 el encuadre quedaba corto y el papel se
 * veía recortado contra el borde.
 */
export const ZOOM_MIN = 0.3;
export const ZOOM_MAX = 2.4;
/** Arrastre interno del cajón (historial, textos o capturas) hacia el papel. */
export const FLIP_MIME = "application/x-atic-flip";

export type FlipFuente = "clip" | "snip" | "cap" | "meet";

export function payloadFlip(tipo: FlipFuente, id: string): string {
  return `${tipo}:${id}`;
}

export function leerPayloadFlip(raw: string): { tipo: FlipFuente; id: string } | null {
  const i = raw.indexOf(":");
  if (i < 1) return null;
  const tipo = raw.slice(0, i);
  const id = raw.slice(i + 1);
  if ((tipo !== "clip" && tipo !== "snip" && tipo !== "cap" && tipo !== "meet") || !id)
    return null;
  return { tipo, id };
}

export const BORRAR_RADIO = 16;

export const TEXTO_W = 280;
export const TEXTO_H = 96;
export const LISTA_W = 260;
export const LISTA_H = 120;

/**
 * Tipografía del texto del tablero, junta en un solo lugar.
 *
 * El `textarea` la toma por CSS (`font: inherit` → `--rb-font`) y el lienzo que
 * mide para ajustar el tamaño usa exactamente la misma: si una cambia y la otra
 * no, el ajuste miente y el texto se corta. El export lee de acá también.
 */
export const FUENTE_TABLERO =
  '"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif';
export const TEXTO_FUENTE = 13.5;
/** Piso de legibilidad: por debajo de esto no se encoge aunque no entre. */
export const TEXTO_FUENTE_MIN = 8.5;
export const TEXTO_INTERLINEADO = 1.5;
/** Padding interno del recuadro de texto: espeja `.objeto textarea`. */
export const TEXTO_PAD_X = 24;
export const TEXTO_PAD_Y = 28;
/** Alto de una caja de una línea sin encoger: un encabezado cabe con aire. */
export const TEXTO_ALTO_MIN = 48;

export type Herramienta = "select" | "draw" | "highlight" | "eraser" | "text" | "check";

/**
 * Página lógica del tablero, en píxeles de papel. Todo empieza en una.
 *
 * Es también la página del export (una celda = una página), así que agrandarla
 * da más lugar para trabajar sin agregar páginas. 4:3: entra una minuta entera
 * sin partirse y la tarjeta del reverso —que es 4:3— la muestra completa.
 */
export const PAGINA_W = 1200;
export const PAGINA_H = 900;

/** Cuántas celdas hay, de izquierda a derecha. */
export function cantidadPaginas(papelW: number): number {
  return Math.max(1, Math.round(papelW / PAGINA_W));
}
/** Cota contra datos corruptos (un bloque en y=1e9 no puede romper el papel). */
export const TABLERO_MAX = 12000;

/** Ancho máximo al pegar una imagen: cabe en la página con margen. */
const IMAGEN_MAX = PAGINA_W - MARGEN * 2;

/** Rectángulo del papel: origen (puede ser negativo) más tamaño. */
export interface Tablero {
  ox: number;
  oy: number;
  w: number;
  h: number;
}

export function marcoDe(bloque: NoteBlock): {
  x: number;
  y: number;
  w: number;
  h: number;
} {
  return {
    x: bloque.x ?? 0,
    y: bloque.y ?? 0,
    w: bloque.w ?? 0,
    h: bloque.h ?? 0,
  };
}

export type Marco = { x: number; y: number; w: number; h: number };

/**
 * Intersección de un marco con una celda, en coords locales de la celda.
 *
 * Vive acá y no en el export porque el tablero también necesita saber qué cae
 * en cada página: la tira de miniaturas filtra con esto.
 */
export function marcoEnPagina(
  x: number,
  y: number,
  w: number,
  h: number,
  indice: number,
): Marco | null {
  const x0 = indice * PAGINA_W;
  const izq = Math.max(x, x0);
  const der = Math.min(x + w, x0 + PAGINA_W);
  const arr = Math.max(y, 0);
  const aba = Math.min(y + h, PAGINA_H);
  if (der - izq < 1 || aba - arr < 1) return null;
  return { x: izq - x0, y: arr, w: der - izq, h: aba - arr };
}

/** ¿Este bloque toca la celda `indice`? La tinta se mide por sus puntos. */
export function bloqueEnPagina(bloque: NoteBlock, indice: number): boolean {
  if (bloque.kind === "ink") {
    const x0 = indice * PAGINA_W;
    const x1 = x0 + PAGINA_W;
    return bloque.strokes.some((trazo) =>
      trazo.points.some(([x, y]) => x >= x0 && x < x1 && y >= 0 && y < PAGINA_H),
    );
  }
  const m = marcoDe(bloque);
  return marcoEnPagina(m.x, m.y, m.w, m.h, indice) !== null;
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
  ox = 0,
  oy = 0,
): { x: number; y: number } {
  const escala = papel.width / anchoLogico;
  if (escala <= 0) return { x: ox, y: oy };
  return {
    x: ox + (clientX - papel.left) / escala,
    y: oy + (clientY - papel.top) / escala,
  };
}

/** Líneas interiores entre páginas, en coordenadas del papel. */
export function lineasPagina(tablero: Tablero): {
  verticales: number[];
  horizontales: number[];
} {
  const verticales: number[] = [];
  for (
    let x = (Math.floor(tablero.ox / PAGINA_W) + 1) * PAGINA_W;
    x < tablero.ox + tablero.w;
    x += PAGINA_W
  ) {
    verticales.push(x - tablero.ox);
  }
  const horizontales: number[] = [];
  for (
    let y = (Math.floor(tablero.oy / PAGINA_H) + 1) * PAGINA_H;
    y < tablero.oy + tablero.h;
    y += PAGINA_H
  ) {
    horizontales.push(y - tablero.oy);
  }
  return { verticales, horizontales };
}

export function tamanoImagen(
  anchoNat: number,
  altoNat: number,
): { w: number; h: number } {
  const w = Math.min(IMAGEN_MAX, Math.max(80, anchoNat));
  const ratio = altoNat <= 0 ? 1 : altoNat / Math.max(1, anchoNat);
  return { w, h: Math.max(48, w * ratio) };
}

/** Ancho del papel para que ningún objeto quede fuera de una celda. */
export function anchoParaBloques(lista: NoteBlock[]): number {
  let maxX = PAGINA_W;
  for (const bloque of lista) {
    if (bloque.kind === "ink") continue;
    const m = marcoDe(bloque);
    maxX = Math.max(maxX, m.x + m.w + MARGEN);
  }
  const celdas = Math.max(1, Math.ceil(maxX / PAGINA_W));
  return Math.min(TABLERO_MAX, celdas * PAGINA_W);
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

/**
 * Envuelve palabras al ancho dado; `medir` mide en px una cadena.
 *
 * Vive acá y no en el export porque el tablero también necesita saber cuánto
 * ocupa un texto antes de dibujarlo.
 */
export function envolver(
  texto: string,
  maxWidth: number,
  medir: (s: string) => number,
): string[] {
  const palabras = texto.split(/\s+/).filter(Boolean);
  if (palabras.length === 0) return [""];
  const lineas: string[] = [];
  let actual = palabras[0] ?? "";
  for (const palabra of palabras.slice(1)) {
    const prueba = `${actual} ${palabra}`;
    if (medir(prueba) <= maxWidth) actual = prueba;
    else {
      lineas.push(actual);
      actual = palabra;
    }
  }
  lineas.push(actual);
  return lineas;
}

/** Alto que pide el cuerpo a un tamaño de fuente dado. */
export function altoEnvolvente(
  body: string,
  ancho: number,
  tamano: number,
  medir: (s: string, tamano: number) => number,
): number {
  const util = Math.max(8, ancho - TEXTO_PAD_X);
  let lineas = 0;
  for (const cruda of body.split("\n")) {
    lineas += envolver(cruda, util, (s) => medir(s, tamano)).length;
  }
  return TEXTO_PAD_Y + lineas * tamano * TEXTO_INTERLINEADO;
}

/**
 * El tamaño de fuente más grande que hace entrar el cuerpo en la caja.
 *
 * Nunca sube del base ni baja del mínimo. Sirve para que encoger una nota no
 * esconda lo que dice: antes el `textarea` scrolleaba sin barra y con
 * `pointer-events: none` —o sea, el texto simplemente desaparecía.
 */
export function ajustarFuente(
  body: string,
  ancho: number,
  alto: number,
  medir: (s: string, tamano: number) => number,
  base = TEXTO_FUENTE,
  minimo = TEXTO_FUENTE_MIN,
): number {
  if (!body.trim() || ancho <= 0 || alto <= 0) return base;
  const entra = (t: number) => altoEnvolvente(body, ancho, t, medir) <= alto;
  if (entra(base)) return base;
  let chico = minimo;
  let grande = base;
  for (let i = 0; i < 12; i++) {
    const medio = (chico + grande) / 2;
    if (entra(medio)) chico = medio;
    else grande = medio;
  }
  return Math.max(minimo, Math.round(chico * 10) / 10);
}

/**
 * Borra por donde pasa el borrador: quita los puntos dentro del radio y parte
 * cada trazo en los tramos que quedan. Los tramos de un solo punto se van.
 */
export function borrarPuntos(
  trazos: InkStroke[],
  x: number,
  y: number,
  radio: number,
): InkStroke[] {
  const salida: InkStroke[] = [];
  for (const trazo of trazos) {
    let tramo: [number, number][] = [];
    const cerrar = () => {
      if (tramo.length >= 2) salida.push({ ...trazo, points: tramo });
      tramo = [];
    };
    for (const [px, py] of trazo.points) {
      if (Math.hypot(px - x, py - y) <= radio) cerrar();
      else tramo.push([px, py]);
    }
    cerrar();
  }
  return salida;
}

/** Tiñe un hex `#rrggbb` con alfa para el resaltador; lo demás pasa igual. */
export function conAlfa(hex: string, alfaHex = "66"): string {
  return /^#[0-9a-fA-F]{6}$/.test(hex) ? `${hex}${alfaHex}` : hex;
}

/** Borra el trazo entero más cercano al punto; no toca nada si está lejos. */
export function borrarLineaCercana(
  trazos: InkStroke[],
  x: number,
  y: number,
  radio: number,
): { trazos: InkStroke[]; borro: boolean } {
  let mejor: InkStroke | null = null;
  let mejorD = Infinity;
  for (const trazo of trazos) {
    for (const [px, py] of trazo.points) {
      const d = Math.hypot(px - x, py - y);
      if (d < mejorD) {
        mejorD = d;
        mejor = trazo;
      }
    }
  }
  if (!mejor || mejorD > radio) return { trazos, borro: false };
  return { trazos: trazos.filter((t) => t !== mejor), borro: true };
}
