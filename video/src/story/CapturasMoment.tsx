import type { ReactNode } from "react";
import { AbsoluteFill } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Caption } from "../lib/Caption";
import { AppWindow, CHART, D, Desktop, EditorApp, ERROR_LINES, chartGeometry } from "../desk/desk";
import { FlipCursor, cursorPos, type CursorKey } from "../lib/FlipCursor";
import { useFormat, useStageBg } from "../lib/format";
import { Notch } from "../lib/Notch";
import { ArrowCursor } from "../lib/PizarraMarks";
import type { Pt } from "../lib/PizarraBar";
import { Screen, SCREEN } from "../lib/Screen";
import { EASE, FONT_MONO, FONT_SANS, lerp } from "../lib/theme";
import { seg, useMs } from "../lib/time";

/**
 * Herramienta Capturas (Ctrl+Shift+4): el velo cubre el monitor, se ilumina el navegador, se arrastra
 * una región alrededor del gráfico, el recorte se levanta y vuela al shelf de la esquina inferior
 * derecha, y allí se copia con «Copiar». Medidas y tiempos de `specs/pizarra-y-color.md` (A2 y A3).
 */

/* ---------- guion (ms) ---------- */

const T = {
  stripOpen: 60,
  cursorIn: 60,
  hoverFrom: 260,
  hoverTo: 1000,
  stripClose: 1000,
  /** Ctrl + Shift + 4: el escritorio se congela y aparece el velo. */
  key: 1300,
  /** Clic en el punto de partida de la región. */
  press: 2150,
  /** Se suelta el botón: el recorte se levanta. */
  release: 3000,
  /** Clic en «Copiar» del shelf. */
  copy: 4150,
} as const;

/** Levantar el recorte (fase 1) y volar al shelf (fase 2), tiempos de `.cap-fly`. */
const LIFT_MS = 320;
const FLY_MS = 150;
const FLY_START = T.release + LIFT_MS;
const FLY_END = FLY_START + FLY_MS;

const VEIL_FADE_MS = 125;
const CHIP_IN_MS = 125;
const SHELF_IN_MS = 150;
const SHELF_OUT_MS = 125;
const NOTE_MS = 900;
const SHELF_EXIT = T.copy + NOTE_MS;
/** Segundos que el shelf espera antes de irse solo (`capture_shelf_timeout_seconds`). */
const SHELF_TTL_MS = 20000;

/* ---------- geometría del escritorio (coordenadas 0..500 del área de ventanas) ---------- */

const MONITOR_H = SCREEN.h;
const TASKBAR_H = 40;
const CX = SCREEN.w / 2;
/** Celda de Capturas en la tira de la pill (índice 6). */
const PILL_CELL: Pt = [CX + 69, 24];

/** Navegador: se pinta a su tamaño natural y se amplía con `scale`. */
const BROWSER = { x: 40, y: 62, scale: 1.05, w: 400, h: 364 } as const;
const BROWSER_RECT = { x: BROWSER.x, y: BROWSER.y, w: BROWSER.w * BROWSER.scale, h: BROWSER.h * BROWSER.scale };
const TITLE_H = 24;

/** Región arrastrada: 400×250 (aspecto 1.6 = el de la miniatura 192×120), del gráfico a la primera fila. */
const REGION = {
  x: BROWSER.x + BROWSER.scale * 9.5,
  y: BROWSER.y + BROWSER.scale * (TITLE_H + 90),
  w: 400,
  h: 250,
};
const P0: Pt = [REGION.x, REGION.y];
const P1: Pt = [REGION.x + REGION.w, REGION.y + REGION.h];

/** Píxeles de pantalla por pixel lógico, para el chip de medidas. */
const PX_PER_LOGICAL = 2;
const sizeLabel = (w: number, h: number) => `${Math.round(w * PX_PER_LOGICAL)} × ${Math.round(h * PX_PER_LOGICAL)}`;

