import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Audio, Easing, interpolate, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { Keys } from "../lib/Caption";
import { AticMark } from "../lib/AticMark";
import { Icon, type IconName } from "../lib/Icon";
import { FPS } from "../Main";
import { C, EASE, FONT_MONO, FONT_SANS, PILL, clamp01 } from "../lib/theme";

/** La pill mide 124 × 40 px en la app; en el video se ve a 1.6×. */
const K = 1.6;
const TAB_W = PILL.islandLong;
const TAB_H = PILL.islandThick;
const STRIP_W = 304;
const PANEL_W = 440;
const PANEL_H = 460;
const FRANJA_H = 40;
const MID_X = 960;

export const GPUI_PILL_SECONDS = 34;
export const GPUI_PILL_FRAMES = Math.round(GPUI_PILL_SECONDS * FPS);

/** Hitos de la historia, en ms. */
const T = {
  hoverFrom: 4600,
  hoverTo: 5200,
  openFrom: 8000,
  openTo: 8500,
  closeFrom: 25800,
  closeTo: 26300,
  textosAt: 14200,
  appsAt: 20200,
  queryFrom: 20700,
  queryTo: 21500,
};

type Tool = "clipboard" | "textos" | "apps";

const TOOLS: { id: Tool; icon: IconName; placeholder: string }[] = [
  { id: "clipboard", icon: "clipboard", placeholder: "Buscar en el portapapeles" },
  { id: "textos", icon: "snippets", placeholder: "Buscar textos" },
  { id: "apps", icon: "search", placeholder: "Abrir una app o acción" },
];

const STRIP_ICONS: IconName[] = ["clipboard", "snippets", "search", "agents", "mic", "system"];

type ClipRow = { kind: "color" | "text" | "image"; text: string; meta: string };

const CLIP_FILTERS = ["Todo", "Textos", "Imágenes", "Colores", "Favoritos"];
const CLIP_ROWS: ClipRow[] = [
  { kind: "color", text: "#FF6B3D", meta: "10:42" },
  { kind: "text", text: "npm run build -- --filter web", meta: "10:38" },
  { kind: "text", text: "https://example.com/rendimiento", meta: "10:31" },
  { kind: "text", text: "sk-ab••••xyz", meta: "10:12" },
  { kind: "image", text: "Captura · 1280 × 720", meta: "Ayer" },
  { kind: "text", text: "Reunión mañana 9:30, sala 2", meta: "Ayer" },
];

const SNIPPET_ROWS = [
  { alias: "firma", name: "Firma de correo", body: "Saludos, {nombre}" },
  { alias: "fecha", name: "Fecha de hoy", body: "{fecha}" },
  { alias: "gracias", name: "Respuesta corta", body: "Gracias, lo reviso hoy y te aviso." },
  { alias: "enlace", name: "Estado de la pill", body: "https://example.com/estado" },
];

type AppRow = { name: string; kind: string; icon: IconName };
const APP_RECENT: AppRow[] = [
  { name: "Correo", kind: "Aplicación", icon: "window" },
  { name: "Configuración", kind: "Sistema", icon: "system" },
  { name: "Calculadora", kind: "Aplicación", icon: "calculator" },
];
const APP_RESULTS: AppRow[] = [
  { name: "Correo", kind: "Aplicación", icon: "window" },
  { name: "Copiar ruta", kind: "Acción", icon: "clipboard" },
];
const SEARCH_WORD = "cor";

const CAPTIONS: { from: number; to: number; title: string; sub: string; keys?: string[] }[] = [
  { from: 3200, to: 7600, title: "Una pill arriba, siempre a mano", sub: "Pasa el cursor sobre la marca y aparecen tus herramientas" },
  { from: 8600, to: 13600, title: "Clipboard", sub: "Lo que copiaste, con filtros y favoritos", keys: ["Ctrl", "Shift", "V"] },
  { from: 14600, to: 19600, title: "Textos", sub: "Snippets con {fecha} y {portapapeles}", keys: ["Ctrl", "Shift", "S"] },
  { from: 20600, to: 25600, title: "Apps", sub: "Escribe y abre lo que necesitas", keys: ["Ctrl", "Shift", "Espacio"] },
  { from: 26400, to: 29800, title: "Todo en un solo lugar", sub: "Sin ventanas nuevas" },
];

