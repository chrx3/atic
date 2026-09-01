/**
 * Cómo se pinta cada forma en un canvas 2D.
 *
 * Va aparte del componente porque es la mitad que se puede probar: recibe un
 * contexto y no sabe nada de Svelte, de la ventana ni del archivo. En los tests
 * se le pasa un contexto de mentira que anota las llamadas, que alcanza para
 * fijar lo que importa —que el resaltador sea translúcido, que la flecha cierre
 * su punta— sin rasterizar nada.
 *
 * El mismo código dibuja la vista y la exportación: si divergieran, lo que se
 * copia al portapapeles no sería lo que se vio.
 */

import {
  arrowHead,
  HIGHLIGHT_FACTOR,
  LINE_HEIGHT,
  normalizeRect,
  type Rect,
  type Shape,
  type SpanShape,
  type TextShape,
} from "./annotateModel";

/**
 * Lo que este módulo necesita de un canvas.
 *
 * Es un subconjunto de `CanvasRenderingContext2D` y no el tipo entero para que
 * un doble de pruebas sea cinco líneas en vez de cincuenta.
 */
export interface DrawTarget {
  save(): void;
  restore(): void;
  beginPath(): void;
  closePath(): void;
  moveTo(x: number, y: number): void;
  lineTo(x: number, y: number): void;
  quadraticCurveTo(cpx: number, cpy: number, x: number, y: number): void;
  ellipse(
    x: number,
    y: number,
    radiusX: number,
    radiusY: number,
    rotation: number,
    startAngle: number,
    endAngle: number,
  ): void;
  rect(x: number, y: number, w: number, h: number): void;
  stroke(): void;
  fill(): void;
  fillRect(x: number, y: number, w: number, h: number): void;
  fillText(text: string, x: number, y: number): void;
  strokeText(text: string, x: number, y: number): void;
  font: string;
  textBaseline: CanvasTextBaseline;
  /* Los mismos tipos que el DOM: un contexto real acepta también degradados. */
  strokeStyle: string | CanvasGradient | CanvasPattern;
  fillStyle: string | CanvasGradient | CanvasPattern;
  lineWidth: number;
  lineCap: CanvasLineCap;
  lineJoin: CanvasLineJoin;
  globalAlpha: number;
  globalCompositeOperation: GlobalCompositeOperation;
}

/** Opacidad del resaltador. Suficiente para marcar, no para tapar el texto. */
const HIGHLIGHT_ALPHA = 0.3;

export function drawShapes(ctx: DrawTarget, shapes: readonly Shape[]): void {
  for (const shape of shapes) drawShape(ctx, shape);
}