/* ---------- shelf (208×136 a 16 px de la esquina inferior derecha del área útil) ---------- */

const SHELF = { w: 208, h: 136, pad: 8, margin: 16, thumbW: 192, thumbH: 120 } as const;
const SHELF_TOP = MONITOR_H - TASKBAR_H - SHELF.margin - SHELF.h;
const shelfLeft = (inset: number) => SCREEN.w + inset - SHELF.margin - SHELF.w;

type Rect = { x: number; y: number; w: number; h: number };
const inRect = (p: Pt, r: Rect) => p[0] >= r.x && p[0] <= r.x + r.w && p[1] >= r.y && p[1] <= r.y + r.h;

/** Primer instante (paso 5 ms) en el que se cumple `test`; `null` si nunca. */
const firstMs = (test: (ms: number) => boolean, from: number, to: number): number | null => {
  for (let t = from; t <= to; t += 5) if (test(t)) return t;
  return null;
};

/* ---------- puntero ---------- */

const keysFor = (inset: number): CursorKey[] => {
  // Sobre «Copiar», a la derecha del texto para no taparlo.
  const copyAt: Pt = [shelfLeft(inset) + SHELF.w / 2 + 26, SHELF_TOP + 40];
  return [
    { ms: 60, x: 452, y: 100 },
    { ms: 300, x: PILL_CELL[0], y: PILL_CELL[1] + 1 },
    { ms: 1000, x: PILL_CELL[0], y: PILL_CELL[1] + 1 },
    // Ctrl+Shift+4 con el puntero sobre el papel tapiz; luego entra al navegador y se detiene.
    { ms: T.key, x: 478, y: 250 },
    { ms: 1650, x: 408, y: 282 },
    { ms: 1850, x: 408, y: 282 },
    { ms: T.press, x: P0[0], y: P0[1] },
    // Arrastre: sigue el trazo en tramos lineales hasta la esquina opuesta.
    { ms: T.press + 90, x: P0[0] + 9, y: P0[1] + 6, linear: true },
    { ms: 2650, x: lerp(P0[0], P1[0], 0.62), y: lerp(P0[1], P1[1], 0.6), linear: true },
    { ms: 2920, x: P1[0], y: P1[1], linear: true },
    { ms: T.release, x: P1[0], y: P1[1] },
    { ms: FLY_START, x: P1[0], y: P1[1] },
    { ms: 3800, x: copyAt[0], y: copyAt[1] },
  ];
};

const CLICKS = [T.copy];

/* ---------- navegador (copia del de Pizarra, con la tabla bajo el eje de horas) ---------- */

const CapturasBrowser = ({ liveMs }: { liveMs: number }) => {
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
      <svg width={BROWSER.w} height={BROWSER.h - TITLE_H} style={{ position: "absolute", left: 0, top: 0 }}>
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
      <div style={{ position: "absolute", left: 14, right: 14, top: CHART.y + CHART.h + 28, fontSize: 8, color: D.inkSoft }}>
        {[
          ["14:02", "GET /orders", "980 ms", D.red],
          ["14:02", "GET /orders", "940 ms", D.red],
        ].map(([t, r, ms, c], i) => (
          <div key={i} style={{ display: "flex", alignItems: "center", height: 22, boxSizing: "border-box", borderTop: `1px solid ${D.line}` }}>
            <span style={{ width: 34 }}>{t}</span>
            <span style={{ flex: 1, color: D.ink }}>{r}</span>
            <span style={{ color: c, fontWeight: 600 }}>{ms}</span>
          </div>
        ))}
      </div>
    </AppWindow>
  );
};

/** Navegador ampliado a su tamaño en el escritorio (`BROWSER.scale`). */
const BrowserOnDesk = ({ liveMs }: { liveMs: number }) => (
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
    <CapturasBrowser liveMs={liveMs} />
  </div>
);

