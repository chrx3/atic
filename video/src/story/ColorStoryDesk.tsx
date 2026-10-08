import type { CSSProperties, ReactNode } from "react";
import { D, Desktop, SWATCHES } from "../desk/desk";
import { FONT_MONO, FONT_SANS } from "../lib/theme";

/**
 * Programa de diseño de la historia (versión grande de `DesignApp`): una tarjeta de tienda y la
 * paleta de la marca. Como en `lib/ColorDesk`, la misma lista de formas se pinta (SVG) y se «lee»
 * (`colorAt`), así el parche 13x13 de la lupa coincide con lo que se ve sin capturar nada.
 * Los textos van encima y no cuentan: el cursor solo se detiene sobre zonas de color plano.
 */

type Prim =
  | { k: "rect"; x: number; y: number; w: number; h: number; r?: number; fill: string; shadow?: boolean }
  | { k: "circle"; x: number; y: number; r: number; fill: string };

const [PRIMARIO, ACENTO, EXITO, AVISO, TINTA] = SWATCHES.map((s) => s.hex);

/** Ventana del programa (coordenadas de pantalla, 500x660). */
export const WIN = { x: 10, y: 60, w: 480, h: 540, bar: 24 } as const;

const CANVAS = "#E9EBF0";
const LINE = "#E8EAEC";
const PAPER = "#FFFFFF";

/** Cada muestra de la paleta: bloque de color a la izquierda, nombre y hex a su derecha. */
export const SWATCH_X = 46;
export const swatchRect = (i: number) => ({ x: SWATCH_X, y: 118 + 74 * i, w: 64, h: 60 });
/** Un punto dentro del bloque de cada muestra (nombre → punto), para detener el cursor. */
export const swatchPoint = (name: string, dx = 0, dy = 0): [number, number] => {
  const i = SWATCHES.findIndex((s) => s.name === name);
  const r = swatchRect(i);
  return [r.x + r.w / 2 + dx, r.y + r.h / 2 + dy];
};

const CARD_A = { x: 218, y: 98, w: 262, h: 184 };
const CARD_B = { x: 218, y: 298, w: 262, h: 280 };

