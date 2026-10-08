import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill } from "remotion";
import { useFormat } from "../lib/format";
import { Icon } from "../lib/Icon";
import { FONT_MONO, FONT_SANS } from "../lib/theme";

/**
 * Escritorio simulado (monitor vertical de 500×660 lógicos, los mismos de `Screen`).
 * Componentes de «ventanas» genéricas y creíbles para las escenas de la historia:
 * un bug en producción que cada herramienta de Atic ayuda a resolver.
 * Nada de marcas reales: los programas son estándar (editor, navegador, chat, diseño).
 */
export const DESK = { w: 500, h: 660, taskbarH: 40 } as const;

/** Colores del escritorio (no son de Atic: son de los programas simulados). */
export const D = {
  ink: "#1c2430",
  inkSoft: "#5b6675",
  line: "rgb(28 36 48 / 10%)",
  paper: "#ffffff",
  paper2: "#f4f6f9",
  blue: "#2f6feb",
  red: "#e5484d",
  green: "#2f9e6a",
  amber: "#e2a23b",
  orange: "#ff6b3d",
  // editor oscuro
  edBg: "#1b1d22",
  edSide: "#15171b",
  edLine: "#2a2d34",
  edText: "#d7dae0",
  edMuted: "#6c7383",
  kw: "#c792ea",
  str: "#e7b27a",
  fn: "#82aaff",
  ty: "#5ccfb3",
  num: "#f78c6c",
  cm: "#5f7e97",
} as const;

/* -------------------------------- fondo y barra -------------------------------- */

export const DeskWallpaper = () => {
  const light = useFormat().wallpaper === "light";
  return (
    <div
      style={{
        position: "absolute",
        left: -400,
        right: -400,
        top: 0,
        bottom: 0,
        background: light
          ? "radial-gradient(70% 50% at 25% 10%, #ffffff 0%, transparent 65%), radial-gradient(60% 50% at 85% 85%, #e3e7f7 0%, transparent 70%), linear-gradient(170deg, #f7f8fc 0%, #eceff8 55%, #dfe4f3 100%)"
          : "radial-gradient(90% 60% at 20% 8%, #7fa6ff 0%, transparent 60%), radial-gradient(80% 60% at 85% 70%, #c9b6f5 0%, transparent 62%), linear-gradient(165deg, #2d59c9 0%, #5b86ec 38%, #aebff0 74%, #e3e9fa 100%)",
      }}
    />
  );
};

/** Iconos de escritorio a la izquierda (solo se ven si el monitor es más ancho que 500). */
const DesktopIcons = () => (
  <div style={{ position: "absolute", left: -66, top: 70, display: "grid", gap: 16, justifyItems: "center", fontSize: 7.5, color: "#4a5568" }}>
    {[
      ["#f2c94c", "Proyectos"],
      ["#5b8def", "Informes"],
      ["#9aa3b5", "Papelera"],
    ].map(([c, t]) => (
      <div key={t} style={{ textAlign: "center", width: 48 }}>
        <div style={{ width: 26, height: 22, margin: "0 auto 3px", borderRadius: 4, background: c }} />
        {t}
      </div>
    ))}
  </div>
);

const TaskGlyph = ({ children, active }: { children: ReactNode; active?: boolean }) => (
  <div style={{ position: "relative", width: 26, height: 26, display: "grid", placeItems: "center", borderRadius: 7 }}>
    {children}
    {active && <span style={{ position: "absolute", bottom: -5, width: 10, height: 2.5, borderRadius: 2, background: D.blue }} />}
  </div>
);