/** Cursor en pantalla: [ms, x, y]. Las posiciones salen de la geometría de la pill. */
const screenX = (designX: number, width: number) => MID_X + (designX - width / 2) * K;
const stripX = (i: number) => screenX(62 + 44 * i, STRIP_W);
const franjaX = (j: number) => screenX(PANEL_W - 22 - (2 - j) * 32, PANEL_W);
const ICON_Y = (TAB_H / 2) * K;
const BODY_Y = 300;
const CURSOR_KEYS: [number, number, number][] = [
  [0, 1500, 500],
  [2600, 1300, 420],
  [4200, MID_X, BODY_Y],
  [5600, stripX(0), ICON_Y],
  [7400, stripX(0), ICON_Y],
  [9400, MID_X, BODY_Y],
  [12900, MID_X, BODY_Y],
  [13600, franjaX(1), ICON_Y],
  [14000, franjaX(1), ICON_Y],
  [15500, MID_X, BODY_Y],
  [18900, MID_X, BODY_Y],
  [19600, franjaX(2), ICON_Y],
  [20000, franjaX(2), ICON_Y],
  [22500, MID_X, BODY_Y],
  [25000, 1500, 700],
  [25600, 1500, 700],
];

/** Progreso 0→1 entre dos instantes. */
const ramp = (ms: number, from: number, to: number, ease = EASE.smoothOut) =>
  ease(clamp01((ms - from) / (to - from)));

const linear = Easing.linear;

/** Aparece en `from`, se queda y se va en `to`, con `d` ms de fundido. */
const fade = (ms: number, from: number, to: number, d = 250) =>
  Math.min(ramp(ms, from, from + d, linear), 1 - ramp(ms, to - d, to, linear));

const activeTool = (ms: number): Tool => (ms < T.textosAt ? "clipboard" : ms < T.appsAt ? "textos" : "apps");

const typedQuery = (ms: number) =>
  SEARCH_WORD.slice(0, Math.round(ramp(ms, T.queryFrom, T.queryTo, linear) * SEARCH_WORD.length));

const chip = (active: boolean): CSSProperties => ({
  fontFamily: FONT_SANS,
  fontSize: 12,
  padding: "4px 10px",
  borderRadius: 999,
  color: active ? C.onAccent : C.muted,
  background: active ? C.accent : C.elevated,
});

const Body = ({ alpha, children }: { alpha: number; children: ReactNode }) =>
  alpha <= 0 ? null : <div style={{ position: "absolute", inset: 0, opacity: alpha }}>{children}</div>;

const ClipboardBody = () => (
  <>
    <div style={{ position: "absolute", left: 12, top: 8, display: "flex", gap: 6 }}>
      {CLIP_FILTERS.map((label, i) => (
        <span key={label} style={chip(i === 0)}>
          {label}
        </span>
      ))}
    </div>
    {CLIP_ROWS.map((row, i) => (
      <div
        key={row.text}
        style={{
          position: "absolute",
          left: 8,
          right: 8,
          top: 48 + i * 44,
          height: 42,
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "0 10px",
          borderRadius: 10,
          background: i === 0 ? "rgba(240, 240, 234, 0.08)" : "transparent",
        }}
      >
        {row.kind === "color" ? (
          <span style={{ width: 22, height: 22, borderRadius: 6, flex: "none", background: row.text }} />
        ) : row.kind === "image" ? (
          <span
            style={{
              width: 28,
              height: 22,
              borderRadius: 6,
              flex: "none",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: C.elevated,
              color: C.muted,
            }}
          >
            <Icon name="captures" size={14} />
          </span>
        ) : (
          <span style={{ width: 22, flex: "none" }} />
        )}
        <span
          style={{
            flex: 1,
            minWidth: 0,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            fontFamily: FONT_MONO,
            fontSize: 14,
            color: C.text,
          }}
        >
          {row.text}
        </span>
        <span style={{ fontFamily: FONT_SANS, fontSize: 12, color: C.faint }}>{row.meta}</span>
      </div>
    ))}
  </>
);

