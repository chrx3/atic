import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Easing } from "remotion";
import { FLIP_WINDOW_TITLE } from "./FlipFront";
import { FlipCursor, type CursorKey } from "./FlipCursor";
import { SNIPPETS } from "./TextosPanel";
import { TIcon, type TIconName } from "./textosIcons";
import { EASE, FONT_SANS } from "./theme";
import { seg } from "./time";
import { typed } from "./ui";

/** Tarjeta 1280×800 (ejemplo numérico de la ficha B.2 / B.4). */
export const CARD = { w: 1280, h: 800 } as const;
const BAR_H = 44;
const TIRA_H = 79;
const VISTA_H = CARD.h - BAR_H - TIRA_H; // 677
const PAPER = { w: 1200, h: 900 } as const;
/** zoom = min(1, (vw-24)/1200, (vh-24)/900) con vista 1256×677 → 0.7256 («73%»). */
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

export const NOTE_TEXT = "Preguntar por el sensor de pH";
const NOTE = { x: 170, y: 90, w: 280, h: 96 };
const ADDED = { x: 460, y: 402, w: 280, h: 96 };
const ADDED_ITEM = SNIPPETS[3];

/** Elipse a mano alzada (unidades de papel) que rodea la nota. */
const LOOP = { cx: NOTE.x + 102, cy: NOTE.y + 29, rx: 134, ry: 42, a0: 205, sweep: 385 };
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