const PRIMS: Prim[] = (() => {
  const p: Prim[] = [];
  // fondo del escritorio bajo la ventana (para el borde del parche) y ventana
  p.push({ k: "rect", x: 0, y: 0, w: 500, h: 660, fill: "#5B86EC" });
  p.push({ k: "rect", x: WIN.x, y: WIN.y, w: WIN.w, h: WIN.bar, fill: "#EEF1F6" });
  p.push({ k: "rect", x: WIN.x, y: WIN.y + WIN.bar, w: WIN.w, h: WIN.h - WIN.bar, fill: CANVAS });
  // barra de herramientas de la izquierda
  p.push({ k: "rect", x: WIN.x, y: WIN.y + WIN.bar, w: 26, h: WIN.h - WIN.bar, fill: PAPER });
  p.push({ k: "rect", x: WIN.x + 25, y: WIN.y + WIN.bar, w: 1, h: WIN.h - WIN.bar, fill: LINE });
  // paleta
  SWATCHES.forEach((s, i) => {
    const r = swatchRect(i);
    p.push({ k: "rect", ...r, r: 10, fill: s.hex });
  });
  // tarjeta de la tienda
  p.push({ k: "rect", ...CARD_A, r: 12, fill: PAPER, shadow: true });
  p.push({ k: "rect", x: 234, y: 196, w: 120, h: 34, r: 17, fill: PRIMARIO });
  p.push({ k: "rect", x: 364, y: 200, w: 100, h: 26, r: 13, fill: EXITO });
  p.push({ k: "rect", x: 234, y: 242, w: 60, h: 24, r: 12, fill: AVISO });
  p.push({ k: "rect", x: 304, y: 242, w: 96, h: 24, r: 12, fill: ACENTO });
  // seguimiento del envío
  p.push({ k: "rect", ...CARD_B, r: 12, fill: PAPER, shadow: true });
  p.push({ k: "rect", x: 234, y: 358, w: 230, h: 10, r: 5, fill: "#E6E9EE" });
  p.push({ k: "rect", x: 234, y: 358, w: 150, h: 10, r: 5, fill: EXITO });
  p.push({ k: "circle", x: 241, y: 398, r: 6, fill: EXITO });
  p.push({ k: "circle", x: 241, y: 428, r: 6, fill: AVISO });
  p.push({ k: "circle", x: 241, y: 458, r: 6, fill: "#D9DEE5" });
  p.push({ k: "rect", x: 234, y: 496, w: 230, h: 62, r: 10, fill: TINTA });
  return p;
})();

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
    if (q.k === "rect" ? inRound(x, y, q.x, q.y, q.w, q.h, q.r ?? 0) : (x - q.x) ** 2 + (y - q.y) ** 2 <= q.r * q.r) {
      return q.fill;
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

const Txt = ({ x, y, w, children, style }: { x: number; y: number; w?: number; children: ReactNode; style?: CSSProperties }) => (
  <div style={{ position: "absolute", left: x, top: y, width: w, whiteSpace: w ? "normal" : "nowrap", ...style }}>{children}</div>
);

/** Texto centrado dentro de una forma de la lista. */
const InRect = ({ x, y, w, h, children, style }: { x: number; y: number; w: number; h: number; children: ReactNode; style?: CSSProperties }) => (
  <div style={{ position: "absolute", left: x, top: y, width: w, height: h, display: "flex", alignItems: "center", justifyContent: "center", ...style }}>
    {children}
  </div>
);

export const ColorStoryDesk = () => (
  <Desktop time="14:35">
    <div
      style={{
        position: "absolute",
        left: WIN.x,
        top: WIN.y,
        width: WIN.w,
        height: WIN.h,
        borderRadius: 8,
        overflow: "hidden",
        boxShadow: "0 18px 44px rgb(15 25 55 / 34%), 0 0 0 1px rgb(15 25 55 / 18%)",
      }}
    >
      {/* capa en coordenadas de pantalla: se desplaza para que (0,0) sea la esquina del monitor */}
      <div style={{ position: "absolute", left: -WIN.x, top: -WIN.y, width: 500, height: 660, fontFamily: FONT_SANS }}>
        <svg width={500} height={660} style={{ position: "absolute", left: 0, top: 0 }}>
          <defs>
            <filter id="story-card-shadow" x="-10%" y="-10%" width="120%" height="130%">
              <feDropShadow dx={0} dy={6} stdDeviation={9} floodColor="#141e3c" floodOpacity={0.12} />
            </filter>
          </defs>
          {PRIMS.map((q, i) =>
            q.k === "rect" ? (
              <rect key={i} x={q.x} y={q.y} width={q.w} height={q.h} rx={q.r ?? 0} fill={q.fill} filter={q.shadow ? "url(#story-card-shadow)" : undefined} />
            ) : (
              <circle key={i} cx={q.x} cy={q.y} r={q.r} fill={q.fill} />
            ),
          )}
          {/* contorno de las muestras (solo se ve, no cuenta al leer) */}
          {SWATCHES.map((s, i) => {
            const r = swatchRect(i);
            return <rect key={s.hex} x={r.x + 0.5} y={r.y + 0.5} width={r.w - 1} height={r.h - 1} rx={9.5} fill="none" stroke="rgb(0 0 0 / 8%)" />;
          })}
        </svg>

        {/* barra de título */}
        <Txt x={WIN.x + 10} y={WIN.y + 6} style={{ fontSize: 9.5, fontWeight: 500, color: D.inkSoft }}>
          Marca — Diseño
        </Txt>
        <Txt x={WIN.x + WIN.w - 66} y={WIN.y + 6} style={{ fontSize: 9, color: D.inkSoft, letterSpacing: "0.9em" }}>
          —▢✕
        </Txt>

        {/* herramientas */}
        {[0, 1, 2, 3].map((i) => (
          <span
            key={i}
            style={{
              position: "absolute",
              left: WIN.x + 7.5,
              top: WIN.y + WIN.bar + 12 + i * 22,
              width: 11,
              height: 11,
              borderRadius: i === 1 ? 6 : 2,
              border: `1.5px solid ${i === 0 ? D.blue : D.inkSoft}`,
            }}
          />
        ))}

        {/* paleta */}
        <Txt x={SWATCH_X} y={98} style={{ fontSize: 9, letterSpacing: "0.08em", color: D.inkSoft }}>
          PALETA
        </Txt>
        {SWATCHES.map((s, i) => {
          const r = swatchRect(i);
          return (
            <div key={s.hex}>
              <Txt x={r.x + r.w + 12} y={r.y + 15} style={{ fontSize: 11.5, fontWeight: 650, color: D.ink }}>
                {s.name}
              </Txt>
              <Txt x={r.x + r.w + 12} y={r.y + 33} style={{ fontSize: 10, color: D.inkSoft, fontFamily: FONT_MONO }}>
                {s.hex}
              </Txt>
            </div>
          );
        })}

        {/* tarjeta de la tienda */}
        <Txt x={234} y={112} style={{ fontSize: 8.5, letterSpacing: "0.08em", color: D.inkSoft }}>
          TIENDA
        </Txt>
        <Txt x={234} y={125} style={{ fontSize: 19, fontWeight: 700, lineHeight: 1.15, color: TINTA }}>
          Tu pedido, más rápido
        </Txt>
        <Txt x={234} y={153} w={226} style={{ fontSize: 10, lineHeight: 1.4, color: D.inkSoft }}>
          Compra en segundos y sigue el envío en tiempo real.
        </Txt>
        <InRect x={234} y={196} w={120} h={34} style={{ color: "#fff", fontSize: 11, fontWeight: 650 }}>
          Comprar ahora
        </InRect>
        <InRect x={364} y={200} w={100} h={26} style={{ color: "#fff", fontSize: 10, fontWeight: 650 }}>
          Envío gratis
        </InRect>
        <InRect x={234} y={242} w={60} h={24} style={{ color: TINTA, fontSize: 10, fontWeight: 700 }}>
          -20 %
        </InRect>
        <InRect x={304} y={242} w={96} h={24} style={{ color: "#fff", fontSize: 10, fontWeight: 650 }}>
          Ver detalles
        </InRect>

        {/* seguimiento del envío */}
        <Txt x={234} y={312} style={{ fontSize: 13, fontWeight: 700, color: TINTA }}>
          Seguimiento del envío
        </Txt>
        <Txt x={234} y={331} style={{ fontSize: 9.5, color: D.inkSoft }}>
          Pedido #1042 · Almacén Central
        </Txt>
        {[
          ["Preparado", "13:58"],
          ["En camino", "14:20"],
          ["Entregado", "—"],
        ].map(([name, time], i) => (
          <div key={name}>
            <Txt x={256} y={391 + 30 * i} style={{ fontSize: 10.5, fontWeight: 600, color: i === 2 ? D.inkSoft : D.ink }}>
              {name}
            </Txt>
            <Txt x={400} y={391 + 30 * i} style={{ width: 64, textAlign: "right", fontSize: 10, color: D.inkSoft }}>
              {time}
            </Txt>
          </div>
        ))}
        <Txt x={234} y={472} style={{ fontSize: 9.5, color: D.inkSoft }}>
          Llega hoy antes de las 17:00
        </Txt>
        <Txt x={248} y={510} style={{ fontSize: 9.5, color: "#AEB7C4" }}>
          Total del pedido
        </Txt>
        <Txt x={248} y={525} style={{ fontSize: 17, fontWeight: 700, color: "#fff" }}>
          $ 126.500
        </Txt>
      </div>
    </div>
  </Desktop>
);
