import { AppWindow, CHART, D, Desktop, chartGeometry } from "../desk/desk";
import type { Pt } from "../lib/PizarraBar";
import { FONT_SANS } from "../lib/theme";

/**
 * Escritorio de la pizarra en la historia: la pantalla lógica de 500x660 vista con zoom 0.88
 * (así cabe la barra de ~504 px), con el navegador simulado grande y su pico de las 14:02.
 * Coordenadas propias de este escritorio (568x750), igual que `lib/PizarraDesk`.
 */
export const STORY_DESK = { w: 568, h: 750, cx: 284, zoom: 500 / 568, taskbarH: 40 } as const;
/** Borde superior de la barra de tareas (el área útil termina aquí). */
export const TASKBAR_TOP = STORY_DESK.h - STORY_DESK.taskbarH;

/** El navegador se pinta a su tamaño natural (340x394) y se amplía con `scale`. */
export const BROWSER = { x: 29, y: 74, scale: 1.5, w: 340, h: 394 } as const;

/** Punto del contenido del navegador (coordenadas de `chartGeometry`) en el escritorio. */
export const browserToDesk = (x: number, y: number): Pt => [
  BROWSER.x + BROWSER.scale * x,
  // la barra de título de la ventana mide 24 px
  BROWSER.y + BROWSER.scale * (24 + y),
];

/**
 * Navegador de `desk.tsx` con la misma geometría del gráfico (`chartGeometry`), pero con la
 * tabla más abajo para que no pise la base del gráfico y con las horas del eje visibles.
 */
const StoryBrowser = ({ liveMs }: { liveMs: number }) => {
  const g = chartGeometry(BROWSER.w);
  const line = g.pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`).join(" ");
  const area = `${line} L${g.pts[g.pts.length - 1][0]} ${CHART.y + CHART.h} L${CHART.x} ${CHART.y + CHART.h} Z`;
  const dot = 0.45 + 0.55 * Math.abs(Math.sin(liveMs / 320));
  return (
    <AppWindow rect={{ x: 0, y: 0, w: BROWSER.w, h: BROWSER.h }} title="Panel · Rendimiento — Navegador">
      <div style={{ height: 22, background: D.paper2, display: "flex", alignItems: "center", gap: 6, padding: "0 8px", fontSize: 8, color: D.inkSoft }}>
        <span style={{ flex: 1, background: "#fff", borderRadius: 11, padding: "2px 10px", border: `1px solid ${D.line}` }}>panel.tienda.cl/rendimiento</span>
      </div>
      <div style={{ padding: "8px 14px 0", fontSize: 8.5, color: D.ink }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <div style={{ fontSize: 12, fontWeight: 650 }}>Tiempo de respuesta</div>
          <div style={{ display: "flex", alignItems: "center", gap: 4, padding: "2px 7px", borderRadius: 9, background: "#e3f4ea", color: "#2f7d4f", fontSize: 7.5, fontWeight: 600 }}>
            <span style={{ width: 4.5, height: 4.5, borderRadius: 5, background: "#2f9e5b", opacity: dot }} />
            En vivo
          </div>
        </div>
        <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
          {[
            ["p95", "980 ms", D.red],
            ["Errores", "4,2 %", D.red],
            ["Pedidos / min", "18", D.amber],
          ].map(([k, v, c]) => (
            <div key={k} style={{ flex: 1, border: `1px solid ${D.line}`, borderRadius: 6, padding: "5px 7px" }}>
              <div style={{ color: D.inkSoft, fontSize: 7.5 }}>{k}</div>
              <div style={{ fontSize: 11.5, fontWeight: 650, color: c }}>{v}</div>
            </div>
          ))}
        </div>
      </div>
      <svg width={BROWSER.w} height={BROWSER.h - 24} style={{ position: "absolute", left: 0, top: 0 }}>
        {[0, 1, 2, 3].map((i) => (
          <line key={i} x1={CHART.x} x2={CHART.x + g.w} y1={CHART.y + (i * CHART.h) / 3} y2={CHART.y + (i * CHART.h) / 3} stroke={D.line} />
        ))}
        <path d={area} fill="rgb(47 111 235 / 12%)" />
        <path d={line} fill="none" stroke={D.blue} strokeWidth={1.8} strokeLinejoin="round" />
        {g.pts.map(([x, y], i) => (
          <circle key={i} cx={x} cy={y} r={i === 8 ? 3 : 1.6} fill={i === 8 ? D.red : D.blue} />
        ))}
        {["10:00", "11:00", "12:00", "13:00", "14:00"].map((t, i) => (
          <text key={t} x={CHART.x + (i * g.w) / 4} y={CHART.y + CHART.h + 12} fontSize={7} fill={D.inkSoft} textAnchor={i === 0 ? "start" : i === 4 ? "end" : "middle"}>
            {t}
          </text>
        ))}
      </svg>
      <div style={{ position: "absolute", left: 14, right: 14, top: CHART.y + CHART.h + 22, fontSize: 8, color: D.inkSoft }}>
        {[
          ["14:02", "GET /orders", "980 ms", D.red],
          ["14:02", "GET /orders", "940 ms", D.red],
          ["14:01", "GET /health", "120 ms", D.green],
        ].map(([t, r, ms, c], i) => (
          <div key={i} style={{ display: "flex", padding: "4px 0", borderTop: `1px solid ${D.line}` }}>
            <span style={{ width: 34 }}>{t}</span>
            <span style={{ flex: 1, color: D.ink }}>{r}</span>
            <span style={{ color: c, fontWeight: 600 }}>{ms}</span>
          </div>
        ))}
      </div>
    </AppWindow>
  );
};

/**
 * `liveMs` marca el reloj de lo que se mueve en pantalla (el punto «en vivo»):
 * al congelarse, la escena deja de avanzarlo.
 */
export const PizarraStoryDesk = ({ liveMs }: { liveMs: number }) => {
  return (
    <div style={{ position: "absolute", left: 0, top: 0, width: STORY_DESK.w, height: STORY_DESK.h, fontFamily: FONT_SANS }}>
      <Desktop time="14:32">
        <div
          style={{
            position: "absolute",
            left: BROWSER.x,
            top: BROWSER.y,
            width: BROWSER.w,
            height: BROWSER.h,
            transformOrigin: "0 0",
            transform: `scale(${BROWSER.scale})`,
          }}
        >
          <StoryBrowser liveMs={liveMs} />
        </div>
      </Desktop>
    </div>
  );
};
