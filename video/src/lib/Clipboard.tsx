import type { ReactNode } from "react";
import { Easing } from "remotion";
import { CbIcon } from "./clipboardIcons";
import { C, EASE, FONT_SANS, lerp } from "./theme";
import { seg } from "./time";

/**
 * Float del historial de clipboard (ClipboardFloat.svelte + ClipboardHistoryList.svelte).
 * Coordenadas del panel: (0,0) es su esquina superior izquierda, 312 x 372.
 * La paleta del clipboard es la vieja `--rb-*` (ficha B.3), distinta de la del launcher.
 */
const RB = {
  text: "#f0f0ea",
  muted: "#9a9a90",
  accent: "#f0f0ea",
};

export const CB = {
  w: 312,
  h: 372,
  radius: 18,
  rowH: 44,
  rowStride: 46,
  listX: 8,
  listY: 7.2 + 24 + 3.2 + 25.6 + 4.8, // 64.8
  listW: 296,
  listH: 372 - (7.2 + 24 + 3.2 + 25.6 + 4.8) - 8.8, // 298.4
  actionW: 22.4,
  actionH: 21.2,
} as const;

/** Punto donde queda el panel en pantalla: centrado bajo la pill, 16 px bajo su borde (48). */
export const PANEL_PILL_BOTTOM = 48;
export const panelOrigin = (cx: number) => ({
  x: cx - CB.w / 2,
  y: PANEL_PILL_BOTTOM + 16,
});

/** Instantes (ms de la escena) de cada gesto. */
export const CB_T = {
  hoverCellMs: 650,
  clickCellMs: 1050,
  openMs: 1100,
  stripCloseMs: 1550,
  readyMs: 1500,
  copyMs: 1800,
  searchClickMs: 2350,
  typeMs: 2450,
  typeStepMs: 90,
  chipHoverMs: 3500,
  pinClickMs: 4300,
  clearMs: 4550,
  clearStepMs: 55,
  pasteClickMs: 5050,
} as const;

const QUERY = "imagen";

/* ------------------------------ datos de ejemplo ------------------------------ */

type Kind = "text" | "image" | "color";
type Item = {
  id: string;
  kind: Kind;
  preview: string;
  when: string;
  source?: string;
  hex?: string;
  art?: "shot" | "warm";
};

/** Orden = más reciente primero. `n0` es lo que se «acaba de copiar». */
const ITEMS: Item[] = [
  { id: "n0", kind: "text", preview: "Enviar la propuesta revisada al equipo antes del viernes", when: "14:36" },
  { id: "i1", kind: "image", preview: "Imagen", when: "14:32", source: "Captura", art: "shot" },
  { id: "c1", kind: "color", preview: "#4F8EF7", when: "14:20", hex: "#4F8EF7" },
  { id: "t1", kind: "text", preview: "Adjuntar imagen del informe en la presentación", when: "14:05" },
  { id: "t2", kind: "text", preview: "https://ejemplo.com/docs/guia-de-inicio", when: "13:48" },
  { id: "t3", kind: "text", preview: "Cotización 2025-084 aprobada por finanzas", when: "11:52" },
  { id: "i2", kind: "image", preview: "Imagen", when: "Ayer · 18:05", art: "warm" },
  { id: "t4", kind: "text", preview: "Confirmo asistencia a la reunión del lunes", when: "Ayer · 16:40" },
];

const KIND_LABEL: Record<Kind, string> = { text: "texto", image: "imagen", color: "color" };

/* ------------------------- búsqueda (lib/core/clipboardSearch.ts) ------------------------- */

const normalize = (s: string) =>
  s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").trim();

/** Coincide por substring o porque están TODAS las palabras (sin fuzzy). */
const matches = (item: Item, query: string) => {
  const q = normalize(query);
  if (!q) return true;
  const hay = normalize(item.preview);
  return hay.includes(q) || q.split(/\s+/).every((w) => hay.includes(w));
};

/** Fijados primero, luego cronológico (clipboard.svelte.ts:29). */
const arrange = (query: string, pinned: boolean, isNew: boolean) => {
  const base = ITEMS.filter((i) => (isNew || i.id !== "n0") && matches(i, query));
  const isPinned = (i: Item) => pinned && i.id === "t1";
  return [...base.filter(isPinned), ...base.filter((i) => !isPinned(i))].map((i) => i.id);
};

/* ------------------------------ línea de tiempo del estado ------------------------------ */

