import type { CSSProperties, ReactNode } from "react";
import { FONT_SANS } from "./theme";

/**
 * Escritorio de la pizarra: la pantalla lógica de 500x660 vista con zoom 0.88
 * (así cabe la barra de ~504 px). Coordenadas propias de este escritorio.
 */
export const DESK = { w: 568, h: 750, cx: 284, zoom: 500 / 568 } as const;

/** Geometría de lo que se marca (referencia para las formas de la escena). */
export const DESK_GEO = {
  window: { x: 24, y: 80, w: 520, h: 600 },
  kpi2: { x: 207, y: 130, w: 154, h: 76 },
  bars: { left: 58, step: 68, width: 44, base: 440 },
  taskbarTop: 702,
} as const;

const INK = "#1f2a37";
const SOFT = "#6b7683";
const CHART_HEIGHTS = [112, 128, 100, 34, 138, 120, 92];
const DAYS = ["L", "M", "M", "J", "V", "S", "D"];

const abs = (x: number, y: number, w: number, h: number): CSSProperties => ({
  position: "absolute",
  left: x,
  top: y,
  width: w,
  height: h,
});

const thousands = (n: number) => Math.round(n).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ".");

const Card = ({ x, y, w, h, children }: { x: number; y: number; w: number; h: number; children?: ReactNode }) => (
  <div
    style={{
      ...abs(x, y, w, h),
      borderRadius: 10,
      background: "#fff",
      boxShadow: "0 0 0 1px rgb(20 24 30 / 7%)",
    }}
  >
    {children}
  </div>
);

const Kpi = ({
  x,
  label,
  value,
  delta,
  deltaColor,
}: {
  x: number;
  label: string;
  value: string;
  delta: string;
  deltaColor: string;
}) => (
  <Card x={x} y={130} w={154} h={76}>
    <div style={{ position: "absolute", left: 14, top: 10, fontSize: 11, color: SOFT }}>{label}</div>
    <div
      style={{
        position: "absolute",
        left: 14,
        top: 25,
        fontSize: 23,
        fontWeight: 650,
        letterSpacing: "-0.02em",
        color: INK,
        fontVariantNumeric: "tabular-nums",
      }}
    >
      {value}
    </div>
    <div style={{ position: "absolute", left: 14, top: 56, fontSize: 11, fontWeight: 500, color: deltaColor }}>
      {delta}
    </div>
  </Card>
);

const TASKBAR_ICONS = ["#5b8def", "#f2a33a", "#43b581", "#c069d6", "#e5606a", "#4bb6c9", "#8896a7"];

/**
 * `liveMs` marca el reloj de lo que se mueve en pantalla (contadores y punto «en vivo»):
 * al congelarse, la escena deja de avanzarlo.
 */
