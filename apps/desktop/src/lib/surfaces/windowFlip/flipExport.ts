/**
 * Una celda del tablero = una página al exportar.
 *
 * PNG/JPEG: foto de la celda. PDF, Word y PowerPoint llevan las imágenes
 * originales (con transparencia) y el texto nativo, no un pantallazo.
 *
 * La geometría, el orden de dibujo y la maquetación de texto viven en funciones
 * puras y exportadas: el `environment: "node"` de la suite no tiene canvas, así
 * que la única forma de testear esta lógica es sacarla de adentro del canvas.
 */
import type { NoteBlock } from "$core/types";
import {
  bloqueEnPagina,
  cantidadPaginas,
  envolver,
  FUENTE_TABLERO,
  marcoDe,
  marcoEnPagina,
  PAGINA_H,
  PAGINA_W,
  TEXTO_FUENTE,
  TEXTO_INTERLINEADO,
  type Marco,
} from "./flipLayout";

export type FormatoTablero = "png" | "jpeg" | "pdf" | "docx" | "pptx";

export const FORMATOS_TABLERO: FormatoTablero[] = [
  "png",
  "jpeg",
  "pdf",
  "docx",
  "pptx",
];

/**
 * Tipografía del texto rasterizado. Debe reflejar la nota del tablero: el
 * `textarea` usa `font: inherit` → `--rb-font`, 13.5px, interlineado 1.5. Si
 * cambia la fuente del tablero, cambiar acá también (no hay forma de leer el CSS
 * desde el canvas). El interlineado se deriva del tamaño, no es un número suelto.
 */
/** El canvas dibuja con la misma tipografía que mide el tablero. */
const ALTO_LINEA = Math.round(TEXTO_FUENTE * TEXTO_INTERLINEADO);
const FUENTE_CSS = `${TEXTO_FUENTE}px ${FUENTE_TABLERO}`;
/** Sangrías internas del recuadro de texto/lista, en px de papel. */
const PAD_X = 8;
const PAD_Y = 10;
/** Margen inferior antes de recortar: nada se dibuja pasado el fondo del marco. */
const PAD_FONDO = 8;
/** Casilla del check y separación del texto respecto del borde izquierdo. */
const CASILLA = 10;
const SANGRIA_CHECK = 24;
const ALTO_CHECK = 22;
const TINTA_TEXTO = "#1c1917";
/** Caja translúcida detrás del texto cuando el fondo no es transparente. */
const CAJA_TEXTO = "rgba(255,255,255,0.55)";

export type ExportFoto = {
  imageBase64: string;
  mime: string;
  x: number;
  y: number;
  w: number;
  h: number;
};

export type ExportTexto = {
  body: string;
  x: number;
  y: number;
  w: number;
  h: number;
};

export type ExportLista = {
  items: { text: string; done: boolean }[];
  x: number;
  y: number;
  w: number;
  h: number;
};

export type ExportPagina = {
  previewBase64: string;
  previewMime: string;
  fotos: ExportFoto[];
  textos: ExportTexto[];
  checks: ExportLista[];
  inkBase64: string;
};

/** Resultado de `armarExport`: las páginas y los assets que no se pudieron leer. */
export type ResultadoExport = { paginas: ExportPagina[]; fallos: string[] };

type BloqueImagen = Extract<NoteBlock, { kind: "image" }>;
type BloqueTexto = Extract<NoteBlock, { kind: "text" }>;
type BloqueCheck = Extract<NoteBlock, { kind: "check" }>;

/** Una línea ya maquetada, lista para `fillText`. */
export type LineaTexto = { texto: string; x: number; y: number };

/** Un ítem de checklist ya maquetado: caja de la casilla y base del texto. */
export type LineaCheck = {
  text: string;
  done: boolean;
  x: number;
  y: number;
  caja: Marco;
};

export function lineasDePagina(bloques: NoteBlock[], indice: number): string[] {
  const lineas: string[] = [];
  for (const bloque of bloques) {
    if (!bloqueEnPagina(bloque, indice)) continue;
    if (bloque.kind === "text" && bloque.body.trim()) {
      lineas.push(...bloque.body.split("\n"));
    } else if (bloque.kind === "check") {
      for (const item of bloque.items) {
        lineas.push(`${item.done ? "[x]" : "[ ]"} ${item.text}`);
      }
    }
  }
  return lineas;
}

