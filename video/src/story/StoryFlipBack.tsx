import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Easing } from "remotion";
import { FlipCursor, type CursorKey } from "../lib/FlipCursor";
import { TIcon, type TIconName } from "../lib/textosIcons";
import { EASE, FONT_MONO, FONT_SANS } from "../lib/theme";
import { seg } from "../lib/time";
import { typed } from "../lib/ui";
import { FLIP_WINDOW_TITLE } from "./StoryFlipFront";

/**
 * Copia adaptada de `lib/FlipBack.tsx` para la historia. Mismo tablero (barra, papel punteado,
 * cajón Insertar, tira de páginas); cambian los datos: dos notas de texto («Causa» y «Fix»),
 * un trazo de lápiz sobre la causa y, desde el cajón (pestaña Clip), el error copiado como tarjeta.
 * La tarjeta es más alta que la del ejemplo porque la ventana del navegador es casi cuadrada.
 */
export const STORY_CARD = { w: 1280, h: 1138 } as const;
const CARD = STORY_CARD;
const BAR_H = 44;
const TIRA_H = 79;
const VISTA_H = CARD.h - BAR_H - TIRA_H;
const PAPER = { w: 1200, h: 900 } as const;
/** zoom = min(1, (vw-24)/1200, (vh-24)/900). */
const ZOOM = Math.min(1, (1256 - 24) / PAPER.w, (VISTA_H - 24) / PAPER.h);
const DRAWER_W = 168;

/** Tokens --rb-* oscuros del tablero. */
const RB = {
  bg1: "#1a1a18",
  surface: "#1e1e1b",
  surface2: "#262622",
  elevated: "#2a2a26",
  text: "#f0f0ea",
  muted: "#9a9a90",
  faint: "#6e6e66",
  hairline: "rgb(240 240 234 / 12%)",
  ok: "#6faf88",
  hoja: "#656561",
};
const INK = "#e5483f";

const PALETTE = ["#e5483f", "#d9622b", "#d6b48a", "#946718", "#3f7355", "#2f8f83", "#526d83", "#7a5ea8", "#c14a7a", "#1c1917"];

type Box = { x: number; y: number; w: number; h: number };

export const CAUSE_TEXT = "Causa: pool de 5 conexiones";
export const FIX_TEXT = "Fix: max 20 + timeout 5 s";
/** El error copiado de la terminal (primer elemento del historial del clipboard). */
export const ERROR_TEXT = "Error: connect ETIMEDOUT 10.0.4.12:5432\n  at pool.query (src/db.ts:88:14)";

const NOTE_CAUSE: Box = { x: 440, y: 110, w: 320, h: 96 };
const NOTE_FIX: Box = { x: 440, y: 250, w: 320, h: 96 };
const ADDED: Box = { x: 390, y: 400, w: 420, h: 96 };

/** Posición del papel dentro de la tarjeta cuando el cajón está cerrado. */
export const paperToCard = (px: number, py: number) => ({
  x: (CARD.w - PAPER.w * ZOOM) / 2 + px * ZOOM,
  y: BAR_H + (VISTA_H - PAPER.h * ZOOM) / 2 + py * ZOOM,
});

/** Elipse a mano alzada (unidades de papel) que rodea la nota de la causa. */
const LOOP = { cx: NOTE_CAUSE.x + 106, cy: NOTE_CAUSE.y + 30, rx: 132, ry: 40, a0: 205, sweep: 385 };
const loopPoint = (u: number) => {
  const a = ((LOOP.a0 + u * LOOP.sweep) * Math.PI) / 180;
  const k = 1 + 0.06 * u;
  return {
    x: LOOP.cx + LOOP.rx * k * Math.cos(a),
    y: LOOP.cy + LOOP.ry * k * Math.sin(a) + Math.sin(u * 9) * 2.5,
  };
};
const LOOP_D = Array.from({ length: 73 }, (_, i) => {
  const p = loopPoint(i / 72);
  return `${i === 0 ? "M" : "L"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`;
}).join(" ");

export type StoryFlipTL = {
  /** Aparece el mensaje de tablero vacío y entra el puntero. */
  emptyAt: number;
  cursorInAt: number;
  textToolAt: number;
  noteAt: number;
  noteTypeAt: number;
  note2At: number;
  note2TypeAt: number;
  noteMsPerChar: number;
  pencilToolAt: number;
  paletteOffAt: number;
  drawAt: number;
  drawMs: number;
  drawerAt: number;
  previewAt: number;
  addAt: number;
  saved: { from: number; to: number }[];
};