type Snap = { t: number; query: string; pinned: boolean; ids: string[]; durMs: number };

const SNAPS: Snap[] = (() => {
  const out: Snap[] = [];
  const push = (t: number, query: string, pinned: boolean, isNew: boolean, durMs: number) =>
    out.push({ t, query, pinned, ids: arrange(query, pinned, isNew), durMs });
  push(0, "", false, false, 0);
  push(CB_T.copyMs, "", false, true, 220);
  for (let k = 1; k <= QUERY.length; k++)
    push(CB_T.typeMs + (k - 1) * CB_T.typeStepMs, QUERY.slice(0, k), false, true, 0);
  push(CB_T.pinClickMs, QUERY, true, true, 0);
  for (let j = 0; j <= QUERY.length - 1; j++)
    push(CB_T.clearMs + j * CB_T.clearStepMs, QUERY.slice(0, QUERY.length - 1 - j), true, true, 0);
  return out;
})();

type RowLayout = { item: Item; y: number; opacity: number };

const layoutAt = (ms: number): { rows: RowLayout[]; snap: Snap } => {
  let k = 0;
  for (let i = 0; i < SNAPS.length; i++) if (SNAPS[i].t <= ms) k = i;
  const cur = SNAPS[k];
  const prev = k > 0 ? SNAPS[k - 1] : null;
  const p = cur.durMs > 0 ? EASE.smoothOut(seg(ms, cur.t, cur.durMs)) : 1;
  const rows: RowLayout[] = [];
  cur.ids.forEach((id, idx) => {
    const item = ITEMS.find((i) => i.id === id)!;
    let y = idx * CB.rowStride;
    let opacity = 1;
    if (prev && cur.durMs > 0) {
      const pi = prev.ids.indexOf(id);
      if (pi >= 0) y = lerp(pi * CB.rowStride, y, p);
      else opacity = p;
    }
    rows.push({ item, y, opacity });
  });
  return { rows, snap: cur };
};

/* ------------------------------ cursor ------------------------------ */

/** Waypoints del puntero en coordenadas lógicas de la pantalla (cx = 250). */
const PATH: { t: number; x: number; y: number }[] = [
  { t: 0, x: 372, y: 96 },
  { t: 250, x: 372, y: 96 },
  { t: 650, x: 135, y: 27 },
  { t: 1120, x: 135, y: 27 },
  { t: 1750, x: 172, y: 118 },
  { t: 2250, x: 192, y: 112 },
  { t: 2600, x: 192, y: 112 },
  { t: 3100, x: 205, y: 152 },
  { t: 3400, x: 205, y: 152 },
  { t: 3620, x: 206, y: 160 },
  { t: 3850, x: 206, y: 160 },
  { t: 4260, x: 384, y: 185.5 },
  { t: 4700, x: 384, y: 185.5 },
  { t: 5000, x: 215, y: 289 },
  { t: 6000, x: 215, y: 289 },
];

const MOUSE = Easing.bezier(0.45, 0, 0.2, 1);

export const cursorAt = (ms: number) => {
  let k = 0;
  for (let i = 0; i < PATH.length - 1; i++) if (PATH[i].t <= ms) k = i;
  const a = PATH[k];
  const b = PATH[Math.min(k + 1, PATH.length - 1)];
  const e = MOUSE(seg(ms, a.t, Math.max(1, b.t - a.t)));
  return { x: lerp(a.x, b.x, e), y: lerp(a.y, b.y, e) };
};

export const CURSOR_CLICKS = [
  CB_T.clickCellMs,
  CB_T.searchClickMs,
  CB_T.pinClickMs,
  CB_T.pasteClickMs,
];

/* ------------------------------ piezas ------------------------------ */

const IconButton = ({ children, on = false, hover = false, size, radius }: { children: ReactNode; on?: boolean; hover?: boolean; size: number; radius: number }) => (
  <div
    style={{
      width: size,
      height: size,
      borderRadius: radius,
      display: "grid",
      placeItems: "center",
      flex: "none",
      color: on ? RB.accent : hover ? RB.text : C.faint,
      background: on ? "rgba(240,240,234,.14)" : hover ? "rgba(240,240,234,.08)" : "transparent",
    }}
  >
    {children}
  </div>
);