export const Taskbar = ({ time = "14:32" }: { time?: string }) => {
  const format = useFormat();
  const inset = ((format.logicalW ?? DESK.w) - DESK.w) / 2;
  return (
  <div
    style={{
      position: "absolute",
      left: -inset,
      right: -inset,
      bottom: 0,
      height: DESK.taskbarH,
      background: "rgb(243 246 252 / 78%)",
      backdropFilter: "blur(14px)",
      borderTop: "1px solid rgb(255 255 255 / 60%)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      gap: 10,
      fontFamily: FONT_SANS,
    }}
  >
    <TaskGlyph>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 8px)", gap: 2 }}>
        {[0, 1, 2, 3].map((i) => (
          <span key={i} style={{ width: 8, height: 8, borderRadius: 2, background: D.blue }} />
        ))}
      </div>
    </TaskGlyph>
    <TaskGlyph>
      <span style={{ color: D.inkSoft, display: "grid" }}>
        <Icon name="search" size={16} strokeWidth={1.9} />
      </span>
    </TaskGlyph>
    <TaskGlyph active>
      <span style={{ width: 16, height: 16, borderRadius: 4, background: "#243044" }} />
    </TaskGlyph>
    <TaskGlyph active>
      <span style={{ width: 16, height: 16, borderRadius: 16, background: "conic-gradient(#e0574c 0 120deg, #e6b93f 120deg 240deg, #4fa870 240deg 360deg)" }} />
    </TaskGlyph>
    <TaskGlyph>
      <span style={{ width: 16, height: 14, borderRadius: 4, background: D.amber }} />
    </TaskGlyph>
    <TaskGlyph active>
      <span style={{ width: 16, height: 16, borderRadius: 5, background: "#6d4bd8" }} />
    </TaskGlyph>
    <div style={{ position: "absolute", right: 14, textAlign: "right", fontSize: 8.5, lineHeight: 1.25, color: D.ink }}>
      {time}
      <br />
      <span style={{ color: D.inkSoft }}>29-09-2026</span>
    </div>
  </div>
  );
};

/** Fondo completo: papel tapiz + barra. Las ventanas van encima como hijos. */
export const Desktop = ({ children, time }: { children?: ReactNode; time?: string }) => {
  const format = useFormat();
  const wide = (format.logicalW ?? DESK.w) > DESK.w;
  return (
    <div style={{ position: "absolute", left: -400, right: -400, top: 0, bottom: 0, fontFamily: FONT_SANS }}>
      <div style={{ position: "absolute", left: 400, top: 0, width: DESK.w, height: DESK.h }}>
        <DeskWallpaper />
        {wide && <DesktopIcons />}
        {children}
        <Taskbar time={time} />
      </div>
    </div>
  );
};

/* ---------------------------------- ventanas ---------------------------------- */

export type Rect = { x: number; y: number; w: number; h: number };