export function estructuraPagina(
  bloques: NoteBlock[],
  indice: number,
): {
  textos: ExportTexto[];
  checks: ExportLista[];
  fotos: { asset: string; x: number; y: number; w: number; h: number }[];
} {
  const textos: ExportTexto[] = [];
  const checks: ExportLista[] = [];
  const fotos: { asset: string; x: number; y: number; w: number; h: number }[] = [];
  for (const bloque of bloques) {
    if (bloque.kind === "ink") continue;
    const m = marcoDe(bloque);
    const recorte = marcoEnPagina(m.x, m.y, m.w, m.h, indice);
    if (!recorte) continue;
    if (bloque.kind === "text") textos.push({ body: bloque.body, ...recorte });
    else if (bloque.kind === "check") {
      checks.push({
        items: bloque.items.map((item) => ({ text: item.text, done: item.done })),
        ...recorte,
      });
    } else if (bloque.kind === "image") {
      fotos.push({ asset: bloque.asset, ...recorte });
    }
  }
  return { textos, checks, fotos };
}

/**
 * Qué pintar en una celda y en qué orden. La tinta va SIEMPRE última: un dibujo
 * se hace sobre las fotos y el texto, igual que en el tablero. El marco es el
 * completo (`marcoDe`): el canvas traslada la celda y recorta solo.
 */
export type PinturaBloque =
  | { tipo: "image"; bloque: BloqueImagen; marco: Marco }
  | { tipo: "text"; bloque: BloqueTexto; marco: Marco }
  | { tipo: "check"; bloque: BloqueCheck; marco: Marco }
  | { tipo: "ink"; bloque: Extract<NoteBlock, { kind: "ink" }> };

export function ordenDePintura(bloques: NoteBlock[], indice: number): PinturaBloque[] {
  const contenido: PinturaBloque[] = [];
  let tinta: PinturaBloque | null = null;
  for (const bloque of bloques) {
    if (!bloqueEnPagina(bloque, indice)) continue;
    if (bloque.kind === "ink") {
      tinta = { tipo: "ink", bloque };
      continue;
    }
    const marco = marcoDe(bloque);
    if (bloque.kind === "image") contenido.push({ tipo: "image", bloque, marco });
    else if (bloque.kind === "text") contenido.push({ tipo: "text", bloque, marco });
    else contenido.push({ tipo: "check", bloque, marco });
  }
  return tinta ? [...contenido, tinta] : contenido;
}

/** Envuelve palabras al ancho dado; `medir` mide el ancho en px de una cadena. */
/**
 * Maqueta el cuerpo de un texto en líneas absolutas. Recibe `medir` inyectado
 * para no depender del canvas: los tests le pasan un medidor falso. Respeta los
 * `\n` explícitos y corta al llegar al fondo del marco.
 */
export function distribuirTexto(
  body: string,
  marco: Marco,
  medir: (s: string) => number,
): LineaTexto[] {
  const lineas: LineaTexto[] = [];
  const fondo = marco.y + marco.h - PAD_FONDO;
  const ancho = marco.w - PAD_X * 2;
  let y = marco.y + PAD_Y;
  for (const cruda of body.split("\n")) {
    for (const linea of envolver(cruda, ancho, medir)) {
      if (y > fondo) return lineas;
      lineas.push({ texto: linea, x: marco.x + PAD_X, y });
      y += ALTO_LINEA;
    }
  }
  return lineas;
}

/** Maqueta un checklist: casilla + base del texto por ítem, cortando al fondo. */
export function distribuirChecks(
  items: { text: string; done: boolean }[],
  marco: Marco,
): LineaCheck[] {
  const filas: LineaCheck[] = [];
  const fondo = marco.y + marco.h - PAD_FONDO;
  let y = marco.y + PAD_Y;
  for (const item of items) {
    if (y > fondo) break;
    filas.push({
      text: item.text,
      done: item.done,
      x: marco.x + SANGRIA_CHECK,
      y,
      caja: { x: marco.x + PAD_X, y: y + 2, w: CASILLA, h: CASILLA },
    });
    y += ALTO_CHECK;
  }
  return filas;
}