const Header = () => (
  <div
    style={{
      position: "relative",
      display: "flex",
      justifyContent: "flex-end",
      alignItems: "center",
      gap: 8,
      minHeight: 24,
      marginBottom: 3.2,
    }}
  >
    <i
      style={{
        position: "absolute",
        left: "50%",
        top: "50%",
        width: 32,
        height: 3,
        marginLeft: -16,
        marginTop: -1.5,
        borderRadius: 999,
        background: "#4d4d4a",
      }}
    />
    <div style={{ display: "flex", gap: 2.4, alignItems: "center", height: 24 }}>
      <IconButton size={28} radius={6.4}>
        <CbIcon name="pin" size={13} />
      </IconButton>
      <IconButton size={28} radius={6.4}>
        <CbIcon name="panelTopClose" size={14} />
      </IconButton>
      <IconButton size={28} radius={6.4}>
        <CbIcon name="x" size={14} />
      </IconButton>
    </div>
  </div>
);

const Toolbar = ({ query, focused, ms }: { query: string; focused: boolean; ms: number }) => (
  <div style={{ display: "flex", alignItems: "center", gap: 4.8, padding: "0 3.2px", height: 25.6 }}>
    <div
      style={{
        position: "relative",
        flex: 1,
        height: 25.6,
        borderRadius: 999,
        background: "rgba(240,240,234,.07)",
        boxShadow: focused ? "inset 0 0 0 1.5px rgba(240,240,234,.7)" : "none",
        display: "flex",
        alignItems: "center",
        padding: "0 6.4px 0 22.4px",
        fontSize: 10,
        fontWeight: 500,
        boxSizing: "border-box",
      }}
    >
      <span style={{ position: "absolute", left: 6.4, top: "50%", marginTop: -6, color: RB.muted }}>
        <CbIcon name="search" size={12} />
      </span>
      {query ? (
        <span style={{ color: RB.text, whiteSpace: "nowrap" }}>
          {query}
          {focused && (
            <span
              style={{
                display: "inline-block",
                width: 1,
                height: 11,
                background: RB.text,
                marginLeft: 1,
                verticalAlign: "middle",
                opacity: Math.floor(ms / 500) % 2 ? 0 : 1,
              }}
            />
          )}
        </span>
      ) : focused ? (
        <span style={{ display: "inline-flex", alignItems: "center" }}>
          <span
            style={{
              display: "inline-block",
              width: 1,
              height: 11,
              background: RB.text,
              opacity: Math.floor(ms / 500) % 2 ? 0 : 1,
              marginRight: 1,
            }}
          />
          <span style={{ color: RB.muted }}>Buscar…</span>
        </span>
      ) : (
        <span style={{ color: RB.muted }}>Buscar…</span>
      )}
    </div>
    <div style={{ display: "flex", alignItems: "center", gap: 2.4 }}>
      <div style={{ display: "flex", gap: 2.4 }}>
        <IconButton size={25.6} radius={999} on>
          <CbIcon name="layers" size={12} />
        </IconButton>
        <IconButton size={25.6} radius={999}>
          <CbIcon name="type" size={12} />
        </IconButton>
        <IconButton size={25.6} radius={999}>
          <CbIcon name="image" size={12} />
        </IconButton>
      </div>
      <IconButton size={25.6} radius={999}>
        <CbIcon name="star" size={14} />
      </IconButton>
    </div>
  </div>
);

/** Miniaturas de imagen abstractas (44 x 34). */
const ImageArt = ({ art }: { art: "shot" | "warm" }) => (
  <svg width={44} height={34} viewBox="0 0 44 34" style={{ display: "block" }}>
    {art === "shot" ? (
      <>
        <defs>
          <linearGradient id="cbShot" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#233247" />
            <stop offset="1" stopColor="#4d6f96" />
          </linearGradient>
        </defs>
        <rect width="44" height="34" fill="url(#cbShot)" />
        <rect x="5" y="5" width="34" height="24" rx="2.5" fill="#e9edf1" opacity=".92" />
        <rect x="5" y="5" width="34" height="5" rx="2.5" fill="#c6d0da" />
        <rect x="9" y="14" width="14" height="2.4" rx="1.2" fill="#7f93a8" />
        <rect x="9" y="19" width="22" height="2.4" rx="1.2" fill="#a9b7c5" />
        <rect x="9" y="24" width="10" height="2.4" rx="1.2" fill="#a9b7c5" />
        <circle cx="32" cy="17.5" r="3.6" fill="#4f8ef7" />
      </>
    ) : (
      <>
        <defs>
          <linearGradient id="cbWarm" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#f2c48a" />
            <stop offset="1" stopColor="#d9728a" />
          </linearGradient>
        </defs>
        <rect width="44" height="34" fill="url(#cbWarm)" />
        <circle cx="32" cy="11" r="5" fill="#fbe8c8" opacity=".95" />
        <path d="M0 34V24l10-8 9 7 8-6 17 9v8Z" fill="#7a3f5c" opacity=".85" />
        <path d="M0 34v-6l12-6 10 6 10-4 12 5v5Z" fill="#4a2a45" />
      </>
    )}
  </svg>
);