/**
 * El recorte de la región dentro de una caja de `w`×`h`: el mismo navegador, escalado y con
 * `overflow: hidden`, para que la miniatura salga nítida en vez de ser una imagen.
 */
const CropContent = ({ w, liveMs }: { w: number; liveMs: number }) => {
  const k = w / REGION.w;
  return (
    <div style={{ position: "absolute", inset: 0, overflow: "hidden" }}>
      <div
        style={{
          position: "absolute",
          left: -(REGION.x - BROWSER.x) * k,
          top: -(REGION.y - BROWSER.y) * k,
          width: BROWSER.w,
          height: BROWSER.h,
          transformOrigin: "0 0",
          transform: `scale(${BROWSER.scale * k})`,
        }}
      >
        <CapturasBrowser liveMs={liveMs} />
      </div>
    </div>
  );
};

/* ---------- iconos del shelf (Lucide, ficha A3) ---------- */

const SHELF_ICON_NODES: Record<"x" | "folder" | "copy" | "pencil" | "scanText", ReactNode> = {
  x: (
    <>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </>
  ),
  folder: <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />,
  copy: (
    <>
      <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    </>
  ),
  pencil: (
    <>
      <path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" />
      <path d="m15 5 4 4" />
    </>
  ),
  scanText: (
    <>
      <path d="M3 7V5a2 2 0 0 1 2-2h2" />
      <path d="M17 3h2a2 2 0 0 1 2 2v2" />
      <path d="M21 17v2a2 2 0 0 1-2 2h-2" />
      <path d="M7 21H5a2 2 0 0 1-2-2v-2" />
      <path d="M7 8h8" />
      <path d="M7 12h10" />
      <path d="M7 16h6" />
    </>
  ),
};

const ShelfIcon = ({ name, size }: { name: keyof typeof SHELF_ICON_NODES; size: number }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={1.75}
    strokeLinecap="round"
    strokeLinejoin="round"
    style={{ display: "block", flex: "none" }}
  >
    {SHELF_ICON_NODES[name]}
  </svg>
);

const SHELF_INK = "#f4f4ee";
const SHELF_RING = "inset 0 0 0 1px rgb(255 255 255 / 16%)";

const ShelfDot = ({ name, corner, t }: { name: "x" | "folder"; corner: "left" | "right"; t: number }) => (
  <div
    style={{
      position: "absolute",
      top: 8,
      [corner]: 8,
      width: 28,
      height: 28,
      borderRadius: 14,
      display: "grid",
      placeItems: "center",
      background: "rgb(18 18 16 / 82%)",
      color: SHELF_INK,
      boxShadow: `0 1px 2px rgb(0 0 0 / 40%), ${SHELF_RING}`,
      opacity: t,
      transform: `scale(${lerp(0.96, 1, t)})`,
    }}
  >
    <ShelfIcon name={name} size={14} />
  </div>
);

const ShelfAction = ({ icon, label, hot }: { icon: "copy" | "pencil" | "scanText"; label: string; hot: boolean }) => (
  <div
    style={{
      minHeight: 26,
      padding: "0 12px",
      borderRadius: 8,
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      gap: 6,
      background: hot ? "rgb(8 8 7 / 90%)" : "rgb(18 18 16 / 78%)",
      color: SHELF_INK,
      fontSize: 12,
      fontWeight: 650,
      boxShadow: SHELF_RING,
      whiteSpace: "nowrap",
    }}
  >
    <ShelfIcon name={icon} size={13} />
    {label}
  </div>
);

