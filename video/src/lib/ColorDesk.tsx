import { FONT_SANS } from "./theme";

/**
 * Escritorio de la escena Color: una app de paletas hecha solo con formas y colores planos.
 * La misma lista de primitivas se pinta (SVG) y se «lee» (colorAt), así el parche 13x13 de la
 * lupa coincide con lo que se ve en pantalla sin capturar nada.
 */

type Grad = { x1: number; y1: number; x2: number; y2: number; stops: [number, string][] };
type Fill = string | Grad;
type Prim =
  | { k: "rect"; x: number; y: number; w: number; h: number; r?: number; fill: Fill }
  | { k: "circle"; x: number; y: number; r: number; fill: Fill };

const hexToRgb = (hex: string): [number, number, number] => [
  parseInt(hex.slice(1, 3), 16),
  parseInt(hex.slice(3, 5), 16),
  parseInt(hex.slice(5, 7), 16),
];
export const rgbToHex = (rgb: [number, number, number]) =>
  "#" +
  rgb
    .map((v) =>
      Math.max(0, Math.min(255, Math.round(v)))
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")
    .toUpperCase();

/** Mezcla `a` hacia `b` en proporción `t`. */
const mix = (a: string, b: string, t: number) => {
  const pa = hexToRgb(a);
  const pb = hexToRgb(b);
  return rgbToHex([0, 1, 2].map((i) => pa[i] + (pb[i] - pa[i]) * t) as [number, number, number]);
};

export const CARD_COLORS = ["#3A82F6", "#F2705C", "#F5B83D", "#34C38F", "#8B5CF6", "#EC4899", "#14B8C4", "#1F2937"];
export const cardY = (i: number) => 96 + 64 * i;
const CARD = { x: 26, w: 180, h: 54 };

const PRIMS: Prim[] = (() => {
  const p: Prim[] = [];
  p.push({
    k: "rect", x: 0, y: 0, w: 500, h: 660,
    fill: { x1: 0, y1: 0, x2: 0, y2: 1, stops: [[0, "#DFE5EC"], [1, "#C4CFDB"]] },
  });
  // ventana
  p.push({ k: "rect", x: 10, y: 56, w: 480, h: 568, r: 12, fill: "#F7F6F2" });
  p.push({ k: "rect", x: 10, y: 56, w: 480, h: 32, r: 12, fill: "#ECEBE5" });
  p.push({ k: "rect", x: 10, y: 76, w: 480, h: 12, fill: "#ECEBE5" });
  p.push({ k: "rect", x: 10, y: 88, w: 480, h: 1, fill: "#DCDAD3" });
  p.push({ k: "circle", x: 28, y: 72, r: 5, fill: "#FF5F57" });
  p.push({ k: "circle", x: 46, y: 72, r: 5, fill: "#FEBC2E" });
  p.push({ k: "circle", x: 64, y: 72, r: 5, fill: "#28C840" });
  // columna de muestras de color
  CARD_COLORS.forEach((c, i) => {
    const y = cardY(i);
    p.push({ k: "rect", x: CARD.x, y, w: CARD.w, h: CARD.h, r: 12, fill: c });
    p.push({ k: "rect", x: 38, y: y + 14, w: 72, h: 7, r: 3.5, fill: mix(c, "#FFFFFF", 0.78) });
    p.push({ k: "rect", x: 38, y: y + 28, w: 48, h: 6, r: 3, fill: mix(c, "#FFFFFF", 0.55) });
    p.push({ k: "circle", x: 176, y: y + 27, r: 10, fill: mix(c, "#FFFFFF", 0.35) });
  });
  // lienzo de la derecha
  p.push({
    k: "rect", x: 222, y: 96, w: 252, h: 150, r: 12,
    fill: { x1: 0, y1: 0, x2: 1, y2: 1, stops: [[0, "#3A82F6"], [0.55, "#8B5CF6"], [1, "#EC4899"]] },
  });
  p.push({ k: "circle", x: 438, y: 132, r: 22, fill: "#FDE68A" });
  p.push({ k: "rect", x: 234, y: 204, w: 228, h: 30, r: 10, fill: "#34C38F" });
  ["#F97316", "#22C55E", "#06B6D4", "#A855F7", "#E11D48"].forEach((c, i) =>
    p.push({ k: "circle", x: 244 + 48 * i, y: 272, r: 20, fill: c }),
  );
  p.push({ k: "rect", x: 222, y: 316, w: 252, h: 196, r: 12, fill: "#FFFFFF" });
  [[120, "#D9DEE5"], [200, "#E6E9EE"], [170, "#E6E9EE"], [210, "#E6E9EE"], [140, "#E6E9EE"]].forEach(([w, c], i) =>
    p.push({ k: "rect", x: 238, y: 334 + 20 * i, w: w as number, h: i === 0 ? 8 : 7, r: 3.5, fill: c as string }),
  );
  p.push({ k: "rect", x: 238, y: 456, w: 112, h: 34, r: 10, fill: "#3A82F6" });
  p.push({ k: "rect", x: 258, y: 470, w: 72, h: 6, r: 3, fill: "#FFFFFF" });
  p.push({ k: "rect", x: 362, y: 456, w: 96, h: 34, r: 10, fill: "#EEF1F5" });
  p.push({ k: "rect", x: 378, y: 470, w: 64, h: 6, r: 3, fill: "#94A3B8" });
  p.push({ k: "rect", x: 222, y: 528, w: 252, h: 84, r: 12, fill: "#1F2937" });
  p.push({ k: "rect", x: 238, y: 546, w: 60, h: 8, r: 4, fill: "#F5B83D" });
  p.push({ k: "rect", x: 238, y: 564, w: 140, h: 7, r: 3.5, fill: "#374151" });
  p.push({ k: "rect", x: 238, y: 580, w: 100, h: 7, r: 3.5, fill: "#374151" });
  return p;
})();

const lerpHex = (stops: [number, string][], t: number) => {
  const tt = Math.max(0, Math.min(1, t));
  for (let i = 1; i < stops.length; i++) {
    if (tt <= stops[i][0]) {
      const [t0, c0] = stops[i - 1];
      const [t1, c1] = stops[i];
      return mix(c0, c1, (tt - t0) / (t1 - t0));
    }
  }
  return stops[stops.length - 1][1];
};

const fillAt = (fill: Fill, u: number, v: number) => {
  if (typeof fill === "string") return fill;
  const dx = fill.x2 - fill.x1;
  const dy = fill.y2 - fill.y1;
  const t = ((u - fill.x1) * dx + (v - fill.y1) * dy) / (dx * dx + dy * dy);
  return lerpHex(fill.stops, t);
};

const inRound = (px: number, py: number, x: number, y: number, w: number, h: number, r: number) => {
  if (px < x || px > x + w || py < y || py > y + h) return false;
  const rr = Math.min(r, w / 2, h / 2);
  const cx = px < x + rr ? x + rr : px > x + w - rr ? x + w - rr : px;
  const cy = py < y + rr ? y + rr : py > y + h - rr ? y + h - rr : py;
  return (px - cx) ** 2 + (py - cy) ** 2 <= rr * rr;
};

/** Color (#RRGGBB) del píxel de pantalla (px, py): centro del píxel, sin suavizado. */
export const colorAt = (px: number, py: number): string => {
  const x = px + 0.5;
  const y = py + 0.5;
  for (let i = PRIMS.length - 1; i >= 0; i--) {
    const q = PRIMS[i];
    if (q.k === "rect") {
      if (inRound(x, y, q.x, q.y, q.w, q.h, q.r ?? 0)) return fillAt(q.fill, (x - q.x) / q.w, (y - q.y) / q.h);
    } else if ((x - q.x) ** 2 + (y - q.y) ** 2 <= q.r * q.r) {
      return fillAt(q.fill, (x - (q.x - q.r)) / (2 * q.r), (y - (q.y - q.r)) / (2 * q.r));
    }
  }
  return "#000000";
};

/** Parche de 13x13 píxeles centrado en (cx, cy), fila a fila. */
export const patchAt = (cx: number, cy: number, grid = 13): string[] => {
  const half = (grid - 1) / 2;
  const out: string[] = [];
  for (let j = -half; j <= half; j++) for (let i = -half; i <= half; i++) out.push(colorAt(cx + i, cy + j));
  return out;
};

export const ColorDesk = () => (
  <svg
    width={500}
    height={660}
    style={{ position: "absolute", left: 0, top: 0, fontFamily: FONT_SANS }}
  >
    <defs>
      {PRIMS.map((q, i) =>
        typeof q.fill === "string" ? null : (
          <linearGradient key={i} id={`cg${i}`} x1={q.fill.x1} y1={q.fill.y1} x2={q.fill.x2} y2={q.fill.y2}>
            {q.fill.stops.map(([o, c]) => (
              <stop key={o} offset={o} stopColor={c} />
            ))}
          </linearGradient>
        ),
      )}
    </defs>
    {PRIMS.map((q, i) => {
      const fill = typeof q.fill === "string" ? q.fill : `url(#cg${i})`;
      return q.k === "rect" ? (
        <rect key={i} x={q.x} y={q.y} width={q.w} height={q.h} rx={q.r ?? 0} fill={fill} />
      ) : (
        <circle key={i} cx={q.x} cy={q.y} r={q.r} fill={fill} />
      );
    })}
  </svg>
);