const Thumb = ({ item }: { item: Item }) => {
  if (item.kind === "image")
    return (
      <span
        style={{
          position: "relative",
          width: 44,
          height: 34,
          borderRadius: 6,
          overflow: "hidden",
          flex: "none",
          outline: "1px solid rgba(255,255,255,.10)",
          outlineOffset: -1,
        }}
      >
        <ImageArt art={item.art ?? "shot"} />
        <span style={{ position: "absolute", inset: 0, borderRadius: 6, boxShadow: "inset 0 0 0 1px rgba(255,255,255,.10)" }} />
      </span>
    );
  if (item.kind === "color")
    return (
      <span
        style={{
          width: 22,
          height: 22,
          borderRadius: 6,
          flex: "none",
          background: item.hex,
          boxShadow: "inset 0 0 0 1px rgba(255,255,255,.22), inset 0 0 0 1px rgba(0,0,0,.18)",
        }}
      />
    );
  return (
    <span
      style={{
        width: 22,
        height: 22,
        borderRadius: 6,
        flex: "none",
        background: "rgba(240,240,234,.08)",
        color: RB.muted,
        display: "grid",
        placeItems: "center",
      }}
    >
      <CbIcon name="type" size={13} />
    </span>
  );
};

const CHIPS: { icon: "pencil" | "externalLink" | "scanText"; label: string }[] = [
  { icon: "pencil", label: "Dibujar" },
  { icon: "externalLink", label: "Abrir" },
  { icon: "scanText", label: "Texto" },
];