const TextosBody = () => (
  <>
    <div style={{ position: "absolute", left: 12, top: 8, display: "flex", gap: 6 }}>
      {["Textos", "Bloc", "Tablero"].map((label, i) => (
        <span key={label} style={chip(i === 0)}>
          {label}
        </span>
      ))}
    </div>
    {SNIPPET_ROWS.map((row, i) => (
      <div
        key={row.alias}
        style={{
          position: "absolute",
          left: 8,
          right: 8,
          top: 48 + i * 62,
          height: 56,
          padding: "0 12px",
          borderRadius: 10,
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          gap: 4,
          background: i === 0 ? "rgba(240, 240, 234, 0.08)" : "transparent",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span
            style={{
              fontFamily: FONT_MONO,
              fontSize: 11,
              color: C.muted,
              background: C.elevated,
              borderRadius: 6,
              padding: "2px 6px",
            }}
          >
            {row.alias}
          </span>
          <span style={{ fontFamily: FONT_SANS, fontSize: 15, fontWeight: 600, color: C.text }}>{row.name}</span>
        </div>
        <span
          style={{
            fontFamily: FONT_SANS,
            fontSize: 13,
            color: C.muted,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {row.body}
        </span>
      </div>
    ))}
  </>
);

const AppsBody = ({ query }: { query: string }) => {
  const rows = query ? APP_RESULTS : APP_RECENT;
  return (
    <>
      <div style={{ position: "absolute", left: 12, top: 8, fontFamily: FONT_SANS, fontSize: 12, color: C.faint }}>
        {query ? "Resultados" : "Recientes"}
      </div>
      {rows.map((row, i) => (
        <div
          key={row.name}
          style={{
            position: "absolute",
            left: 8,
            right: 8,
            top: 34 + i * 52,
            height: 46,
            padding: "0 12px",
            borderRadius: 10,
            display: "flex",
            alignItems: "center",
            gap: 12,
            background: i === 0 ? "rgba(240, 240, 234, 0.08)" : "transparent",
          }}
        >
          <span
            style={{
              width: 30,
              height: 30,
              borderRadius: 8,
              flex: "none",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: C.elevated,
              color: C.text,
            }}
          >
            <Icon name={row.icon} size={16} />
          </span>
          <span style={{ flex: 1, fontFamily: FONT_SANS, fontSize: 15, color: C.text }}>{row.name}</span>
          <span style={{ fontFamily: FONT_SANS, fontSize: 12, color: C.faint }}>{row.kind}</span>
        </div>
      ))}
    </>
  );
};

/** La pill: tab, franja de herramientas y panel, todo en un mismo objeto que cambia de tamaño. */
const Pill = ({ ms, lookX }: { ms: number; lookX: number }) => {
  const openT = ramp(ms, T.openFrom, T.openTo);
  const closeT = ramp(ms, T.closeFrom, T.closeTo);
  const panel = openT * (1 - closeT);
  const hover = ramp(ms, T.hoverFrom, T.hoverTo) * (1 - openT);
  const hoverW = TAB_W + (STRIP_W - TAB_W) * hover;
  const width = hoverW + (PANEL_W - hoverW) * panel;
  const height = TAB_H + (PANEL_H - TAB_H) * panel;
  const radius = 14 + 8 * panel;
  const markX = TAB_W / 2 + (18 - TAB_W / 2) * clamp01(hover + panel);
  const active = activeTool(ms);
  const query = active === "apps" ? typedQuery(ms) : "";
  const searchText = query || TOOLS.find((t) => t.id === active)?.placeholder || "";

  return (
    <div
      style={{
        position: "absolute",
        left: MID_X,
        top: 0,
        width: 0,
        height: 0,
        transform: `scale(${K})`,
        transformOrigin: "0 0",
      }}
    >
      <div
        style={{
          position: "absolute",
          left: -width / 2,
          top: 0,
          width,
          height,
          boxSizing: "border-box",
          borderRadius: `0 0 ${radius}px ${radius}px`,
          overflow: "hidden",
          background: "rgba(26, 26, 24, 0.82)",
          backdropFilter: "blur(18px) saturate(130%)",
          border: `1px solid ${C.line}`,
          boxShadow: "0 10px 26px rgba(0, 0, 0, 0.4)",
        }}
      >
        <div style={{ position: "absolute", left: markX - 11, top: 9 }}>
          <AticMark size={22} color={C.text} lookX={lookX} />
        </div>

        <div style={{ position: "absolute", left: 0, top: 0, width: STRIP_W, height: TAB_H, opacity: hover }}>
          {STRIP_ICONS.map((icon, i) => (
            <div
              key={icon}
              style={{
                position: "absolute",
                left: 40 + 44 * i,
                top: 0,
                width: 44,
                height: TAB_H,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: C.muted,
              }}
            >
              <Icon name={icon} size={20} />
            </div>
          ))}
        </div>

        <div style={{ position: "absolute", left: 0, top: 0, width: PANEL_W, height: FRANJA_H, opacity: panel }}>
          <div
            style={{
              position: "absolute",
              left: 40,
              right: 112,
              top: 6,
              height: 28,
              boxSizing: "border-box",
              padding: "0 10px",
              display: "flex",
              alignItems: "center",
              gap: 8,
              borderRadius: 10,
              background: C.surface2,
              boxShadow: `inset 0 0 0 1px ${C.line}`,
              fontFamily: FONT_SANS,
              fontSize: 14,
              color: query ? C.text : C.faint,
              whiteSpace: "nowrap",
              overflow: "hidden",
            }}
          >
            <Icon name="search" size={14} />
            {searchText}
          </div>
          {TOOLS.map((tool, j) => (
            <div
              key={tool.id}
              style={{
                position: "absolute",
                left: PANEL_W - 22 - (2 - j) * 32 - 14,
                top: 6,
                width: 28,
                height: 28,
                borderRadius: 8,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                color: tool.id === active ? C.text : C.muted,
                background: tool.id === active ? "rgba(240, 240, 234, 0.12)" : "transparent",
              }}
            >
              <Icon name={tool.icon} size={16} />
            </div>
          ))}
        </div>

        <div style={{ position: "absolute", left: 0, top: FRANJA_H, width: PANEL_W, height: PANEL_H - FRANJA_H, opacity: panel }}>
          <Body alpha={fade(ms, T.openFrom, T.textosAt)}>
            <ClipboardBody />
          </Body>
          <Body alpha={fade(ms, T.textosAt, T.appsAt)}>
            <TextosBody />
          </Body>
          <Body alpha={fade(ms, T.appsAt, T.closeFrom)}>
            <AppsBody query={query} />
          </Body>
        </div>
      </div>
    </div>
  );
};

const Desktop = () => (
  <AbsoluteFill style={{ background: "linear-gradient(160deg, #2a3b52 0%, #161d27 55%, #0e1218 100%)" }}>
    <MockWindow left={260} top={210} width={1160} height={640} accent="#2f80ed" />
    <MockWindow left={820} top={380} width={880} height={520} accent="#27ae60" />
  </AbsoluteFill>
);

/** Ventana de relleno para que el vidrio tenga algo detrás. */
const MockWindow = ({
  left,
  top,
  width,
  height,
  accent,
}: {
  left: number;
  top: number;
  width: number;
  height: number;
  accent: string;
}) => (
  <div
    style={{
      position: "absolute",
      left,
      top,
      width,
      height,
      borderRadius: 14,
      overflow: "hidden",
      background: "#1d2530",
      boxShadow: "0 30px 60px rgba(0, 0, 0, 0.45)",
    }}
  >
    <div style={{ height: 34, background: "#252f3d", display: "flex", alignItems: "center", gap: 8, padding: "0 14px" }}>
      {[0, 1, 2].map((i) => (
        <span key={i} style={{ width: 10, height: 10, borderRadius: 5, background: "rgba(240, 240, 234, 0.25)" }} />
      ))}
    </div>
    <div style={{ position: "absolute", left: 28, top: 70, bottom: 28, width: 6, borderRadius: 3, background: accent, opacity: 0.8 }} />
    {Array.from({ length: 9 }, (_, i) => (
      <div
        key={i}
        style={{
          position: "absolute",
          left: 60,
          top: 72 + i * 48,
          width: `${40 + ((i * 37) % 45)}%`,
          height: 12,
          borderRadius: 6,
          background: "rgba(240, 240, 234, 0.1)",
        }}
      />
    ))}
  </div>
);

const Cursor = ({ x, y, alpha }: { x: number; y: number; alpha: number }) => (
  <svg
    width={34}
    height={34}
    viewBox="0 0 24 24"
    style={{ position: "absolute", left: x, top: y, opacity: alpha, filter: "drop-shadow(0 2px 3px rgba(0, 0, 0, 0.5))" }}
  >
    <path d="M4 3l15 8.5-6.6 1.6L9.6 20 4 3z" fill="#fff" stroke="#111" strokeWidth={1.2} strokeLinejoin="round" />
  </svg>
);

const CaptionLine = ({ ms, title, sub, keys, from, to }: (typeof CAPTIONS)[number] & { ms: number }) => {
  const a = fade(ms, from, to, 350);
  return (
    <div
      style={{
        position: "absolute",
        left: 120,
        right: 120,
        bottom: 110,
        fontFamily: FONT_SANS,
        color: C.text,
        opacity: a,
        transform: `translateY(${(1 - a) * 18}px)`,
      }}
    >
      <div style={{ fontSize: 64, fontWeight: 650, letterSpacing: "-0.03em", lineHeight: 1 }}>{title}</div>
      <div style={{ display: "flex", alignItems: "center", gap: 18, marginTop: 18 }}>
        {keys && <Keys keys={keys} size={0.8} />}
        <span style={{ fontSize: 30, color: C.muted }}>{sub}</span>
      </div>
    </div>
  );
};

const BrandCard = ({ alpha, sub }: { alpha: number; sub: string }) =>
  alpha <= 0 ? null : (
    <AbsoluteFill
      style={{
        alignItems: "center",
        justifyContent: "center",
        opacity: alpha,
        fontFamily: FONT_SANS,
        color: C.text,
      }}
    >
      <AticMark size={120} color={C.text} />
      <div style={{ marginTop: 36, fontSize: 120, fontWeight: 650, letterSpacing: "-0.04em", lineHeight: 1 }}>Atic</div>
      <div style={{ marginTop: 28, fontSize: 44, color: C.muted }}>{sub}</div>
    </AbsoluteFill>
  );

/** Demo de la pill GPUI. La música es opcional: pasa `musicSrc` (archivo en public/) al renderizar. */
export const GpuiPillVideo = ({ musicSrc }: { musicSrc?: string }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const ms = (frame / fps) * 1000;

  const cursorX = interpolate(
    ms,
    CURSOR_KEYS.map((k) => k[0]),
    CURSOR_KEYS.map((k) => k[1]),
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" },
  );
  const cursorY = interpolate(
    ms,
    CURSOR_KEYS.map((k) => k[0]),
    CURSOR_KEYS.map((k) => k[2]),
    { extrapolateLeft: "clamp", extrapolateRight: "clamp" },
  );
  const cursorAlpha = Math.min(ramp(ms, 2800, 3200, linear), 1 - ramp(ms, 29600, 29900, linear));
  const lookX = clamp01(0.5 + (cursorX - MID_X) / 400) * 2 - 1;

  const coverAlpha = Math.max(1 - ramp(ms, 2800, 3300, linear), ramp(ms, 30000, 30500, linear));

  return (
    <AbsoluteFill style={{ background: C.bg }}>
      <Desktop />
      <Pill ms={ms} lookX={lookX} />
      {CAPTIONS.map((c) => (
        <CaptionLine key={c.title} ms={ms} {...c} />
      ))}
      <Cursor x={cursorX} y={cursorY} alpha={cursorAlpha} />
      <AbsoluteFill style={{ background: C.bg, opacity: coverAlpha }} />
      <BrandCard alpha={fade(ms, 0, 2700, 300)} sub="La pill, ahora en GPUI" />
      <BrandCard alpha={ramp(ms, 30300, 30800, linear)} sub="Prototipo para Windows" />
      {musicSrc ? <Audio src={staticFile(musicSrc)} volume={0.9} /> : null}
    </AbsoluteFill>
  );
};