function hexStroke(color: string): string {
  return color.length === 9 ? color.slice(0, 7) : color;
}

function alfaStroke(color: string): number {
  if (color.length !== 9) return 1;
  return parseInt(color.slice(7, 9), 16) / 255;
}

function pintarFoto(src: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = src;
  });
}

function partirDataUrl(data: string): { mime: string; imageBase64: string } {
  const match = data.match(/^data:([^;]+);base64,([\s\S]+)$/);
  if (match) return { mime: match[1] ?? "image/png", imageBase64: match[2] ?? "" };
  return { mime: "image/png", imageBase64: data };
}

async function cargarFoto(
  dataDe: () => Promise<string>,
): Promise<HTMLImageElement | null> {
  try {
    return await pintarFoto(await dataDe());
  } catch {
    return null;
  }
}

function dibujarTinta(ctx: CanvasRenderingContext2D, bloques: NoteBlock[]): void {
  const tinta = bloques.find((b) => b.kind === "ink");
  if (tinta?.kind !== "ink") return;
  for (const trazo of tinta.strokes) {
    if (trazo.points.length < 2) continue;
    ctx.beginPath();
    ctx.strokeStyle = hexStroke(trazo.color);
    ctx.globalAlpha = alfaStroke(trazo.color);
    ctx.lineWidth = trazo.width;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.moveTo(trazo.points[0][0], trazo.points[0][1]);
    for (const [x, y] of trazo.points.slice(1)) ctx.lineTo(x, y);
    ctx.stroke();
    ctx.globalAlpha = 1;
  }
}

function rasterTinta(bloques: NoteBlock[], indice: number): string {
  const tinta = bloques.find((b) => b.kind === "ink");
  if (tinta?.kind !== "ink" || !bloqueEnPagina(tinta, indice)) return "";
  const escala = 2;
  const canvas = document.createElement("canvas");
  canvas.width = PAGINA_W * escala;
  canvas.height = PAGINA_H * escala;
  const ctx = canvas.getContext("2d", { alpha: true });
  if (!ctx) return "";
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.scale(escala, escala);
  ctx.translate(-indice * PAGINA_W, 0);
  dibujarTinta(ctx, bloques);
  const data = canvas.toDataURL("image/png");
  return data.split(",")[1] ?? "";
}

/** Pinta un ítem de check ya maquetado: casilla, tilde y texto atenuado si va. */
function pintarCheck(ctx: CanvasRenderingContext2D, fila: LineaCheck): void {
  ctx.strokeStyle = TINTA_TEXTO;
  ctx.strokeRect(fila.caja.x, fila.caja.y, fila.caja.w, fila.caja.h);
  if (fila.done) {
    ctx.beginPath();
    ctx.moveTo(fila.caja.x + 2, fila.caja.y + 5);
    ctx.lineTo(fila.caja.x + 4, fila.caja.y + 8);
    ctx.lineTo(fila.caja.x + 8, fila.caja.y + 2);
    ctx.stroke();
  }
  ctx.globalAlpha = fila.done ? 0.45 : 1;
  ctx.fillText(fila.text, fila.x, fila.y);
  ctx.globalAlpha = 1;
}