type Pt = { x: number; y: number; w: number; h: number };
type Pts = Record<string, Pt>;

/** Posición de `el` respecto a `root` sumando offsets (ignora transformaciones). */
const rel = (root: HTMLElement, el: HTMLElement): Pt => {
  let x = 0;
  let y = 0;
  let cur: HTMLElement | null = el;
  while (cur && cur !== root) {
    x += cur.offsetLeft;
    y += cur.offsetTop;
    cur = cur.offsetParent as HTMLElement | null;
  }
  return { x, y, w: el.offsetWidth, h: el.offsetHeight };
};

const samePts = (a: Pts | null, b: Pts) => {
  if (!a) return false;
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  if (ka.length !== kb.length) return false;
  return kb.every((k) => a[k] && a[k].x === b[k].x && a[k].y === b[k].y && a[k].w === b[k].w && a[k].h === b[k].h);
};

const mixHex = (a: string, b: string, t: number) => {
  const pa = [1, 3, 5].map((i) => parseInt(a.slice(i, i + 2), 16));
  const pb = [1, 3, 5].map((i) => parseInt(b.slice(i, i + 2), 16));
  return `rgb(${pa.map((v, i) => Math.round(v + (pb[i] - v) * t)).join(" ")})`;
};

const cubicOut = Easing.out(Easing.cubic);

/** Transición Svelte `emerge`: opacity t, translateY(u·8), scale(.97+.03t), blur(u·2). */
const emerge = (ms: number, at: number, dur = 200): CSSProperties => {
  const t = cubicOut(seg(ms, at, dur));
  const u = 1 - t;
  return {
    opacity: t,
    transform: `translateY(${u * 8}px) scale(${0.97 + 0.03 * t})`,
    filter: u > 0 ? `blur(${u * 2}px)` : undefined,
  };
};
/** Salida de `emerge` (125 ms). */
const emergeOut = (ms: number, at: number, dur = 125): CSSProperties => {
  const t = 1 - cubicOut(seg(ms, at, dur));
  const u = 1 - t;
  return {
    opacity: t,
    transform: `translateY(${u * 8}px) scale(${0.97 + 0.03 * t})`,
    filter: u > 0 ? `blur(${u * 2}px)` : undefined,
  };
};

type ToolId = "select" | "draw" | "highlight" | "eraser" | "text" | "check";
const TOOLS: { id: ToolId; label: string; key: string; icon: TIconName; ink?: boolean }[] = [
  { id: "select", label: "Mover", key: "V", icon: "pointer" },
  { id: "draw", label: "Lápiz", key: "P", icon: "pencil", ink: true },
  { id: "highlight", label: "Resaltador", key: "H", icon: "highlighter", ink: true },
  { id: "eraser", label: "Borrador", key: "E", icon: "eraser" },
  { id: "text", label: "Texto", key: "T", icon: "type" },
  { id: "check", label: "Lista", key: "L", icon: "listChecks" },
];

const btnBase: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 6.4,
  minHeight: 36,
  boxSizing: "border-box",
  borderRadius: 999,
  padding: "6.4px 14.4px",
  fontSize: 13,
  fontWeight: 500,
  lineHeight: 1.2,
  color: RB.muted,
  whiteSpace: "nowrap",
  position: "relative",
};