export type FlipTL = {
  /** Aparece el mensaje de tablero vacío. */
  emptyAt: number;
  textToolAt: number;
  noteAt: number;
  noteTypeAt: number;
  noteMsPerChar: number;
  pencilToolAt: number;
  paletteOffAt: number;
  drawAt: number;
  drawMs: number;
  drawerAt: number;
  drawerTabAt: number;
  previewAt: number;
  addAt: number;
  cursorInAt: number;
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

const inOut = (k: number) => (k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2);

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

/** Objeto de texto del tablero (280×96): agarre, quitar y asa cuando está seleccionado. */
const TextObject = ({
  box,
  text,
  placeholder,
  chrome,
  selected,
  caret,
  style,
}: {
  box: { x: number; y: number; w: number; h: number };
  text: string;
  placeholder?: string;
  chrome: number;
  selected: number;
  caret: boolean;
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
        fontSize: 13.5,
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
  noteOn,
  addedOn,
  inkP,
  active,
  n,
  removable,
}: {
  noteOn: boolean;
  addedOn: boolean;
  inkP: number;
  active: boolean;
  n: number;
  removable?: boolean;
}) => {
  const bar = (x: number, y: number, w: number, k: number) => (
    <rect key={`${x}${y}${k}`} x={x + 36} y={y + 54 + k * 60} width={(w - 72) * (k === 1 ? 0.55 : 1)} height={28} rx={12} fill="rgb(240 240 234 / 38%)" />
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
          {noteOn && [0, 1].map((k) => bar(NOTE.x, NOTE.y, NOTE.w, k))}
          {addedOn && [0, 1].map((k) => bar(ADDED.x, ADDED.y, ADDED.w, k))}
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
const CLIPS = ["Caudal promedio: 42 L/s", "https://ejemplo.cl/informes/semana-38", "Sensor de pH estable en 7,2"];

type Props = { ms: number; tl: FlipTL };

/** Reverso de la tarjeta: FlipBoard (barra, vista con papel punteado, cajón Insertar y tira de páginas). */
export const FlipBack = ({ ms, tl }: Props) => {
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

  const noteVisible = ms >= tl.noteAt;
  const noteSelected = noteVisible && ms < tl.pencilToolAt ? seg(ms, tl.noteAt, 125) : 1 - seg(ms, tl.pencilToolAt, 125);
  const noteText = typed(NOTE_TEXT, ms, tl.noteTypeAt, tl.noteMsPerChar);
  const noteTyping = ms >= tl.noteTypeAt && noteText.length < NOTE_TEXT.length;
  const noteFocused = noteVisible && ms < tl.pencilToolAt;
  const caretOn = noteFocused && (noteTyping || Math.floor(ms / 530) % 2 === 0);
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
    const paperAt = (px: number, py: number) => ({
      x: (CARD.w - PAPER.w * ZOOM) / 2 + px * ZOOM,
      y: BAR_H + paperT + py * ZOOM,
    });
    const hold = (click: number, p: { x: number; y: number }, lead = 20, after = 120) => {
      cursorKeys.push({ ms: click - lead, ...p }, { ms: click + after, ...p });
    };
    cursorKeys.push({ ms: tl.cursorInAt, x: 1000, y: 620 });
    hold(tl.textToolAt, center("t-text", 0.4, 0.5));
    hold(tl.noteAt, paperAt(NOTE.x + 40, NOTE.y + 46));
    hold(tl.pencilToolAt, center("t-draw", 0.4, 0.5));
    const start = loopPoint(0);
    const sp = paperAt(start.x, start.y);
    cursorKeys.push({ ms: tl.drawAt - 40, ...sp });
    for (let i = 1; i <= 16; i++) {
      const p = paperAt(loopPoint(i / 16).x, loopPoint(i / 16).y);
      cursorKeys.push({ ms: tl.drawAt + (tl.drawMs * i) / 16, ...p, linear: true });
    }
    hold(tl.drawerAt, center("act-insert", 0.45, 0.5));
    hold(tl.drawerTabAt, center("d-tab1"));
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

            {noteVisible && (
              <TextObject
                box={NOTE}
                text={noteText}
                placeholder="Escribe aquí. Se guarda solo y lo ves desde cualquier ventana."
                chrome={noteSelected}
                selected={noteSelected}
                caret={caretOn}
                style={emerge(ms, tl.noteAt)}
              />
            )}
            {addedVisible && (
              <TextObject
                box={ADDED}
                text={ADDED_ITEM.body}
                chrome={addedSelected}
                selected={addedSelected}
                caret={false}
                style={emerge(ms, tl.addAt)}
              />
            )}

            <svg width={PAPER.w} height={PAPER.h} viewBox={`0 0 ${PAPER.w} ${PAPER.h}`} style={{ position: "absolute", left: 0, top: 0, pointerEvents: "none" }}>
              {inkP > 0 && (
                <path d={LOOP_D} pathLength={1} stroke={INK} strokeWidth={2.6} fill="none" strokeLinecap="round" strokeLinejoin="round" strokeDasharray="1 2" strokeDashoffset={1 - inkP} />
              )}
            </svg>
          </div>
        </div>

        {/* cajón Insertar */}
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
              {DRAWER_TABS.map((label, i) => {
                const start =
                  ms < tl.drawerTabAt
                    ? i === 0
                      ? 1
                      : 0
                    : i === 1
                      ? seg(ms, tl.drawerTabAt, 75)
                      : i === 0
                        ? 1 - seg(ms, tl.drawerTabAt, 75)
                        : 0;
                return (
                  <span
                    key={label}
                    ref={reg(`d-tab${i}`)}
                    style={{
                      padding: "4px 2px",
                      borderRadius: 5,
                      textAlign: "center",
                      fontSize: 10,
                      fontWeight: 600,
                      color: mixHex("#9a9a90", "#f0f0ea", start),
                      background: `rgb(240 240 234 / ${(0.1 * start).toFixed(3)})`,
                    }}
                  >
                    {label}
                  </span>
                );
              })}
            </div>
            <div style={{ position: "relative", flex: 1 }}>
              {/* pestaña Clip */}
              <div
                style={{
                  position: "absolute",
                  inset: 0,
                  display: "flex",
                  flexDirection: "column",
                  gap: 6,
                  opacity: ms >= tl.drawerTabAt ? 1 - inOut(seg(ms, tl.drawerTabAt, 125)) : 1,
                  transform: `translateY(${ms >= tl.drawerTabAt ? inOut(seg(ms, tl.drawerTabAt, 125)) * 4 : 0}px)`,
                }}
              >
                {CLIPS.map((t) => (
                  <Recorte key={t}>{t}</Recorte>
                ))}
              </div>
              {/* pestaña Textos (siempre montada para medir el blanco del cursor) */}
              <div
                style={{
                  position: "absolute",
                  inset: 0,
                  display: "flex",
                  flexDirection: "column",
                  gap: 6,
                  opacity: ms >= tl.drawerTabAt ? inOut(seg(ms, tl.drawerTabAt, 125)) : 0,
                  transform: `translateY(${ms >= tl.drawerTabAt ? (1 - inOut(seg(ms, tl.drawerTabAt, 125))) * 4 : 4}px)`,
                }}
              >
                {SNIPPETS.slice(0, 5).map((s, i) => (
                  <Recorte
                    key={s.name}
                    name={s.name}
                    hot={i === 3 && ms >= tl.previewAt - 60 && ms < tl.addAt}
                    reg={i === 3 ? reg("d-item") : undefined}
                  >
                    {s.body.replace(/\n/g, " ")}
                  </Recorte>
                ))}
              </div>
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
        <MiniPage noteOn={noteVisible} addedOn={addedVisible} inkP={inkP} active n={1} />
        <MiniPage noteOn={false} addedOn={false} inkP={0} active={false} n={2} removable />
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
          <pre style={{ margin: 0, font: "400 12.5px/1.45 'Aptos','Segoe UI Variable Text','Segoe UI Variable',sans-serif", whiteSpace: "pre-wrap", color: RB.text }}>
            {ADDED_ITEM.body}
          </pre>
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
        <FlipCursor ms={ms} keys={cursorKeys} fadeOutAt={tl.addAt + 450} clicks={[tl.textToolAt, tl.noteAt, tl.pencilToolAt, tl.drawerAt, tl.drawerTabAt, tl.previewAt, tl.addAt]} size={30} />
      </div>
    </div>
  );
};

const Recorte = ({
  name,
  hot,
  reg,
  children,
}: {
  name?: string;
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
    {name && <div style={{ fontSize: 10.5, fontWeight: 600, color: RB.text }}>{name}</div>}
    <div
      style={{
        display: "-webkit-box",
        WebkitLineClamp: 2,
        WebkitBoxOrient: "vertical",
        overflow: "hidden",
        color: RB.text,
      }}
    >
      {children}
    </div>
  </div>
);