export const AppWindow = ({
  rect,
  title,
  dark = false,
  focused = true,
  style,
  children,
}: {
  rect: Rect;
  title: string;
  dark?: boolean;
  focused?: boolean;
  style?: CSSProperties;
  children: ReactNode;
}) => {
  const bar = dark ? "#101216" : "#eef1f6";
  const fg = dark ? "#9aa3b5" : D.inkSoft;
  return (
    <div
      style={{
        position: "absolute",
        left: rect.x,
        top: rect.y,
        width: rect.w,
        height: rect.h,
        borderRadius: 8,
        overflow: "hidden",
        background: dark ? D.edBg : D.paper,
        boxShadow: focused
          ? "0 18px 44px rgb(15 25 55 / 34%), 0 0 0 1px rgb(15 25 55 / 18%)"
          : "0 10px 26px rgb(15 25 55 / 22%), 0 0 0 1px rgb(15 25 55 / 12%)",
        ...style,
      }}
    >
      <div style={{ height: 24, background: bar, display: "flex", alignItems: "center", padding: "0 8px", fontSize: 8.5, color: fg, fontWeight: 500, gap: 6 }}>
        <span style={{ flex: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{title}</span>
        {["—", "▢", "✕"].map((g) => (
          <span key={g} style={{ width: 18, textAlign: "center", fontSize: 8 }}>
            {g}
          </span>
        ))}
      </div>
      <div style={{ position: "absolute", left: 0, right: 0, top: 24, bottom: 0 }}>{children}</div>
    </div>
  );
};

/* ---------------------------------- editor ---------------------------------- */

type Tok = [string, string];
const CODE: Tok[][] = [
  [["import ", D.kw], ["{ Pool } ", D.edText], ["from ", D.kw], ['"pg"', D.str], [";", D.edText]],
  [],
  [["const ", D.kw], ["pool ", D.edText], ["= new ", D.kw], ["Pool", D.ty], ["({", D.edText]],
  [["  host: ", D.edText], ["process.env.DB_HOST", D.fn], [",", D.edText]],
  [["  max: ", D.edText], ["5", D.num], [",", D.edText], ["  // pocas conexiones", D.cm]],
  [["  connectionTimeoutMillis: ", D.edText], ["2000", D.num], [",", D.edText]],
  [["});", D.edText]],
  [],
  [["export async function ", D.kw], ["getOrders", D.fn], ["(id: ", D.edText], ["string", D.ty], [") {", D.edText]],
  [["  const ", D.kw], ["{ rows } ", D.edText], ["= await ", D.kw], ["pool.query", D.fn], ["(", D.edText]],
  [['    "SELECT * FROM orders WHERE user = $1"', D.str], [",", D.edText]],
  [["    [id]", D.edText]],
  [["  );", D.edText]],
  [["  return ", D.kw], ["rows", D.edText], [";", D.edText]],
  [["}", D.edText]],
];

export const CODE_LINE_H = 11;

export type TermLine = { t: string; c?: "err" | "ok" | "dim" | "cmd" | "warn" | "white" };
export const TERM_COLOR: Record<NonNullable<TermLine["c"]>, string> = {
  err: "#ff7b72",
  ok: "#7ee787",
  dim: "#7d8590",
  cmd: "#e6edf3",
  warn: "#e3b341",
  white: "#e6edf3",
};

/** Terminal integrada del editor. `lines` puede crecer con el tiempo. */
export const TerminalPanel = ({
  lines,
  height,
  cursor = false,
  fontSize = 8.6,
}: {
  lines: TermLine[];
  height: number;
  cursor?: boolean;
  fontSize?: number;
}) => (
  <div
    style={{
      height,
      background: "#0f1115",
      borderTop: `1px solid ${D.edLine}`,
      fontFamily: FONT_MONO,
      fontSize,
      lineHeight: 1.38,
      padding: "6px 10px",
      overflow: "hidden",
      color: TERM_COLOR.white,
    }}
  >
    <div style={{ fontFamily: FONT_SANS, fontSize: 7.5, letterSpacing: "0.08em", color: D.edMuted, marginBottom: 3 }}>TERMINAL</div>
    {lines.map((l, i) => (
      <div key={i} style={{ color: TERM_COLOR[l.c ?? "white"], whiteSpace: "pre" }}>
        {l.t}
        {cursor && i === lines.length - 1 && <span style={{ background: "#e6edf3", marginLeft: 1 }}>&nbsp;</span>}
      </div>
    ))}
  </div>
);

/** Errores de la prueba que fallan (los que se copian en la historia). */
export const ERROR_LINES: TermLine[] = [
  { t: "$ npm test", c: "cmd" },
  { t: " FAIL  src/orders.test.ts", c: "err" },
  { t: "  ● getOrders › devuelve los pedidos", c: "err" },
  { t: "    Error: connect ETIMEDOUT 10.0.4.12:5432", c: "err" },
  { t: "      at pool.query (src/db.ts:88:14)", c: "dim" },
];

/** Editor + terminal. `termLines` y `termH` definen el panel de abajo. */
export const EditorApp = ({
  rect,
  termLines = [],
  termH = 0,
  highlightLine,
}: {
  rect: Rect;
  termLines?: TermLine[];
  termH?: number;
  highlightLine?: number;
}) => (
  <AppWindow rect={rect} title="db.ts — tienda-api — Editor" dark>
    <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column" }}>
      <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
        <div style={{ width: 26, background: D.edSide, display: "grid", gridAutoRows: 22, justifyItems: "center", alignContent: "start", paddingTop: 6, gap: 2 }}>
          {[D.edMuted, D.edMuted, D.edMuted].map((c, i) => (
            <span key={i} style={{ width: 10, height: 10, borderRadius: 3, border: `1.5px solid ${i === 0 ? "#9fb3ff" : c}` }} />
          ))}
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ height: 20, display: "flex", background: D.edSide, fontSize: 8, fontFamily: FONT_SANS }}>
            {["db.ts", "orders.test.ts"].map((t, i) => (
              <span key={t} style={{ padding: "0 10px", display: "flex", alignItems: "center", background: i === 0 ? D.edBg : "transparent", color: i === 0 ? D.edText : D.edMuted, borderTop: i === 0 ? "1.5px solid #82aaff" : "1.5px solid transparent" }}>
                {t}
              </span>
            ))}
          </div>
          <div style={{ padding: "5px 0", fontFamily: FONT_MONO, fontSize: 8.6, lineHeight: `${CODE_LINE_H}px` }}>
            {CODE.map((toks, i) => (
              <div key={i} style={{ display: "flex", background: highlightLine === i ? "rgb(255 123 114 / 14%)" : "transparent", height: CODE_LINE_H }}>
                <span style={{ width: 24, textAlign: "right", paddingRight: 8, color: D.edMuted }}>{i + 78}</span>
                <span style={{ whiteSpace: "pre" }}>
                  {toks.map(([t, c], j) => (
                    <span key={j} style={{ color: c }}>
                      {t}
                    </span>
                  ))}
                </span>
              </div>
            ))}
          </div>
        </div>
      </div>
      {termH > 0 && <TerminalPanel lines={termLines} height={termH} />}
    </div>
  </AppWindow>
);

