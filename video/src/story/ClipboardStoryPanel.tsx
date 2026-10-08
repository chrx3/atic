import type { ReactNode } from "react";
import { CB, panelOrigin } from "../lib/Clipboard";
import { CbIcon } from "../lib/clipboardIcons";
import { C, FONT_SANS } from "../lib/theme";
import { seg } from "../lib/time";

/**
 * Float del historial de clipboard con los datos de la historia (error de la terminal,
 * URL del panel, IP de la base, captura del gráfico...). Es una copia adaptada de
 * `lib/Clipboard.tsx`: mismas medidas, colores y piezas; solo cambian los datos, el
 * texto buscado y el guion (sin animación de «copia nueva», que aquí ya viene copiada).
 * Coordenadas del panel: (0,0) es su esquina superior izquierda, 312 x 372.
 */
const RB = {
  text: "#f0f0ea",
  muted: "#9a9a90",
  accent: "#f0f0ea",
};

/** Texto que se escribe en el buscador. */
export const STORY_QUERY = "error";

/** Instantes (ms de la escena) que el panel necesita para su estado. */
export type ClipboardStoryTimeline = {
  /** El float terminó de abrirse: desde aquí responde al puntero. */
  readyMs: number;
  /** Destello suave sobre la fila recién copiada. */
  pulseMs: number;
  searchClickMs: number;
  typeMs: number;
  typeStepMs: number;
  /** Clic en la estrella de la fila del error. */
  pinClickMs: number;
};

export type Pointer = { x: number; y: number };

/* ------------------------------ datos de la historia ------------------------------ */

type Kind = "text" | "image" | "color";
type Item = {
  id: string;
  kind: Kind;
  preview: string;
  when: string;
  source?: string;
  hex?: string;
};

/** Orden = más reciente primero. `err` es lo que el usuario acaba de copiar de la terminal. */
const ITEMS: Item[] = [
  { id: "err", kind: "text", preview: "Error: connect ETIMEDOUT 10.0.4.12:5432 at pool.query (src/db.ts:88:14)", when: "14:32" },
  { id: "url", kind: "text", preview: "https://panel.tienda.cl/rendimiento", when: "14:27" },
  { id: "ip", kind: "text", preview: "10.0.4.12", when: "14:24" },
  { id: "shot", kind: "image", preview: "Imagen", when: "14:21", source: "Captura" },
  { id: "sql", kind: "text", preview: "SELECT * FROM orders WHERE user = $1", when: "13:58" },
  { id: "hex", kind: "color", preview: "#FF6B3D", when: "13:41", hex: "#FF6B3D" },
];

const PINNED_ID = "err";

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
const arrange = (query: string, pinned: boolean) => {
  const base = ITEMS.filter((i) => matches(i, query));
  const isPinned = (i: Item) => pinned && i.id === PINNED_ID;
  return [...base.filter(isPinned), ...base.filter((i) => !isPinned(i))];
};

const stateAt = (ms: number, tl: ClipboardStoryTimeline) => {
  const typedCount =
    ms < tl.typeMs
      ? 0
      : Math.min(STORY_QUERY.length, Math.floor((ms - tl.typeMs) / tl.typeStepMs) + 1);
  const query = STORY_QUERY.slice(0, typedCount);
  const pinned = ms >= tl.pinClickMs;
  return { query, pinned, items: arrange(query, pinned) };
};

/* ------------------------------ piezas ------------------------------ */