export function drawShape(ctx: DrawTarget, shape: Shape): void {
  ctx.save();
  ctx.strokeStyle = shape.color;
  ctx.fillStyle = shape.color;
  ctx.lineWidth = shape.width;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  switch (shape.kind) {
    case "highlight":
      // `multiply` deja ver lo que hay debajo en vez de lavarlo, que es la
      // diferencia entre un resaltador y una mancha de pintura.
      ctx.globalAlpha = HIGHLIGHT_ALPHA;
      ctx.globalCompositeOperation = "multiply";
      ctx.lineWidth = shape.width * HIGHLIGHT_FACTOR;
      strokePath(ctx, shape.points);
      break;
    case "pen":
      strokePath(ctx, shape.points);
      break;
    case "arrow":
      drawArrow(ctx, shape);
      break;
    case "ellipse": {
      const r = normalizeRect(shape.from, shape.to);
      ctx.beginPath();
      ctx.ellipse(r.x + r.w / 2, r.y + r.h / 2, r.w / 2, r.h / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
      break;
    }
    case "rect": {
      const r = normalizeRect(shape.from, shape.to);
      ctx.beginPath();
      ctx.rect(r.x, r.y, r.w, r.h);
      ctx.stroke();
      break;
    }
    case "text":
      drawText(ctx, shape);
      break;
  }

  ctx.restore();
}

/**
 * La familia del texto.
 *
 * La misma pila que usa la app, para que lo escrito en el cuadro de edición y
 * lo dibujado en el lienzo sean la misma letra: cualquier otra cosa haría que
 * el texto se corriera al confirmar.
 */
export const TEXT_FONT =
  '"Segoe UI", system-ui, -apple-system, "Helvetica Neue", Arial, sans-serif';

export function fontFor(size: number): string {
  return `600 ${size}px ${TEXT_FONT}`;
}

/**
 * Texto con contorno.
 *
 * El contorno no es adorno: la anotación cae sobre una captura cualquiera, y
 * un texto blanco sobre un fondo claro —o negro sobre uno oscuro— no se lee.
 * Un halo del color contrario lo despega de lo que haya debajo sin tener que
 * pintarle una caja atrás.
 */
function drawText(ctx: DrawTarget, shape: TextShape): void {
  const lines = shape.text.split("\n");
  const step = shape.size * LINE_HEIGHT;
  ctx.font = fontFor(shape.size);
  ctx.textBaseline = "top";
  ctx.lineJoin = "round";
  ctx.lineWidth = Math.max(2, shape.size / 7);
  ctx.strokeStyle = haloFor(shape.color);
  for (let i = 0; i < lines.length; i++) {
    const y = shape.at.y + i * step;
    if (lines[i].length === 0) continue;
    ctx.strokeText(lines[i], shape.at.x, y);
    ctx.fillText(lines[i], shape.at.x, y);
  }
}

/** Negro para los colores claros, blanco para los oscuros. */
export function haloFor(color: string): string {
  return luminance(color) > 0.55 ? "#00000099" : "#ffffffcc";
}

/** Luminancia aproximada de un `#rrggbb`. Lo que no se entiende, se asume claro. */
function luminance(color: string): number {
  const hex = color.trim().replace("#", "");
  if (hex.length !== 6) return 1;
  const r = Number.parseInt(hex.slice(0, 2), 16);
  const g = Number.parseInt(hex.slice(2, 4), 16);
  const b = Number.parseInt(hex.slice(4, 6), 16);
  if (!Number.isFinite(r + g + b)) return 1;
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

/**
 * El velo de lo que el recorte deja afuera.
 *
 * Se pinta al final y en coordenadas de imagen, como todo lo demás: lo que se
 * ve oscurecido es exactamente lo que se va a perder al soltar.
 */
export function drawCropMask(ctx: DrawTarget, crop: Rect, bounds: Rect): void {
  ctx.save();
  ctx.globalAlpha = 1;
  ctx.globalCompositeOperation = "source-over";
  ctx.fillStyle = "#00000080";
  const right = bounds.x + bounds.w;
  const bottom = bounds.y + bounds.h;
  ctx.fillRect(bounds.x, bounds.y, bounds.w, crop.y - bounds.y);
  ctx.fillRect(bounds.x, crop.y + crop.h, bounds.w, bottom - (crop.y + crop.h));
  ctx.fillRect(bounds.x, crop.y, crop.x - bounds.x, crop.h);
  ctx.fillRect(crop.x + crop.w, crop.y, right - (crop.x + crop.w), crop.h);

  ctx.strokeStyle = "#ffffff";
  ctx.lineWidth = Math.max(1, (bounds.w / 1280) * 2);
  ctx.beginPath();
  ctx.rect(crop.x, crop.y, crop.w, crop.h);
  ctx.stroke();
  ctx.restore();
}

/**
 * Trazo libre suavizado.
 *
 * Une los puntos por el punto medio con una curva cuadrática en vez de con
 * segmentos rectos: a mano alzada, los segmentos se ven como una cadena de
 * palitos en cuanto el trazo es grueso.
 */
function strokePath(
  ctx: DrawTarget,
  points: readonly { x: number; y: number }[],
): void {
  if (points.length === 0) return;
  ctx.beginPath();
  if (points.length === 1) {
    // Un punto solo: un toque. Se dibuja como una línea de largo cero, que con
    // `lineCap: round` es exactamente un círculo del grosor del trazo.
    ctx.moveTo(points[0].x, points[0].y);
    ctx.lineTo(points[0].x, points[0].y);
    ctx.stroke();
    return;
  }
  ctx.moveTo(points[0].x, points[0].y);
  for (let i = 1; i < points.length - 1; i++) {
    const mid = {
      x: (points[i].x + points[i + 1].x) / 2,
      y: (points[i].y + points[i + 1].y) / 2,
    };
    ctx.quadraticCurveTo(points[i].x, points[i].y, mid.x, mid.y);
  }
  const last = points[points.length - 1];
  ctx.lineTo(last.x, last.y);
  ctx.stroke();
}

function drawArrow(ctx: DrawTarget, shape: SpanShape): void {
  const head = arrowHead(shape.from, shape.to, shape.width);
  // La línea termina en el cuello y no en la punta: si llegara hasta el final,
  // el `lineCap` redondo asomaría por delante del triángulo.
  ctx.beginPath();
  ctx.moveTo(shape.from.x, shape.from.y);
  ctx.lineTo(head.neck.x, head.neck.y);
  ctx.stroke();

  ctx.beginPath();
  ctx.moveTo(head.tip.x, head.tip.y);
  ctx.lineTo(head.left.x, head.left.y);
  ctx.lineTo(head.right.x, head.right.y);
  ctx.closePath();
  ctx.fill();
}