/* --------------------------------- navegador --------------------------------- */

const LAT = [120, 118, 126, 122, 131, 128, 135, 140, 980, 940, 610, 300];
export const CHART = { x: 14, y: 128, w: 0, h: 150 }; // `w` se calcula con el ancho de la ventana
export const chartGeometry = (winW: number) => {
  const w = winW - 28;
  const step = w / (LAT.length - 1);
  const yOf = (v: number) => CHART.y + CHART.h - (v / 1050) * CHART.h;
  const pts = LAT.map((v, i) => [CHART.x + i * step, yOf(v)] as const);
  return { w, step, pts, spike: pts[8], yOf };
};

/** Navegador con un panel de rendimiento: el tiempo de respuesta se dispara. */
export const BrowserApp = ({ rect }: { rect: Rect }) => {
  const g = chartGeometry(rect.w);
  const line = g.pts.map(([x, y], i) => `${i ? "L" : "M"}${x.toFixed(1)} ${y.toFixed(1)}`).join(" ");
  const area = `${line} L${g.pts[g.pts.length - 1][0]} ${CHART.y + CHART.h} L${CHART.x} ${CHART.y + CHART.h} Z`;
  return (
    <AppWindow rect={rect} title="Panel · Rendimiento — Navegador">
      <div style={{ height: 22, background: D.paper2, display: "flex", alignItems: "center", gap: 6, padding: "0 8px", fontSize: 8, color: D.inkSoft }}>
        <span style={{ flex: 1, background: "#fff", borderRadius: 11, padding: "2px 10px", border: `1px solid ${D.line}` }}>panel.tienda.cl/rendimiento</span>
      </div>
      <div style={{ padding: "8px 14px 0", fontSize: 8.5, color: D.ink }}>
        <div style={{ fontSize: 12, fontWeight: 650 }}>Tiempo de respuesta</div>
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
      <svg width={rect.w} height={220} style={{ position: "absolute", left: 0, top: 46 }}>
        <g transform="translate(0,-46)">
          {[0, 1, 2, 3].map((i) => (
            <line key={i} x1={CHART.x} x2={CHART.x + g.w} y1={CHART.y + (i * CHART.h) / 3} y2={CHART.y + (i * CHART.h) / 3} stroke={D.line} />
          ))}
          <path d={area} fill="rgb(47 111 235 / 12%)" />
          <path d={line} fill="none" stroke={D.blue} strokeWidth={1.8} strokeLinejoin="round" />
          {g.pts.map(([x, y], i) => (
            <circle key={i} cx={x} cy={y} r={i === 8 ? 3 : 1.6} fill={i === 8 ? D.red : D.blue} />
          ))}
        </g>
        {["10:00", "11:00", "12:00", "13:00", "14:00"].map((t, i) => (
          <text key={t} x={CHART.x + (i * g.w) / 4} y={CHART.y - 46 + CHART.h + 14} fontSize={7} fill={D.inkSoft} textAnchor={i === 0 ? "start" : i === 4 ? "end" : "middle"}>
            {t}
          </text>
        ))}
      </svg>
      <div style={{ position: "absolute", left: 14, right: 14, top: 258, fontSize: 8, color: D.inkSoft }}>
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

/* ----------------------------------- chat ----------------------------------- */

export const CHAT_COMPOSER_H = 44;

export const ChatApp = ({
  rect,
  composer = "",
  showCaret = false,
  extraMessage,
}: {
  rect: Rect;
  composer?: string;
  showCaret?: boolean;
  /** Mensaje enviado por el usuario (aparece tras pulsar Enter). */
  extraMessage?: string;
}) => {
  const msgs: { who: string; t: string; mine?: boolean; time: string }[] = [
    { who: "Lucía Fuentes", time: "14:28", t: "¿Siguen con problemas? El panel no carga las ventas." },
    { who: "Tú", time: "14:29", t: "Estoy revisándolo ahora.", mine: true },
    { who: "Lucía Fuentes", time: "14:31", t: "El cliente pregunta cuándo se arregla." },
  ];
  if (extraMessage) msgs.push({ who: "Tú", time: "14:33", t: extraMessage, mine: true });
  return (
    <AppWindow rect={rect} title="# soporte — Mensajes">
      <div style={{ position: "absolute", inset: 0, display: "flex" }}>
        <div style={{ width: 36, background: "#2b2350", display: "grid", alignContent: "start", justifyItems: "center", gap: 8, paddingTop: 8 }}>
          {["#8b6df0", "#5b4bb0", "#5b4bb0", "#5b4bb0"].map((c, i) => (
            <span key={i} style={{ width: 18, height: 18, borderRadius: 6, background: c }} />
          ))}
        </div>
        <div style={{ flex: 1, position: "relative", minWidth: 0 }}>
          <div style={{ padding: "8px 12px", display: "grid", gap: 9, fontSize: 8.8, lineHeight: 1.35 }}>
            {msgs.map((m, i) => (
              <div key={i} style={{ display: "flex", gap: 7 }}>
                <span style={{ width: 20, height: 20, borderRadius: 7, background: m.mine ? "#6d4bd8" : "#e2a23b", flex: "none" }} />
                <div>
                  <div style={{ color: D.ink, fontWeight: 650 }}>
                    {m.who} <span style={{ color: D.inkSoft, fontWeight: 400, fontSize: 7.5 }}>{m.time}</span>
                  </div>
                  <div style={{ color: D.ink }}>{m.t}</div>
                </div>
              </div>
            ))}
          </div>
          <div style={{ position: "absolute", left: 10, right: 10, bottom: 8, height: CHAT_COMPOSER_H - 8, border: `1px solid ${D.line}`, borderRadius: 8, background: "#fff", padding: "6px 9px", fontSize: 8.8, lineHeight: 1.35, color: composer ? D.ink : D.inkSoft }}>
            {composer || "Responder en #soporte…"}
            {showCaret && <span style={{ display: "inline-block", width: 1, height: 10, background: D.ink, verticalAlign: "middle", marginLeft: 1 }} />}
          </div>
        </div>
      </div>
    </AppWindow>
  );
};

/* ---------------------------------- diseño ---------------------------------- */

export const SWATCHES = [
  { name: "Primario", hex: "#FF6B3D" },
  { name: "Acento", hex: "#2F80ED" },
  { name: "Éxito", hex: "#27AE60" },
  { name: "Aviso", hex: "#F2C94C" },
  { name: "Tinta", hex: "#1C2430" },
];

/** Lienzo de diseño con una tarjeta y la paleta de la marca. */
export const DesignApp = ({ rect }: { rect: Rect }) => (
  <AppWindow rect={rect} title="Marca — Diseño">
    <div style={{ position: "absolute", inset: 0, display: "flex", background: "#e9ebf0" }}>
      <div style={{ width: 26, background: "#fff", display: "grid", alignContent: "start", justifyItems: "center", gap: 10, paddingTop: 8, borderRight: `1px solid ${D.line}` }}>
        {[0, 1, 2, 3].map((i) => (
          <span key={i} style={{ width: 11, height: 11, borderRadius: i === 1 ? 6 : 2, border: `1.5px solid ${i === 0 ? D.blue : D.inkSoft}` }} />
        ))}
      </div>
      <div style={{ flex: 1, padding: 14, position: "relative" }}>
        <div style={{ background: "#fff", borderRadius: 10, padding: 14, boxShadow: "0 6px 18px rgb(20 30 60 / 12%)" }}>
          <div style={{ fontSize: 8, letterSpacing: "0.08em", color: D.inkSoft }}>TIENDA</div>
          <div style={{ fontSize: 15, fontWeight: 700, color: D.ink, lineHeight: 1.15, marginTop: 3 }}>Tu pedido, más rápido</div>
          <div style={{ fontSize: 8.5, color: D.inkSoft, marginTop: 4, lineHeight: 1.4 }}>Compra en segundos y sigue el envío en tiempo real.</div>
          <div style={{ marginTop: 10, display: "inline-block", background: SWATCHES[0].hex, color: "#fff", fontSize: 9, fontWeight: 650, padding: "6px 14px", borderRadius: 999 }}>Comprar ahora</div>
        </div>
        <div style={{ marginTop: 12, fontSize: 8, color: D.inkSoft, letterSpacing: "0.06em" }}>PALETA</div>
        <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
          {SWATCHES.map((s) => (
            <div key={s.hex} style={{ flex: 1 }}>
              <div style={{ height: 40, borderRadius: 8, background: s.hex, boxShadow: "inset 0 0 0 1px rgb(0 0 0 / 8%)" }} />
              <div style={{ fontSize: 7.5, color: D.ink, marginTop: 3, fontWeight: 600 }}>{s.name}</div>
              <div style={{ fontSize: 7, color: D.inkSoft, fontFamily: FONT_MONO }}>{s.hex}</div>
            </div>
          ))}
        </div>
      </div>
    </div>
  </AppWindow>
);