const Shelf = ({
  ms,
  left,
  hoverAt,
  overCopy,
}: {
  ms: number;
  left: number;
  hoverAt: number;
  overCopy: boolean;
}) => {
  const inP = EASE.smoothOut(seg(ms, FLY_END, SHELF_IN_MS));
  const outP = EASE.smoothOut(seg(ms, SHELF_EXIT, SHELF_OUT_MS));
  const p = inP * (1 - outP);
  if (p <= 0) return null;

  const hover = seg(ms, hoverAt, 75);
  const noteT = seg(ms, T.copy, 75);
  // La barra TTL se detiene mientras el puntero está encima.
  const ttl = 1 - (Math.min(ms, hoverAt) - FLY_END) / SHELF_TTL_MS;
  const thumbFrozen = T.key;

  return (
    <div
      style={{
        position: "absolute",
        left,
        top: SHELF_TOP,
        width: SHELF.w,
        height: SHELF.h,
        opacity: p,
        transformOrigin: "100% 100%",
        transform: `translateY(${lerp(8, 0, p)}px) scale(${lerp(0.96, 1, p)})`,
        fontFamily: FONT_SANS,
      }}
    >
      <div
        style={{
          position: "absolute",
          left: SHELF.pad,
          top: SHELF.pad,
          width: SHELF.thumbW,
          height: SHELF.thumbH,
          borderRadius: 8,
          overflow: "hidden",
          background: "rgb(8 8 7 / 94%)",
          boxShadow: "0 4px 12px rgb(0 0 0 / 38%)",
        }}
      >
        <CropContent w={SHELF.thumbW} liveMs={thumbFrozen} />
        <div style={{ position: "absolute", inset: 0, borderRadius: 8, background: "rgb(0 0 0 / 38%)", opacity: hover }} />
        <div style={{ position: "absolute", inset: 0, borderRadius: 8, boxShadow: SHELF_RING }} />
      </div>

      <ShelfDot name="x" corner="left" t={hover} />
      <ShelfDot name="folder" corner="right" t={hover} />

      <div
        style={{
          position: "absolute",
          left: SHELF.pad,
          top: SHELF.pad,
          width: SHELF.thumbW,
          height: SHELF.thumbH,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 5,
            minWidth: 88,
            opacity: hover * (1 - noteT),
            transform: `translateY(${lerp(4, 0, hover)}px)`,
          }}
        >
          <ShelfAction icon="copy" label="Copiar" hot={overCopy} />
          <ShelfAction icon="pencil" label="Dibujar" hot={false} />
          <ShelfAction icon="scanText" label="Texto" hot={false} />
        </div>
        {noteT > 0 && (
          <div
            style={{
              position: "absolute",
              padding: "5px 10px",
              borderRadius: 999,
              background: "rgb(18 18 16 / 82%)",
              color: "rgb(157 255 196)",
              fontSize: 11,
              fontWeight: 650,
              whiteSpace: "nowrap",
              opacity: noteT,
            }}
          >
            Copiada al portapapeles
          </div>
        )}
      </div>

      <div
        style={{
          position: "absolute",
          left: SHELF.pad,
          right: SHELF.pad,
          bottom: 4,
          height: 2,
          background: "rgb(240 240 234 / 75%)",
          transformOrigin: "0 50%",
          transform: `scaleX(${Math.max(0, ttl)})`,
        }}
      />
    </div>
  );
};

/* ---------- overlay de selección (CaptureOverlaySurface) ---------- */

const SCRIM = "rgb(0 0 0 / 28%)";
const CAP_BLUE = "#5ec8ff";
const HELP_TEXT = "Clic: ventana · Arrastrar: región · Espacio: pantalla · Esc cancela";
const HELP_W = 412;

const chipStyle = {
  padding: "2px 8px",
  borderRadius: 5,
  background: "rgb(0 0 0 / 75%)",
  color: "#fff",
  fontFamily: FONT_MONO,
  fontSize: 12,
  lineHeight: 1.25,
  fontVariantNumeric: "tabular-nums",
  boxShadow: "inset 0 0 0 1px rgb(255 255 255 / 10%)",
  whiteSpace: "nowrap",
} as const;