export async function rasterPaginas(
  bloques: NoteBlock[],
  papelW: number,
  hoja: string,
  dataDe: (asset: string) => Promise<string>,
  mime: "image/png" | "image/jpeg" = "image/png",
  transparente = true,
): Promise<{ base64: string; width: number; height: number }[]> {
  const n = cantidadPaginas(papelW);
  const escala = 2;
  const w = PAGINA_W * escala;
  const h = PAGINA_H * escala;
  const salida: { base64: string; width: number; height: number }[] = [];
  const cache = new Map<string, HTMLImageElement | null>();
  const transparencia = mime === "image/png" && transparente;

  for (let i = 0; i < n; i++) {
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d", { alpha: transparencia });
    if (!ctx) continue;
    ctx.imageSmoothingEnabled = true;
    ctx.imageSmoothingQuality = "high";
    if (transparencia) {
      ctx.clearRect(0, 0, w, h);
    } else {
      ctx.fillStyle = hoja || "#f4f1ea";
      ctx.fillRect(0, 0, w, h);
    }
    ctx.save();
    ctx.scale(escala, escala);
    ctx.translate(-i * PAGINA_W, 0);

    // El canvas solo camina la lista ya ordenada; toda la aritmética de
    // maquetado vive en las funciones puras de arriba.
    for (const item of ordenDePintura(bloques, i)) {
      if (item.tipo === "ink") {
        dibujarTinta(ctx, bloques);
        continue;
      }
      const m = item.marco;
      if (item.tipo === "image") {
        let img = cache.get(item.bloque.asset);
        if (img === undefined) {
          img = await cargarFoto(() => dataDe(item.bloque.asset));
          cache.set(item.bloque.asset, img);
        }
        if (img) ctx.drawImage(img, m.x, m.y, m.w, m.h);
        continue;
      }
      if (!transparencia) {
        ctx.fillStyle = CAJA_TEXTO;
        ctx.fillRect(m.x, m.y, m.w, m.h);
      }
      ctx.fillStyle = TINTA_TEXTO;
      ctx.font = FUENTE_CSS;
      ctx.textBaseline = "top";
      if (item.tipo === "text") {
        const medir = (s: string) => ctx.measureText(s).width;
        for (const linea of distribuirTexto(item.bloque.body, m, medir)) {
          ctx.fillText(linea.texto, linea.x, linea.y);
        }
      } else {
        for (const fila of distribuirChecks(item.bloque.items, m)) {
          pintarCheck(ctx, fila);
        }
      }
    }

    ctx.restore();
    const data = canvas.toDataURL(mime, 0.92);
    const base64 = data.split(",")[1] ?? "";
    salida.push({ base64, width: w, height: h });
  }
  return salida;
}

export async function armarExport(
  bloques: NoteBlock[],
  papelW: number,
  hoja: string,
  dataDe: (asset: string) => Promise<string>,
  formato: FormatoTablero,
  rasterizar: typeof rasterPaginas = rasterPaginas,
): Promise<ResultadoExport> {
  const n = cantidadPaginas(papelW);
  // Formatos de contenido nativo: PDF, Word y PowerPoint llevan texto e imágenes
  // reales, no un pantallazo. PNG/JPEG son imágenes por definición → rasterizado.
  const nativo = formato === "pdf" || formato === "docx" || formato === "pptx";
  const transparente = formato === "png" || nativo;
  const mime = formato === "jpeg" ? "image/jpeg" : "image/png";
  const previews = nativo
    ? []
    : await rasterizar(bloques, papelW, hoja, dataDe, mime, transparente);
  // Guarda el resultado por asset (o null si falló) para no releer ni reportar
  // dos veces el mismo archivo cuando aparece en varias celdas.
  const cacheData = new Map<string, { mime: string; imageBase64: string } | null>();
  const fallos: string[] = [];

  const paginas: ExportPagina[] = [];
  for (let i = 0; i < n; i++) {
    // Usa la misma función pura que testeamos en vez de rehacer el recorte acá.
    const { textos, checks, fotos: fotosAsset } = estructuraPagina(bloques, i);
    const fotos: ExportFoto[] = [];
    for (const foto of fotosAsset) {
      let data = cacheData.get(foto.asset);
      if (data === undefined) {
        try {
          data = partirDataUrl(await dataDe(foto.asset));
        } catch {
          // Antes se tragaba el error con `imageBase64: ""`. Ahora se junta el
          // asset en `fallos` para que la UI avise "N imágenes no se leyeron".
          data = null;
          fallos.push(foto.asset);
        }
        cacheData.set(foto.asset, data);
      }
      if (data?.imageBase64) {
        fotos.push({ ...data, x: foto.x, y: foto.y, w: foto.w, h: foto.h });
      }
    }
    paginas.push({
      previewBase64: previews[i]?.base64 ?? "",
      previewMime: mime,
      fotos,
      textos,
      checks,
      // La tinta no puede volverse texto nativo: se rasteriza siempre.
      inkBase64: nativo ? rasterTinta(bloques, i) : "",
    });
  }
  return { paginas, fallos };
}