const IconButton = ({ children, on = false, size, radius }: { children: ReactNode; on?: boolean; size: number; radius: number }) => (
  <div
    style={{
      width: size,
      height: size,
      borderRadius: radius,
      display: "grid",
      placeItems: "center",
      flex: "none",
      color: on ? RB.accent : C.faint,
      background: on ? "rgba(240,240,234,.14)" : "transparent",
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

/** Miniatura de la captura del gráfico de rendimiento (44 x 34). */
const ChartShot = () => (
  <svg width={44} height={34} viewBox="0 0 44 34" style={{ display: "block" }}>
    <rect width="44" height="34" fill="#f4f6f9" />
    <rect width="44" height="5" fill="#dfe4ec" />
    {[13, 20, 27].map((y) => (
      <line key={y} x1="3" x2="41" y1={y} y2={y} stroke="#d5dbe5" strokeWidth="0.7" />
    ))}
    <path
      d="M3 25 L8 24 L13 25 L18 23.5 L23 24.5 L28 8 L33 9 L38 20 L41 25"
      fill="none"
      stroke="#2f6feb"
      strokeWidth="1.5"
      strokeLinejoin="round"
      strokeLinecap="round"
    />
    <circle cx="28" cy="8" r="2.2" fill="#e5484d" />
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
        <ChartShot />
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
  hover,
  quick,
  chipHover,
  starHover,
  pinned,
  flash,
}: {
  item: Item;
  y: number;
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
const smoothed = (ms: number, pointerAt: (ms: number) => Pointer, inside: (p: Pointer) => boolean) => {
  let n = 0;
  for (let k = 0; k < 4; k++) if (inside(pointerAt(ms - k * 25))) n++;
  return n / 4;
};

/** Centro de la estrella de la fila `index` en coordenadas de pantalla (para el cursor). */
export const starCenter = (cx: number, index: number) => {
  const o = panelOrigin(cx);
  const starX0 = CB.listX + CB.listW - 2.4 - CB.actionW;
  return { x: o.x + starX0 + CB.actionW / 2, y: o.y + CB.listY + index * CB.rowStride + CB.actionH / 2 };
};

/** Centro del buscador en coordenadas de pantalla (para el cursor). */
export const searchCenter = (cx: number) => {
  const o = panelOrigin(cx);
  return { x: o.x + 98, y: o.y + 7.2 + 24 + 3.2 + 12.8 };
};

/** Contenido del float (312 x 372). El fondo y la silueta los pone FloatFromPill. */
export const ClipboardStoryPanel = ({
  ms,
  cx,
  tl,
  pointerAt,
}: {
  ms: number;
  cx: number;
  tl: ClipboardStoryTimeline;
  pointerAt: (ms: number) => Pointer;
}) => {
  const o = panelOrigin(cx);
  const { query, pinned, items } = stateAt(ms, tl);
  const interactive = ms >= tl.readyMs;
  const focused = ms >= tl.searchClickMs;

  const itemX0 = CB.listX + 2.4;
  const itemX1 = itemX0 + (CB.listW - 4.8 - CB.actionW - 3.2);
  const starX0 = CB.listX + CB.listW - 2.4 - CB.actionW;
  const starX1 = starX0 + CB.actionW;

  const insideRow = (rowY: number, part: "item" | "star") => (p: Pointer) => {
    const lx = p.x - o.x;
    const ly = p.y - o.y - CB.listY - rowY;
    if (ly < 0 || ly > CB.rowH) return false;
    if (p.y - o.y < CB.listY || p.y - o.y > CB.listY + CB.listH) return false;
    return part === "item"
      ? lx >= itemX0 && lx <= itemX1
      : lx >= starX0 && lx <= starX1 && ly <= CB.actionH;
  };
  const insideChip = (rowY: number) => (p: Pointer) => {
    const lx = p.x - o.x;
    const ly = p.y - o.y - CB.listY - rowY;
    return lx >= 67 && lx <= 125 && ly >= 19 && ly <= 39;
  };

  const pulseT = seg(ms, tl.pulseMs, 700);
  const pulse = pulseT > 0 && pulseT < 1 ? Math.sin(pulseT * Math.PI) : 0;

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
      <Toolbar query={query} focused={focused} ms={ms} />
      <div
        style={{
          position: "relative",
          marginTop: 4.8,
          flex: 1,
          minHeight: 0,
          overflow: "hidden",
        }}
      >
        {items.map((item, idx) => {
          const y = idx * CB.rowStride;
          const hover = interactive ? smoothed(ms, pointerAt, insideRow(y, "item")) : 0;
          const starOver = interactive ? smoothed(ms, pointerAt, insideRow(y, "star")) : 0;
          const chip = interactive ? smoothed(ms, pointerAt, insideChip(y)) : 0;
          return (
            <Row
              key={item.id}
              item={item}
              y={y}
              hover={Math.max(hover, chip)}
              quick={hover}
              chipHover={chip}
              starHover={starOver}
              pinned={pinned && item.id === PINNED_ID}
              flash={item.id === PINNED_ID ? pulse : 0}
            />
          );
        })}
      </div>
    </div>
  );
};