const Row = ({
  item,
  y,
  opacity,
  hover,
  quick,
  chipHover,
  starHover,
  pinned,
  flash,
}: {
  item: Item;
  y: number;
  opacity: number;
  hover: number;
  quick: number;
  chipHover: number;
  starHover: number;
  pinned: boolean;
  flash: number;
}) => {
  const meta =
    `${KIND_LABEL[item.kind]} · ${item.when}` + (item.source ? ` · ${item.source}` : "");
  return (
    <div
      style={{
        position: "absolute",
        left: 2.4,
        right: 2.4,
        top: y,
        height: CB.rowH,
        display: "flex",
        alignItems: "stretch",
        gap: 3.2,
        opacity,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          flex: 1,
          minWidth: 0,
          display: "flex",
          alignItems: "center",
          gap: 6.4,
          padding: "3.2px 6.4px",
          borderRadius: 12,
          background: `rgba(240,240,234,${0.08 * hover + 0.1 * flash})`,
        }}
      >
        <Thumb item={item} />
        <span style={{ display: "flex", flexDirection: "column", minWidth: 0, flex: 1 }}>
          <span
            style={{
              fontSize: 11,
              fontWeight: 500,
              lineHeight: 1.25,
              color: RB.text,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {item.preview}
          </span>
          <span style={{ position: "relative", minHeight: 20, display: "flex", alignItems: "center" }}>
            <span
              style={{
                fontSize: 9,
                color: RB.muted,
                fontVariantNumeric: "tabular-nums",
                whiteSpace: "nowrap",
                opacity: item.kind === "image" ? 1 - quick : 1,
              }}
            >
              {meta}
            </span>
            {item.kind === "image" && (
              <span
                style={{
                  position: "absolute",
                  left: 0,
                  top: "50%",
                  transform: "translateY(-50%)",
                  display: "flex",
                  gap: 3,
                  opacity: quick,
                }}
              >
                {CHIPS.map((chip, i) => {
                  const hot = i === 0 ? chipHover : 0;
                  return (
                    <span
                      key={chip.label}
                      style={{
                        height: 20,
                        padding: "0 5.1px",
                        borderRadius: 5.6,
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 2.9,
                        fontSize: 9.3,
                        fontWeight: 650,
                        color: hot > 0.5 ? RB.text : RB.muted,
                        background: `rgba(240,240,234,${hot > 0.5 ? 0.14 : 0.08})`,
                        whiteSpace: "nowrap",
                      }}
                    >
                      <CbIcon name={chip.icon} size={11} />
                      {chip.label}
                    </span>
                  );
                })}
              </span>
            )}
          </span>
        </span>
      </div>
      <div
        style={{
          width: CB.actionW,
          flex: "none",
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          gap: 1.6,
        }}
      >
        <div
          style={{
            width: CB.actionW,
            height: CB.actionH,
            borderRadius: 6,
            display: "grid",
            placeItems: "center",
            color: pinned ? RB.accent : starHover > 0.5 ? RB.text : RB.muted,
            background: starHover > 0.5 ? "rgba(240,240,234,.08)" : "transparent",
          }}
        >
          <CbIcon name="star" size={14} filled={pinned} />
        </div>
        <div
          style={{
            width: CB.actionW,
            height: CB.actionH,
            borderRadius: 6,
            display: "grid",
            placeItems: "center",
            color: RB.muted,
          }}
        >
          <CbIcon name="x" size={14} />
        </div>
      </div>
    </div>
  );
};

/* ------------------------------ panel ------------------------------ */

/** Promedia el «cursor encima» en los últimos 75 ms (fundido de hover de 75 ms). */
const smoothed = (ms: number, inside: (p: { x: number; y: number }) => boolean) => {
  let n = 0;
  for (let k = 0; k < 4; k++) if (inside(cursorAt(ms - k * 25))) n++;
  return n / 4;
};

/** Contenido del float (312 x 372). El fondo y la silueta los pone FloatFromPill. */
export const ClipboardPanel = ({ ms, cx }: { ms: number; cx: number }) => {
  const o = panelOrigin(cx);
  const { rows, snap } = layoutAt(ms);
  const interactive = ms >= CB_T.readyMs;
  const focused = ms >= CB_T.searchClickMs;

  const itemX0 = CB.listX + 2.4;
  const itemX1 = itemX0 + (CB.listW - 4.8 - CB.actionW - 3.2);
  const starX0 = CB.listX + CB.listW - 2.4 - CB.actionW;
  const starX1 = starX0 + CB.actionW;

  const insideRow = (r: RowLayout, part: "item" | "star") => (p: { x: number; y: number }) => {
    const lx = p.x - o.x;
    const ly = p.y - o.y - CB.listY - r.y;
    if (ly < 0 || ly > CB.rowH) return false;
    if (p.y - o.y < CB.listY || p.y - o.y > CB.listY + CB.listH) return false;
    return part === "item"
      ? lx >= itemX0 && lx <= itemX1
      : lx >= starX0 && lx <= starX1 && ly <= CB.actionH;
  };
  const insideChip = (r: RowLayout) => (p: { x: number; y: number }) => {
    const lx = p.x - o.x;
    const ly = p.y - o.y - CB.listY - r.y;
    return lx >= 67 && lx <= 125 && ly >= 19 && ly <= 39;
  };

  const flashT = seg(ms, CB_T.pasteClickMs, 320);
  const flash = flashT > 0 && flashT < 1 ? Math.sin(flashT * Math.PI) : 0;

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: CB.w,
        height: CB.h,
        boxSizing: "border-box",
        padding: "7.2px 8px 8.8px",
        fontFamily: FONT_SANS,
        color: C.text,
        display: "flex",
        flexDirection: "column",
        gap: 0,
      }}
    >
      <Header />
      <Toolbar query={snap.query} focused={focused} ms={ms} />
      <div
        style={{
          position: "relative",
          marginTop: 4.8,
          flex: 1,
          minHeight: 0,
          overflow: "hidden",
        }}
      >
        {rows.map((r) => {
          const hover = interactive ? smoothed(ms, insideRow(r, "item")) : 0;
          const starOver = interactive ? smoothed(ms, insideRow(r, "star")) : 0;
          const chip = interactive ? smoothed(ms, insideChip(r)) : 0;
          return (
            <Row
              key={r.item.id}
              item={r.item}
              y={r.y}
              opacity={r.opacity}
              hover={Math.max(hover, chip)}
              quick={hover}
              chipHover={chip}
              starHover={starOver}
              pinned={snap.pinned && r.item.id === "t1"}
              flash={r.item.id === "c1" ? flash : 0}
            />
          );
        })}
      </div>
    </div>
  );
};
