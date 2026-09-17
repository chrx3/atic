/**
 * El arte de las capturas y de la pizarra.
 *
 * No hay pantalla real que recortar en el navegador, así que las capturas se
 * dibujan: un escritorio con ventanas, cada una con contenido distinto —
 * código, gráfico, artículo—. Es lo que hace que el shelf y la pizarra se
 * puedan "usar" sin mentir sobre de dónde salió la imagen.
 */

export type CaptureArt = {
  /** PNG como data URL. */
  url: string;
  width: number;
  height: number;
};

/** Ruido determinista: la misma captura sale igual siempre. */
function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function drawWallpaper(ctx: CanvasRenderingContext2D, w: number, h: number) {
  const grad = ctx.createLinearGradient(0, 0, w, h);
  grad.addColorStop(0, "#14161f");
  grad.addColorStop(0.6, "#0d1016");
  grad.addColorStop(1, "#090b10");
  ctx.fillStyle = grad;
  ctx.fillRect(0, 0, w, h);

  const glowA = ctx.createRadialGradient(w * 0.18, h * 0.12, 0, w * 0.18, h * 0.12, w * 0.62);
  glowA.addColorStop(0, "rgba(47, 58, 99, 0.9)");
  glowA.addColorStop(1, "rgba(47, 58, 99, 0)");
  ctx.fillStyle = glowA;
  ctx.fillRect(0, 0, w, h);

  const glowB = ctx.createRadialGradient(w * 0.84, h * 0.8, 0, w * 0.84, h * 0.8, w * 0.7);
  glowB.addColorStop(0, "rgba(18, 63, 70, 0.85)");
  glowB.addColorStop(1, "rgba(18, 63, 70, 0)");
  ctx.fillStyle = glowB;
  ctx.fillRect(0, 0, w, h);

  ctx.strokeStyle = "rgba(255, 255, 255, 0.032)";
  ctx.lineWidth = 1;
  for (let x = 0.5; x < w; x += 48) {
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }
  for (let y = 0.5; y < h; y += 48) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

export function drawWindow(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  title: string,
) {
  ctx.save();
  ctx.shadowColor = "rgba(0, 0, 0, 0.5)";
  ctx.shadowBlur = 24;
  ctx.shadowOffsetY = 10;
  ctx.fillStyle = "#1a1a18";
  roundRect(ctx, x, y, w, h, 12);
  ctx.fill();
  ctx.restore();

  ctx.fillStyle = "#22221e";
  roundRect(ctx, x, y, w, 30, 12);
  ctx.fill();
  ctx.fillRect(x, y + 18, w, 12);

  const dots = ["#ff5f57", "#febc2e", "#28c840"];
  dots.forEach((color, i) => {
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(x + 14 + i * 14, y + 15, 4, 0, Math.PI * 2);
    ctx.fill();
  });

  ctx.fillStyle = "#74746c";
  ctx.font = "500 11px -apple-system, system-ui, sans-serif";
  ctx.fillText(title, x + 62, y + 19);
  return { x, y: y + 30, w, h: h - 30 };
}

function drawCodeWindow(
  ctx: CanvasRenderingContext2D,
  box: { x: number; y: number; w: number; h: number },
  rand: () => number,
) {
  const lineH = 13;
  const colors = ["#6faf88", "#8fa9b8", "#d4a84b", "#a8a89e", "#e85a52"];
  let yy = box.y + 18;
  for (let row = 0; row < Math.floor((box.h - 24) / lineH); row++) {
    const indent = Math.floor(rand() * 3) * 14;
    let xx = box.x + 18 + indent;
    ctx.fillStyle = "rgba(240, 240, 234, 0.09)";
    ctx.fillRect(box.x, yy - 8, box.w, lineH - 3);
    const chunks = 2 + Math.floor(rand() * 3);
    for (let c = 0; c < chunks; c++) {
      const len = 18 + rand() * 46;
      ctx.fillStyle = colors[Math.floor(rand() * colors.length)];
      ctx.globalAlpha = 0.75;
      ctx.fillRect(xx, yy - 5, len, 7);
      ctx.globalAlpha = 1;
      xx += len + 12;
      if (xx > box.x + box.w - 40) break;
    }
    yy += lineH;
  }
}

function drawChartWindow(
  ctx: CanvasRenderingContext2D,
  box: { x: number; y: number; w: number; h: number },
  rand: () => number,
) {
  const baseY = box.y + box.h - 24;
  const bars = 9;
  const gap = 10;
  const bw = (box.w - 40 - gap * (bars - 1)) / bars;
  for (let i = 0; i < bars; i++) {
    const bh = 20 + rand() * (box.h - 90);
    const x = box.x + 20 + i * (bw + gap);
    const grad = ctx.createLinearGradient(0, baseY - bh, 0, baseY);
    grad.addColorStop(0, "#8fa9b8");
    grad.addColorStop(1, "rgba(143, 169, 184, 0.25)");
    ctx.fillStyle = grad;
    ctx.fillRect(x, baseY - bh, bw, bh);
  }
  ctx.strokeStyle = "rgba(240, 240, 234, 0.16)";
  ctx.beginPath();
  ctx.moveTo(box.x + 10, baseY + 0.5);
  ctx.lineTo(box.x + box.w - 10, baseY + 0.5);
  ctx.stroke();
}

function drawArticleWindow(
  ctx: CanvasRenderingContext2D,
  box: { x: number; y: number; w: number; h: number },
  rand: () => number,
) {
  ctx.fillStyle = "#f0f0ea";
  ctx.globalAlpha = 0.85;
  ctx.fillRect(box.x + 22, box.y + 22, box.w * 0.5, 12);
  ctx.globalAlpha = 1;
  let yy = box.y + 48;
  while (yy < box.y + box.h - 20) {
    ctx.fillStyle = "rgba(240, 240, 234, 0.12)";
    const len = box.w - 44 - rand() * 120;
    ctx.fillRect(box.x + 22, yy, len, 8);
    yy += 16;
  }
}

export type CaptureVariant = "code" | "chart" | "article";

export const CAPTURE_VARIANTS: CaptureVariant[] = ["code", "chart", "article"];

/** Una captura de escritorio con una ventana de contenido, como PNG. */
export function makeCapture(
  variant: CaptureVariant,
  width = 960,
  height = 600,
  seed = 7,
): CaptureArt {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) return { url: "", width, height };
  const rand = mulberry32(seed);

  drawWallpaper(ctx, width, height);

  const w = width * 0.72;
  const h = height * 0.66;
  const x = (width - w) / 2;
  const y = (height - h) / 2;
  const title =
    variant === "code" ? "editor — atic" : variant === "chart" ? "ventas-q3.xlsx" : "Notas de la reunión";
  const box = drawWindow(ctx, x, y, w, h, title);

  if (variant === "code") drawCodeWindow(ctx, box, rand);
  else if (variant === "chart") drawChartWindow(ctx, box, rand);
  else drawArticleWindow(ctx, box, rand);

  return { url: canvas.toDataURL("image/png"), width, height };
}

export function canvasToBlob(canvas: HTMLCanvasElement): Promise<Blob | null> {
  return new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
}