export const PizarraDesk = ({ liveMs }: { liveMs: number }) => {
  const visits = 12480 + Math.floor(liveMs / 60) * 3;
  const revenue = 892340 + Math.floor(liveMs / 80) * 130;
  const dot = 0.45 + 0.55 * Math.abs(Math.sin(liveMs / 320));

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: DESK.w,
        height: DESK.h,
        overflow: "hidden",
        fontFamily: FONT_SANS,
        background: "radial-gradient(120% 90% at 20% 0%, #e9e3d6 0%, #cfd3d1 45%, #aeb9c2 100%)",
      }}
    >
      {/* ventana: panel genérico de métricas */}
      <div
        style={{
          ...abs(24, 80, 520, 600),
          borderRadius: 12,
          background: "#f4f5f7",
          boxShadow: "0 24px 60px rgb(20 24 30 / 22%)",
          overflow: "hidden",
        }}
      >
        <div style={{ ...abs(0, 0, 520, 34), background: "#e9ecf0", borderBottom: "1px solid rgb(20 24 30 / 8%)" }}>
          {["#ff5f57", "#febc2e", "#28c840"].map((c, i) => (
            <span key={c} style={{ ...abs(14 + i * 16, 12, 10, 10), borderRadius: 10, background: c }} />
          ))}
          <div style={{ ...abs(0, 0, 520, 34), lineHeight: "34px", textAlign: "center", fontSize: 12, color: SOFT }}>
            Panel semanal
          </div>
          <div
            style={{
              ...abs(408, 8, 98, 18),
              borderRadius: 9,
              background: "#dff3e6",
              color: "#2f7d4f",
              fontSize: 10.5,
              fontWeight: 600,
              display: "flex",
              alignItems: "center",
              gap: 5,
              paddingLeft: 8,
              boxSizing: "border-box",
            }}
          >
            <span style={{ width: 6, height: 6, borderRadius: 6, background: "#2f9e5b", opacity: dot }} />
            En vivo
          </div>
        </div>
      </div>

      {/* tarjetas de métricas (coordenadas del escritorio) */}
      <Kpi x={42} label="Visitas" value={thousands(visits)} delta="▲ 4 % vs. ayer" deltaColor="#2f7d4f" />
      <Kpi x={207} label="Conversión" value="3,4 %" delta="▼ 12 % vs. ayer" deltaColor="#c2412d" />
      <Kpi x={372} label="Ingresos" value={`$ ${thousands(revenue)}`} delta="▲ 2 % vs. ayer" deltaColor="#2f7d4f" />

      {/* gráfico de barras */}
      <Card x={42} y={224} w={484} h={246}>
        <div style={{ position: "absolute", left: 16, top: 14, fontSize: 12, fontWeight: 600, color: INK }}>
          Ingresos por día
        </div>
        <div style={{ position: "absolute", right: 16, top: 15, fontSize: 10.5, color: SOFT }}>Esta semana</div>
        {[0, 1, 2, 3].map((i) => (
          <div key={i} style={{ ...abs(16, 216 - i * 45, 452, 1), background: "rgb(20 24 30 / 6%)" }} />
        ))}
        {CHART_HEIGHTS.map((h, i) => (
          <div key={i}>
            <div
              style={{
                ...abs(DESK_GEO.bars.left - 42 + DESK_GEO.bars.step * i, DESK_GEO.bars.base - 224 - h, DESK_GEO.bars.width, h),
                borderRadius: "6px 6px 0 0",
                background: i === 3 ? "#9db4d6" : "#5b7fb8",
              }}
            />
            <div
              style={{
                ...abs(DESK_GEO.bars.left - 42 + DESK_GEO.bars.step * i, 220, DESK_GEO.bars.width, 14),
                textAlign: "center",
                fontSize: 10,
                color: SOFT,
              }}
            >
              {DAYS[i]}
            </div>
          </div>
        ))}
      </Card>

      {/* tabla */}
      <Card x={42} y={488} w={484} h={176}>
        <div style={{ position: "absolute", left: 16, top: 14, fontSize: 12, fontWeight: 600, color: INK }}>
          Pedidos recientes
        </div>
        {[
          ["#1041", "Tienda Norte", "Entregado", "#e3f4ea", "#2f7d4f", "$ 48.900"],
          ["#1042", "Almacén Central", "Retrasado", "#fdf0d3", "#a26a0b", "$ 126.500"],
          ["#1043", "Librería del Sur", "En camino", "#e2ecfb", "#2c5fb3", "$ 32.150"],
        ].map(([id, name, status, bg, fg, amount], i) => (
          <div key={id} style={{ ...abs(0, 42 + i * 42, 484, 42), borderTop: "1px solid rgb(20 24 30 / 6%)" }}>
            <div style={{ ...abs(16, 0, 48, 42), lineHeight: "42px", fontSize: 12, color: SOFT }}>{id}</div>
            <div style={{ ...abs(70, 0, 200, 42), lineHeight: "42px", fontSize: 13, fontWeight: 500, color: INK }}>{name}</div>
            <div
              style={{
                ...abs(300, 12, 78, 20),
                borderRadius: 10,
                background: bg,
                color: fg,
                fontSize: 11,
                fontWeight: 600,
                textAlign: "center",
                lineHeight: "20px",
              }}
            >
              {status}
            </div>
            <div
              style={{
                ...abs(390, 0, 78, 42),
                lineHeight: "42px",
                textAlign: "right",
                fontSize: 12,
                color: INK,
                fontVariantNumeric: "tabular-nums",
              }}
            >
              {amount}
            </div>
          </div>
        ))}
      </Card>

      {/* barra de tareas genérica (el área útil termina en su borde superior) */}
      <div
        style={{
          ...abs(0, DESK_GEO.taskbarTop, DESK.w, DESK.h - DESK_GEO.taskbarTop),
          background: "rgb(243 244 246 / 86%)",
          borderTop: "1px solid rgb(20 24 30 / 10%)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 12,
        }}
      >
        {TASKBAR_ICONS.map((c) => (
          <span key={c} style={{ width: 24, height: 24, borderRadius: 7, background: c, opacity: 0.85 }} />
        ))}
      </div>
    </div>
  );
};