/** Objeto de texto del tablero: agarre, quitar y asa cuando está seleccionado. */
const TextObject = ({
  box,
  text,
  placeholder,
  chrome,
  selected,
  caret,
  mono,
  style,
}: {
  box: Box;
  text: string;
  placeholder?: string;
  chrome: number;
  selected: number;
  caret: boolean;
  mono?: boolean;
  style?: CSSProperties;
}) => (
  <div
    style={{
      position: "absolute",
      left: box.x,
      top: box.y,
      width: box.w,
      height: box.h,
      boxSizing: "border-box",
      borderRadius: 5,
      background: RB.hoja,
      outline: `${1 + 0.5 * selected}px solid rgb(240 240 234 / ${(0.45 * selected).toFixed(3)})`,
      ...style,
    }}
  >
    <div
      style={{
        position: "absolute",
        left: 0,
        right: 0,
        top: 0,
        height: 20,
        borderRadius: "5px 5px 0 0",
        background: "rgb(240 240 234 / 8%)",
        display: "grid",
        placeItems: "center",
        color: RB.muted,
        opacity: chrome,
      }}
    >
      <TIcon name="grip" size={12} style={{ transform: "rotate(90deg)" }} />
    </div>
    <div
      style={{
        position: "absolute",
        top: 3,
        right: 3,
        width: 20,
        height: 20,
        borderRadius: 999,
        background: RB.surface,
        border: `1px solid ${RB.hairline}`,
        boxSizing: "border-box",
        display: "grid",
        placeItems: "center",
        color: RB.muted,
        opacity: chrome,
      }}
    >
      <TIcon name="x" size={11} />
    </div>
    <div
      style={{
        position: "absolute",
        right: -7,
        bottom: -7,
        width: 16,
        height: 16,
        boxSizing: "border-box",
        border: `2px solid ${RB.surface}`,
        borderRadius: 3,
        background: RB.text,
        opacity: selected,
        transform: `scale(${0.25 + 0.75 * selected})`,
      }}
    />
    <div
      style={{
        padding: "18px 12px 10px",
        fontSize: mono ? 12.5 : 13.5,
        fontFamily: mono ? FONT_MONO : undefined,
        lineHeight: 1.5,
        color: RB.text,
        whiteSpace: "pre-wrap",
        overflow: "hidden",
        height: "100%",
        boxSizing: "border-box",
      }}
    >
      {text === "" && placeholder ? (
        <span style={{ fontStyle: "italic", color: RB.muted }}>{placeholder}</span>
      ) : (
        <>
          {text}
          {caret && <span style={{ display: "inline-block", width: 1.5, height: 15, marginLeft: 1, verticalAlign: "-3px", background: RB.text }} />}
        </>
      )}
    </div>
  </div>
);

/** Miniatura de página: el mismo dibujo del papel a escala. */
const MiniPage = ({
  boxes,
  inkP,
  active,
  n,
  removable,
}: {
  boxes: Box[];
  inkP: number;
  active: boolean;
  n: number;
  removable?: boolean;
}) => {
  const bar = (b: Box, k: number) => (
    <rect key={`${b.x}${b.y}${k}`} x={b.x + 36} y={b.y + 54 + k * 60} width={(b.w - 72) * (k === 1 ? 0.55 : 1)} height={28} rx={12} fill="rgb(240 240 234 / 38%)" />
  );
  return (
    <div
      style={{
        position: "relative",
        width: 84,
        height: 63,
        boxSizing: "border-box",
        border: `1px solid ${active ? RB.text : RB.hairline}`,
        boxShadow: active ? `0 0 0 1px ${RB.text}` : undefined,
        borderRadius: 5,
        background: RB.elevated,
        flex: "none",
      }}
    >
      <div style={{ position: "absolute", inset: 0, borderRadius: 4, overflow: "hidden" }}>
        <svg width={82} height={61} viewBox="0 0 1200 900" style={{ display: "block" }}>
          {boxes.map((b) => [0, 1].map((k) => bar(b, k)))}
          {inkP > 0 && (
            <path d={LOOP_D} pathLength={1} stroke={INK} strokeWidth={22} fill="none" strokeLinecap="round" strokeLinejoin="round" strokeDasharray="1 2" strokeDashoffset={1 - inkP} />
          )}
        </svg>
      </div>
      <span style={{ position: "absolute", right: 4, bottom: 2, fontSize: 9, fontWeight: 600, color: RB.muted, textShadow: `0 0 3px ${RB.elevated}` }}>{n}</span>
      {removable && (
        <span style={{ position: "absolute", top: -5, right: -5, width: 16, height: 16, borderRadius: 999, background: RB.surface, border: `1px solid ${RB.hairline}`, boxSizing: "border-box", display: "grid", placeItems: "center", color: RB.muted }}>
          <TIcon name="x" size={10} />
        </span>
      )}
    </div>
  );
};