/** Velo con un agujero: una sola forma (evenodd) para que no queden costuras entre paños. */
const Veil = ({ hole, inset }: { hole: Rect | null; inset: number }) => {
  const L = -inset - 400;
  const R = SCREEN.w + inset + 400;
  const B = MONITOR_H + 200;
  const T0 = -200;
  const outer = `M${L} ${T0}H${R}V${B}H${L}Z`;
  const inner = hole ? `M${hole.x} ${hole.y}h${hole.w}v${hole.h}h${-hole.w}Z` : "";
  return (
    <svg
      width={R - L}
      height={B - T0}
      style={{ position: "absolute", left: L, top: T0, overflow: "visible" }}
      viewBox={`${L} ${T0} ${R - L} ${B - T0}`}
    >
      <path d={outer + inner} fill={SCRIM} fillRule="evenodd" />
    </svg>
  );
};

/** Borde punteado cian, vértices en L y chips (`.cap-hole`, `.cap-v`, `.cap-size`, `.cap-name`). */
const Selection = ({ rect, name, chipT }: { rect: Rect; name?: string; chipT: number }) => {
  const vSize = Math.min(22, 0.32 * Math.min(rect.w, rect.h));
  const vertex = (pos: Record<string, number>, borders: Record<string, string>, key: string) => (
    <div
      key={key}
      style={{
        position: "absolute",
        width: vSize,
        height: vSize,
        boxSizing: "border-box",
        filter: "drop-shadow(0 0 0.6px rgb(0 0 0 / 55%))",
        ...pos,
        ...borders,
      }}
    />
  );
  const edge = `3px solid ${CAP_BLUE}`;
  return (
    <>
      <div
        style={{
          position: "absolute",
          left: rect.x,
          top: rect.y,
          width: rect.w,
          height: rect.h,
          boxSizing: "border-box",
          border: `2px dashed ${CAP_BLUE}`,
          boxShadow: "0 0 0 1px rgb(0 0 0 / 55%), inset 0 0 0 1px rgb(255 255 255 / 22%)",
        }}
      >
        {vertex({ left: -2, top: -2 }, { borderLeft: edge, borderTop: edge }, "tl")}
        {vertex({ right: -2, top: -2 }, { borderRight: edge, borderTop: edge }, "tr")}
        {vertex({ left: -2, bottom: -2 }, { borderLeft: edge, borderBottom: edge }, "bl")}
        {vertex({ right: -2, bottom: -2 }, { borderRight: edge, borderBottom: edge }, "br")}
      </div>
      <div
        style={{
          position: "absolute",
          left: rect.x,
          top: rect.y - 6,
          transform: `translateY(calc(-100% - ${(1 - chipT) * 4}px))`,
          opacity: chipT,
          display: "flex",
          flexDirection: "column",
          alignItems: "flex-start",
          gap: 4,
          fontFamily: FONT_MONO,
        }}
      >
        {name && <div style={{ ...chipStyle, fontFamily: FONT_SANS, fontWeight: 650 }}>{name}</div>}
        <div style={chipStyle}>{sizeLabel(rect.w, rect.h)}</div>
      </div>
    </>
  );
};

const rectBetween = (a: Pt, b: Pt): Rect => ({
  x: Math.min(a[0], b[0]),
  y: Math.min(a[1], b[1]),
  w: Math.abs(a[0] - b[0]),
  h: Math.abs(a[1] - b[1]),
});

/* ---------- escena ---------- */