const DRAWER_TABS = ["Clip", "Textos", "Fotos", "Reun."];
/** Historial del clipboard (más nuevo arriba): el primero es el error copiado en el paso 3. */
const CLIPS = [ERROR_TEXT, "https://panel.tienda.cl/rendimiento", "10.0.4.12", "SELECT * FROM orders WHERE user = $1"];

type Props = { ms: number; tl: StoryFlipTL };

/** Reverso de la tarjeta: FlipBoard (barra, vista con papel punteado, cajón Insertar y tira de páginas). */
export const StoryFlipBack = ({ ms, tl }: Props) => {
  const rootRef = useRef<HTMLDivElement>(null);
  const els = useRef<Record<string, HTMLElement | null>>({});
  const drawerRef = useRef<HTMLDivElement>(null);
  const groupRef = useRef<HTMLDivElement>(null);
  const [pts, setPts] = useState<Pts | null>(null);
  const reg = (k: string) => (el: HTMLElement | null) => {
    els.current[k] = el;
  };

  useLayoutEffect(() => {
    const root = rootRef.current;
    const drawer = drawerRef.current;
    const group = groupRef.current;
    if (!root || !drawer || !group) return;
    const out: Pts = { group: rel(root, group) };
    for (const [k, el] of Object.entries(els.current)) {
      if (!el) continue;
      if (k.startsWith("d-")) {
        const p = rel(drawer, el);
        out[k] = { ...p, x: p.x + (CARD.w - DRAWER_W), y: p.y + BAR_H };
      } else if (k.startsWith("t-")) {
        const p = rel(group, el);
        out[k] = { ...p, x: p.x + out.group.x, y: p.y + out.group.y };
      } else {
        out[k] = rel(root, el);
      }
    }
    setPts((prev) => (samePts(prev, out) ? prev : out));
  });

  // ---- estado derivado del guion ----
  const drawerK = EASE.smoothOut(seg(ms, tl.drawerAt, 200));
  const drawerCol = DRAWER_W * drawerK;
  const vistaW = CARD.w - drawerCol;
  const paperL = (vistaW - PAPER.w * ZOOM) / 2;
  const paperT = (VISTA_H - PAPER.h * ZOOM) / 2;

  const tools: { at: number; tool: ToolId }[] = [
    { at: -1, tool: "select" },
    { at: tl.textToolAt, tool: "text" },
    { at: tl.pencilToolAt, tool: "draw" },
  ];
  let curIdx = 0;
  tools.forEach((t, i) => {
    if (ms >= t.at) curIdx = i;
  });
  const curTool = tools[curIdx];
  const prevTool = curIdx > 0 ? tools[curIdx - 1] : null;
  const toolK = curIdx > 0 ? EASE.smoothOut(seg(ms, curTool.at, 150)) : 1;
  const toolOn = (id: ToolId) => (id === curTool.tool ? toolK : prevTool && id === prevTool.tool ? 1 - toolK : 0);

  const noteDefs = [
    { box: NOTE_CAUSE, full: CAUSE_TEXT, at: tl.noteAt, typeAt: tl.noteTypeAt, endAt: tl.note2At },
    { box: NOTE_FIX, full: FIX_TEXT, at: tl.note2At, typeAt: tl.note2TypeAt, endAt: tl.pencilToolAt },
  ];
  const notes = noteDefs.map((n) => {
    const visible = ms >= n.at;
    const selected = visible ? seg(ms, n.at, 125) * (1 - seg(ms, n.endAt, 125)) : 0;
    const text = typed(n.full, ms, n.typeAt, tl.noteMsPerChar);
    const typing = ms >= n.typeAt && text.length < n.full.length;
    const focused = visible && ms < n.endAt;
    return { ...n, visible, selected, text, caretOn: focused && (typing || Math.floor(ms / 530) % 2 === 0) };
  });
  const noteVisible = notes[0].visible;

  const inkP = seg(ms, tl.drawAt, tl.drawMs);
  const addedVisible = ms >= tl.addAt;
  const addedSelected = addedVisible ? seg(ms, tl.addAt, 125) : 0;
  const emptyOn = ms >= tl.emptyAt && ms < tl.noteAt + 125;

  const paletteIn = ms >= tl.pencilToolAt && ms < tl.paletteOffAt;
  const paletteOut = ms >= tl.paletteOffAt && ms < tl.paletteOffAt + 125;
  const previewIn = ms >= tl.previewAt && ms < tl.addAt + 75;
  const previewK = previewIn
    ? ms < tl.addAt
      ? EASE.smoothOut(seg(ms, tl.previewAt, 125))
      : 1 - seg(ms, tl.addAt, 75)
    : 0;

  const savedK = (() => {
    for (const s of tl.saved) {
      if (ms >= s.from && ms < s.to + 125) return ms < s.to ? seg(ms, s.from, 200) : 1 - seg(ms, s.to, 125);
    }
    return 0;
  })();

  // ---- cursor de guion, con blancos medidos en el DOM ----
  const cursorKeys: CursorKey[] = [];
  if (pts) {
    const center = (k: string, dx = 0.5, dy = 0.5) => ({ x: pts[k].x + pts[k].w * dx, y: pts[k].y + pts[k].h * dy });
    const paperAt = (px: number, py: number) => {
      const p = paperToCard(px, py);
      return { x: p.x, y: p.y };
    };
    const hold = (click: number, p: { x: number; y: number }, lead = 20, after = 120) => {
      cursorKeys.push({ ms: click - lead, ...p }, { ms: click + after, ...p });
    };
    cursorKeys.push({ ms: tl.cursorInAt, x: 1000, y: 700 });
    hold(tl.textToolAt, center("t-text", 0.4, 0.5));
    hold(tl.noteAt, paperAt(NOTE_CAUSE.x + 40, NOTE_CAUSE.y + 46));
    hold(tl.note2At, paperAt(NOTE_FIX.x + 40, NOTE_FIX.y + 46));
    hold(tl.pencilToolAt, center("t-draw", 0.4, 0.5));
    const start = loopPoint(0);
    const sp = paperAt(start.x, start.y);
    cursorKeys.push({ ms: tl.drawAt - 40, ...sp });
    for (let i = 1; i <= 16; i++) {
      const p = paperAt(loopPoint(i / 16).x, loopPoint(i / 16).y);
      cursorKeys.push({ ms: tl.drawAt + (tl.drawMs * i) / 16, ...p, linear: true });
    }
    hold(tl.drawerAt, center("act-insert", 0.45, 0.5));
    hold(tl.previewAt, center("d-item"));
    hold(tl.addAt, center("prev-add"), 20, 400);
  }

  // ---- píldora deslizante de herramientas ----
  const slider = (() => {
    if (!pts || !pts[`t-${curTool.tool}`]) return null;
    const cur = pts[`t-${curTool.tool}`];
    const prev = prevTool && pts[`t-${prevTool.tool}`] ? pts[`t-${prevTool.tool}`] : cur;
    return { x: prev.x + (cur.x - prev.x) * toolK - pts.group.x, w: prev.w + (cur.w - prev.w) * toolK };
  })();

  const paletteLeft = pts && pts["t-draw"] ? pts["t-draw"].x : 0;

  return (
    <div
      ref={rootRef}
      style={{
        position: "absolute",
        inset: 0,
        background: RB.bg1,
        color: RB.text,
        fontFamily: FONT_SANS,
        display: "flex",
        flexDirection: "column",
        overflow: "hidden",
        WebkitFontSmoothing: "antialiased",
      }}
    >
      {/* ---------- barra ---------- */}
      <div
        style={{
          position: "relative",
          height: BAR_H,
          boxSizing: "border-box",
          padding: "0 18px 8px",
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          justifyContent: "center",
          gap: 12,
          flex: "none",
          zIndex: 5,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8, maxWidth: "34%" }}>
          <span style={{ font: "600 13px/1.3 'Avenir Next','Aptos Display','Segoe UI Variable Display','Segoe UI Variable',sans-serif" }}>Tablero</span>
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 5,
              padding: "2px 8px 2px 3px",
              borderRadius: 999,
              background: RB.surface2,
              color: "rgb(240 240 234 / 72%)",
              fontSize: 11,
              fontWeight: 500,
              whiteSpace: "nowrap",
            }}
          >
            <span style={{ width: 16, height: 16, borderRadius: 4, background: RB.elevated, display: "grid", placeItems: "center", color: RB.muted }}>
              <TIcon name="appWindow" size={11} />
            </span>
            sobre {FLIP_WINDOW_TITLE}
          </span>
        </div>

        <div ref={groupRef} style={{ position: "relative", display: "flex", gap: 2 }}>
          {slider && (
            <span
              style={{
                position: "absolute",
                top: 0,
                bottom: 0,
                left: 0,
                width: slider.w,
                transform: `translateX(${slider.x}px)`,
                borderRadius: 999,
                background: "rgb(240 240 234 / 10%)",
              }}
            />
          )}
          {TOOLS.map((t) => (
            <span key={t.id} ref={reg(`t-${t.id}`)} style={{ ...btnBase, color: mixHex("#9a9a90", "#f0f0ea", toolOn(t.id)) }}>
              <span style={{ position: "relative", display: "flex" }}>
                <TIcon name={t.icon} size={15} />
                {t.ink && (
                  <span style={{ position: "absolute", right: -3, bottom: -3, width: 7, height: 7, borderRadius: 999, background: INK, boxShadow: `0 0 0 1.5px ${RB.surface}` }} />
                )}
              </span>
              {t.label}
            </span>
          ))}
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: 2 }}>
          <span style={{ ...btnBase, gap: 5, opacity: ms >= tl.noteTypeAt ? 1 : 0.45 }}>
            <TIcon name="undo" size={15} />
          </span>
          <span style={{ ...btnBase, gap: 5, opacity: 0.45 }}>
            <TIcon name="redo" size={15} />
          </span>
          <span style={{ ...btnBase, gap: 5 }}>
            <TIcon name="minus" size={15} />
          </span>
          <span style={{ ...btnBase, minWidth: 51.2, fontVariantNumeric: "tabular-nums" }}>{Math.round(ZOOM * 100)}%</span>
          <span style={{ ...btnBase, gap: 5 }}>
            <TIcon name="plus" size={15} />
          </span>
          <span style={{ ...btnBase, gap: 5 }}>
            <TIcon name="download" size={15} />
          </span>
          <span ref={reg("act-insert")} style={{ ...btnBase, gap: 5, color: mixHex("#9a9a90", "#f0f0ea", drawerK) }}>
            <TIcon name={drawerK > 0.5 ? "panelRightClose" : "panelRightOpen"} size={15} />
          </span>
          <span style={{ ...btnBase, gap: 5, color: RB.text, background: "rgb(240 240 234 / 6%)" }}>
            <TIcon name="arrowLeft" size={15} />
          </span>
        </div>

        {(paletteIn || paletteOut) && (
          <div
            style={{
              position: "absolute",
              top: BAR_H + 8,
              left: paletteLeft,
              maxWidth: 232,
              boxSizing: "border-box",
              padding: 8,
              display: "flex",
              flexWrap: "wrap",
              gap: 4,
              background: RB.surface,
              border: `1px solid ${RB.hairline}`,
              borderRadius: 8,
              boxShadow: "0 8px 24px rgb(0 0 0 / 22%)",
              ...(paletteIn ? emerge(ms, tl.pencilToolAt) : emergeOut(ms, tl.paletteOffAt)),
            }}
          >
            {PALETTE.map((c, i) => (
              <span key={c} style={{ position: "relative", width: 24, height: 24, borderRadius: 5 }}>
                <span
                  style={{
                    position: "absolute",
                    inset: i === 0 ? 3 : 5,
                    borderRadius: 999,
                    background: c,
                    boxShadow: `inset 0 0 0 1px rgb(0 0 0 / 25%)${i === 0 ? `, 0 0 0 2px ${RB.surface}, 0 0 0 3px ${RB.text}` : ""}`,
                  }}
                />
              </span>
            ))}
            <span style={{ position: "relative", width: 24, height: 24, borderRadius: 5, display: "grid", placeItems: "center", color: RB.muted }}>
              <span style={{ width: 16, height: 16, borderRadius: 999, boxSizing: "border-box", border: `1.5px dashed ${RB.muted}`, display: "grid", placeItems: "center" }}>
                <TIcon name="plus" size={9} strokeWidth={2.4} />
              </span>
            </span>
          </div>
        )}
      </div>

      {/* ---------- cuerpo: vista + cajón ---------- */}
      <div style={{ position: "relative", height: VISTA_H, flex: "none", display: "flex" }}>
        <div style={{ position: "relative", width: vistaW, height: VISTA_H, overflow: "hidden", flex: "none", background: RB.bg1 }}>
          <div
            style={{
              position: "absolute",
              left: paperL,
              top: paperT,
              width: PAPER.w,
              height: PAPER.h,
              transform: `scale(${ZOOM})`,
              transformOrigin: "0 0",
              borderRadius: 8,
              boxShadow: "0 2px 8px rgb(0 0 0 / 28%)",
              overflow: "hidden",
              background: `radial-gradient(circle, rgb(240 240 234 / 22%) 1px, transparent 1.25px) 12px 12px / 24px 24px, ${RB.hoja}`,
            }}
          >
            {emptyOn && (
              <div
                style={{
                  position: "absolute",
                  left: "15%",
                  right: "15%",
                  top: "26%",
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: 12,
                  textAlign: "center",
                  fontSize: 13,
                  lineHeight: 1.45,
                  ...(ms >= tl.noteAt ? emergeOut(ms, tl.noteAt) : emerge(ms, tl.emptyAt)),
                }}
              >
                <span style={{ maxWidth: "36ch" }}>Elige el Lápiz para esbozar, o empieza con una nota.</span>
                <span style={{ ...btnBase, color: RB.text, background: "rgb(240 240 234 / 6%)" }}>Añadir texto</span>
              </div>
            )}

            {notes.map(
              (n) =>
                n.visible && (
                  <TextObject
                    key={n.full}
                    box={n.box}
                    text={n.text}
                    placeholder="Escribe aquí. Se guarda solo y lo ves desde cualquier ventana."
                    chrome={n.selected}
                    selected={n.selected}
                    caret={n.caretOn}
                    style={emerge(ms, n.at)}
                  />
                ),
            )}
            {addedVisible && (
              <TextObject
                box={ADDED}
                text={ERROR_TEXT}
                chrome={addedSelected}
                selected={addedSelected}
                caret={false}
                mono
                style={emerge(ms, tl.addAt)}
              />
            )}

            <svg width={PAPER.w} height={PAPER.h} viewBox={`0 0 ${PAPER.w} ${PAPER.h}`} style={{ position: "absolute", left: 0, top: 0, pointerEvents: "none" }}>
              {inkP > 0 && (
                <path d={LOOP_D} pathLength={1} stroke={INK} strokeWidth={2.8} fill="none" strokeLinecap="round" strokeLinejoin="round" strokeDasharray="1 2" strokeDashoffset={1 - inkP} />
              )}
            </svg>
          </div>
        </div>

        {/* cajón Insertar (pestaña Clip) */}
        <div
          style={{
            position: "relative",
            width: drawerCol,
            height: VISTA_H,
            overflow: "hidden",
            flex: "none",
            borderLeft: `1px solid rgb(240 240 234 / ${(0.12 * drawerK).toFixed(3)})`,
            boxSizing: "border-box",
          }}
        >
          <div
            ref={drawerRef}
            style={{
              position: "absolute",
              left: 0,
              top: 0,
              width: DRAWER_W,
              height: VISTA_H,
              boxSizing: "border-box",
              padding: "0 18px 0 10px",
              display: "flex",
              flexDirection: "column",
              gap: 6,
              opacity: seg(ms, tl.drawerAt + 30, 150),
              transform: `translateX(${(1 - seg(ms, tl.drawerAt + 30, 150)) * 8}px)`,
            }}
          >
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: 3,
                paddingBottom: 6,
                borderBottom: `1px solid ${RB.hairline}`,
                background: RB.bg1,
              }}
            >
              {DRAWER_TABS.map((label, i) => (
                <span
                  key={label}
                  style={{
                    padding: "4px 2px",
                    borderRadius: 5,
                    textAlign: "center",
                    fontSize: 10,
                    fontWeight: 600,
                    color: i === 0 ? RB.text : RB.muted,
                    background: i === 0 ? "rgb(240 240 234 / 10%)" : "transparent",
                  }}
                >
                  {label}
                </span>
              ))}
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {CLIPS.map((t, i) => (
                <Recorte key={t} hot={i === 0 && ms >= tl.previewAt - 60 && ms < tl.addAt} reg={i === 0 ? reg("d-item") : undefined}>
                  {t.replace(/\n\s*/g, " ")}
                </Recorte>
              ))}
            </div>
          </div>
        </div>
      </div>

      {/* ---------- tira de páginas ---------- */}
      <div
        style={{
          height: TIRA_H,
          flex: "none",
          boxSizing: "border-box",
          padding: "6px 18px 10px",
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          gap: 6,
        }}
      >
        <MiniPage boxes={[...(noteVisible ? [NOTE_CAUSE] : []), ...(notes[1].visible ? [NOTE_FIX] : []), ...(addedVisible ? [ADDED] : [])]} inkP={inkP} active n={1} />
        <MiniPage boxes={[]} inkP={0} active={false} n={2} removable />
        <div
          style={{
            width: 84,
            height: 63,
            boxSizing: "border-box",
            border: `1px dashed ${RB.hairline}`,
            borderRadius: 5,
            display: "grid",
            placeItems: "center",
            color: RB.muted,
          }}
        >
          <TIcon name="plus" size={15} />
        </div>
      </div>

      {/* ---------- vista previa antes de insertar ---------- */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          zIndex: 20,
          padding: 16,
          boxSizing: "border-box",
          display: "grid",
          placeItems: "center",
          background: `rgb(26 26 24 / ${(0.55 * previewK).toFixed(3)})`,
          visibility: previewK > 0 ? "visible" : "hidden",
        }}
      >
        <div
          style={{
            width: 420,
            boxSizing: "border-box",
            padding: 12,
            display: "flex",
            flexDirection: "column",
            gap: 10,
            background: RB.surface,
            border: `1px solid ${RB.hairline}`,
            borderRadius: 8,
            boxShadow: "0 12px 40px rgb(0 0 0 / 28%)",
            opacity: previewK,
            transform: `scale(${0.96 + 0.04 * previewK})`,
            filter: previewK < 1 ? `blur(${(1 - previewK) * 2}px)` : undefined,
          }}
        >
          <span style={{ fontSize: 11, fontWeight: 600, color: RB.muted }}>Esto se va a añadir al tablero</span>
          <pre style={{ margin: 0, font: `400 12px/1.45 ${FONT_MONO}`, whiteSpace: "pre-wrap", color: RB.text }}>{ERROR_TEXT}</pre>
          <div style={{ display: "flex", justifyContent: "flex-end", gap: 6 }}>
            <span style={btnBase}>Cancelar</span>
            <span ref={reg("prev-add")} style={{ ...btnBase, color: RB.text, background: "rgb(240 240 234 / 10%)" }}>
              Añadir
            </span>
          </div>
        </div>
      </div>

      {/* píldora «Guardado» */}
      <div
        style={{
          position: "absolute",
          left: 18,
          bottom: 14,
          display: "inline-flex",
          alignItems: "center",
          gap: 4,
          padding: "4px 9px",
          borderRadius: 999,
          background: RB.surface,
          color: RB.ok,
          fontSize: 11.5,
          boxShadow: "0 2px 10px rgb(0 0 0 / 20%)",
          zIndex: 30,
          ...(savedK > 0 ? { opacity: savedK, transform: `translateY(${(1 - savedK) * 8}px)` } : { opacity: 0 }),
        }}
      >
        <TIcon name="check" size={12} />
        Guardado
      </div>

      <div style={{ position: "absolute", inset: 0, zIndex: 40, pointerEvents: "none" }}>
        <FlipCursor ms={ms} keys={cursorKeys} fadeOutAt={tl.addAt + 450} clicks={[tl.textToolAt, tl.noteAt, tl.note2At, tl.pencilToolAt, tl.drawerAt, tl.previewAt, tl.addAt]} size={30} />
      </div>
    </div>
  );
};

const Recorte = ({
  hot,
  reg,
  children,
}: {
  hot?: boolean;
  reg?: (el: HTMLDivElement | null) => void;
  children: ReactNode;
}) => (
  <div
    ref={reg}
    style={{
      padding: "5px 7px",
      borderRadius: 5,
      background: hot ? "rgb(240 240 234 / 10%)" : RB.surface2,
      fontSize: 11.5,
      lineHeight: 1.35,
      flex: "none",
    }}
  >
    <div
      style={{
        display: "-webkit-box",
        WebkitLineClamp: 2,
        WebkitBoxOrient: "vertical",
        overflow: "hidden",
        color: RB.text,
        wordBreak: "break-word",
      }}
    >
      {children}
    </div>
  </div>
);