export const CapturasMoment = () => {
  const ms = useMs();
  const format = useFormat();
  const inset = ((format.logicalW ?? SCREEN.w) - SCREEN.w) / 2;
  const keys = keysFor(inset);
  const cursorAt = (t: number): Pt => {
    const p = cursorPos(keys, t);
    return p ? [p.x, p.y] : [keys[0].x, keys[0].y];
  };
  const cursor = cursorAt(ms);

  const shelfX = shelfLeft(inset);
  const shelfRect: Rect = { x: shelfX, y: SHELF_TOP, w: SHELF.w, h: SHELF.h };
  const copyRect: Rect = { x: shelfX + SHELF.w / 2 - 44, y: SHELF_TOP + 24, w: 88, h: 26 };

  /* Instantes que dependen de la trayectoria del puntero. */
  const winEnter = firstMs((t) => inRect(cursorAt(t), BROWSER_RECT), T.key, T.press) ?? T.key;
  const regionEnter = firstMs((t) => Math.hypot(cursorAt(t)[0] - P0[0], cursorAt(t)[1] - P0[1]) > 4, T.press, T.release) ?? T.press;
  const hoverAt = firstMs((t) => inRect(cursorAt(t), shelfRect), FLY_END, T.copy) ?? T.copy;

  /* Overlay: activo entre Ctrl+Shift+4 y el fin del vuelo; el velo se funde después. */
  const overlayIn = EASE.smoothOut(seg(ms, T.key, VEIL_FADE_MS));
  const overlayOut = EASE.smoothOut(seg(ms, FLY_END, VEIL_FADE_MS));
  const overlayOpacity = overlayIn * (1 - overlayOut);
  const overlayActive = ms >= T.key && ms < FLY_END;
  const selecting = ms >= T.key && ms < T.release;
  const frozenMs = overlayActive ? T.key : ms;

  /* Agujero: la ventana bajo el puntero, o la región cuando el arrastre pasa de 4 px. */
  const dragging = ms >= regionEnter && ms < T.release;
  let hole: Rect | null = null;
  let holeName: string | undefined;
  let chipT = 0;
  if (selecting && dragging) {
    hole = rectBetween(P0, cursor);
    chipT = EASE.smoothOut(seg(ms, regionEnter, CHIP_IN_MS));
  } else if (selecting && ms >= winEnter) {
    hole = BROWSER_RECT;
    holeName = "Navegador";
    chipT = EASE.smoothOut(seg(ms, winEnter, CHIP_IN_MS));
  }

  /* Ayuda: sigue al puntero solo en X. */
  const helpT = EASE.smoothOut(seg(ms, T.key + 40, CHIP_IN_MS));
  const helpLo = -inset + 8 + HELP_W / 2;
  const helpHi = SCREEN.w + inset - 8 - HELP_W / 2;
  const helpX = helpLo > helpHi ? CX : Math.min(helpHi, Math.max(helpLo, cursor[0]));

  /* Recorte que se levanta y vuela al shelf. */
  const lift = EASE.smoothOut(seg(ms, T.release, LIFT_MS));
  const fly = EASE.smoothOut(seg(ms, FLY_START, FLY_MS));
  const thumb: Rect = { x: shelfX + SHELF.pad, y: SHELF_TOP + SHELF.pad, w: SHELF.thumbW, h: SHELF.thumbH };
  const cropShown = ms >= T.release && ms < FLY_END + SHELF_IN_MS;
  const rise = lift * (1 - fly);
  const flyBlur = fly < 0.4 ? 4 * (fly / 0.4) : 4 * (1 - (fly - 0.4) / 0.6);
  const shadowY = lerp(lift * 24, 8, fly);
  const shadowBlur = lerp(lift * 70, 24, fly);
  const shadowAlpha = lerp(lift * 0.45, 0.32, fly);

  const overCopy = ms >= hoverAt && inRect(cursor, copyRect);
  // El puntero del sistema se oculta mientras dura el overlay (que dibuja el suyo) y vuelve al cerrarse.
  const cursorOpacity = overlayActive ? 0 : ms >= FLY_END ? seg(ms, FLY_END, 120) : 1;

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Capturas" sub="Un recorte, al instante" keys={["Ctrl", "Shift", "4"]} />
      <Screen wallpaper={false}>
        <Desktop time="14:32">
          <EditorApp rect={{ x: 10, y: 84, w: 400, h: 430 }} termLines={ERROR_LINES} termH={84} highlightLine={10} />
          <BrowserOnDesk liveMs={frozenMs} />
        </Desktop>

        {overlayOpacity > 0 && (
          <div style={{ position: "absolute", left: 0, top: 0, width: 0, height: 0, opacity: overlayOpacity }}>
            <Veil hole={hole} inset={inset} />
            {hole && <Selection rect={hole} name={holeName} chipT={chipT} />}
            {selecting && (
              <div
                style={{
                  position: "absolute",
                  left: helpX,
                  top: MONITOR_H - 32,
                  transform: `translate(-50%, calc(-100% + ${(1 - helpT) * 4}px))`,
                  opacity: helpT,
                  padding: "6px 14px",
                  borderRadius: 8,
                  background: "rgb(0 0 0 / 75%)",
                  color: "#fff",
                  fontFamily: FONT_SANS,
                  fontSize: 14,
                  lineHeight: 1.4,
                  whiteSpace: "nowrap",
                  boxShadow: "0 8px 24px rgb(0 0 0 / 28%), inset 0 0 0 1px rgb(255 255 255 / 10%)",
                }}
              >
                {HELP_TEXT}
              </div>
            )}
          </div>
        )}

        <Notch
          cx={CX}
          toggles={[T.stripOpen, T.stripClose]}
          hovers={[{ index: 6, fromMs: T.hoverFrom, toMs: T.hoverTo }]}
          lookX={Math.max(-1, Math.min(1, (cursor[0] - CX) / 160)) * 0.9}
          lookY={Math.max(0, Math.min(1, (cursor[1] - 20) / 240)) * 0.7}
          lid={blinkLid(ms, [620])}
        />

        <Shelf ms={ms} left={shelfX} hoverAt={hoverAt} overCopy={overCopy} />

        {cropShown && (
          <div
            style={{
              position: "absolute",
              left: lerp(REGION.x, thumb.x, fly),
              top: lerp(REGION.y, thumb.y, fly),
              width: lerp(REGION.w, thumb.w, fly),
              height: lerp(REGION.h, thumb.h, fly),
              borderRadius: lerp(5, 5, fly) + 2 * rise,
              overflow: "hidden",
              transform: `translateY(${-14 * rise}px) scale(${1 + 0.03 * rise})`,
              boxShadow: `0 ${shadowY}px ${shadowBlur}px rgb(0 0 0 / ${shadowAlpha})`,
              filter: flyBlur > 0.01 ? `blur(${flyBlur}px)` : undefined,
            }}
          >
            <CropContent w={lerp(REGION.w, thumb.w, fly)} liveMs={T.key} />
            <div
              style={{
                position: "absolute",
                inset: 0,
                borderRadius: "inherit",
                boxShadow: `inset 0 0 0 1px rgb(255 255 255 / ${0.18 * rise})`,
              }}
            />
          </div>
        )}

        {selecting && ms >= T.key && (
          <div style={{ opacity: overlayIn }}>
            <ArrowCursor x={cursor[0]} y={cursor[1]} />
          </div>
        )}

        <div style={{ opacity: cursorOpacity }}>
          <FlipCursor ms={ms} keys={keys} clicks={CLICKS} size={16} />
        </div>
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos de la escena (ms) que, seguidos, la cuentan en ≈1.9 s (≈1.35×): el velo con la ventana
 * iluminada, el arrastre de la región, el recorte que se levanta y vuela, y el shelf con
 * «Copiar» y su aviso.
 */
export const CAPTURAS_SEGMENTS: [number, number][] = [
  [1350, 1750],
  [2150, 2950],
  [3000, 3600],
  [3700, 4450],
];
/** El shelf con las tres acciones a la vista y el puntero sobre «Copiar», justo antes del clic. */
export const CAPTURAS_HERO_MS = 4100;
