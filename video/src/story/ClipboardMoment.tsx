import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Caption } from "../lib/Caption";
import { CB, PANEL_PILL_BOTTOM } from "../lib/Clipboard";
import { BrowserApp, D, Desktop, EditorApp, ERROR_LINES } from "../desk/desk";
import { FlipCursor, cursorPos, type CursorKey } from "../lib/FlipCursor";
import { FloatFromPill } from "../lib/Float";
import { useStageBg } from "../lib/format";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { EASE, FONT_MONO, FONT_SANS, lerp } from "../lib/theme";
import { seg, useMs } from "../lib/time";
import {
  ClipboardStoryPanel,
  searchCenter,
  starCenter,
  type ClipboardStoryTimeline,
} from "./ClipboardStoryPanel";

/**
 * Paso 3 de la historia — Clipboard: el usuario selecciona el error de la terminal,
 * lo copia con Ctrl+C, abre el historial desde la pill, filtra por «error» y lo fija.
 */

/* ------------------------------- cronología (ms) ------------------------------- */

const DRAG_START_MS = 650; // empieza a arrastrar sobre la terminal
const DRAG_END_MS = 1250; // suelta: el error queda seleccionado
const COPY_MS = 1350; // Ctrl+C: destello + aviso «Copiado»
const TOAST_MS = 1000; // cuánto dura el aviso
const LEAVE_MS = 1600; // el puntero se va hacia la pill
const STRIP_OPEN_MS = 2150; // la tira de la pill se abre
const HOVER_CELL_MS = 2300; // el puntero llega a la celda Clipboard
const CLICK_CELL_MS = 2700;
const FLOAT_OPEN_MS = 2750; // nace el float desde la pill
const READY_MS = 3150; // el float ya responde al puntero
const STRIP_CLOSE_MS = 3200;
const PULSE_MS = 3250; // destello sobre la fila recién copiada
const SEARCH_CLICK_MS = 3750;
const TYPE_MS = 3850; // escribe «error» (5 letras)
const TYPE_STEP_MS = 90;
const PIN_CLICK_MS = 4800; // estrella de la fila del error

const TL: ClipboardStoryTimeline = {
  readyMs: READY_MS,
  pulseMs: PULSE_MS,
  searchClickMs: SEARCH_CLICK_MS,
  typeMs: TYPE_MS,
  typeStepMs: TYPE_STEP_MS,
  pinClickMs: PIN_CLICK_MS,
};

/* ------------------------- geometría de la terminal ------------------------- */

const EDITOR = { x: 14, y: 62, w: 412, h: 380 };
const TERM_H = 84;
const TERM_FONT = 8.6;
const TERM_LINE_H = TERM_FONT * 1.38;
// La terminal va pegada al fondo del editor (box-sizing: border-box).
const TERM_TOP = EDITOR.y + EDITOR.h - TERM_H;
// borde (1) + padding (6) + etiqueta TERMINAL (7.5 * 1.38) + margen (3)
const LINES_TOP = TERM_TOP + 1 + 6 + 7.5 * 1.38 + 3;
const TEXT_LEFT = EDITOR.x + 10;
/** Ancho de carácter estimado, solo para mapear el puntero a columnas (el resaltado usa `ch`). */
const CHAR_W = 5.04;

const ERR_LINE = 3; // «Error: connect ETIMEDOUT ...»
const AT_LINE = 4; // «at pool.query (...)»
const ERR_LEN = ERROR_LINES[ERR_LINE].t.length;
const AT_LEN = ERROR_LINES[AT_LINE].t.length;
const ERR_FROM = ERROR_LINES[ERR_LINE].t.search(/\S/);

const colX = (col: number) => TEXT_LEFT + col * CHAR_W;
const lineMidY = (line: number) => LINES_TOP + (line + 0.5) * TERM_LINE_H;

/* --------------------------------- puntero --------------------------------- */

const CX = SCREEN.w / 2;
const SEARCH = searchCenter(CX);
const STAR = starCenter(CX, 0);

const POINTER_KEYS: CursorKey[] = [
  { ms: 150, x: 420, y: 500 },
  { ms: 640, x: colX(ERR_FROM), y: lineMidY(ERR_LINE) },
  // Arrastre en diagonal: la selección sigue al puntero.
  { ms: DRAG_END_MS, x: colX(AT_LEN) + 1, y: lineMidY(AT_LINE), linear: true },
  { ms: LEAVE_MS, x: colX(AT_LEN) + 1, y: lineMidY(AT_LINE) },
  { ms: HOVER_CELL_MS, x: 135, y: 27 },
  { ms: FLOAT_OPEN_MS, x: 135, y: 27 },
  { ms: SEARCH_CLICK_MS - 200, x: SEARCH.x, y: SEARCH.y },
  { ms: 4300, x: SEARCH.x, y: SEARCH.y },
  { ms: 4700, x: STAR.x, y: STAR.y },
  { ms: 6000, x: STAR.x, y: STAR.y },
];
const POINTER_CLICKS = [DRAG_START_MS, CLICK_CELL_MS, SEARCH_CLICK_MS, PIN_CLICK_MS];

const pointerAt = (ms: number) => cursorPos(POINTER_KEYS, ms) ?? { x: -99, y: -99 };

/* ------------------------------ selección y aviso ------------------------------ */

type SelSpan = { line: number; from: number; to: number };

/** Tramos seleccionados según dónde está el puntero durante el arrastre. */
const selectionAt = (ms: number): SelSpan[] => {
  if (ms < DRAG_START_MS) return [];
  // Al soltar el botón la selección queda fija, aunque el puntero se vaya.
  const p = pointerAt(Math.min(ms, DRAG_END_MS));
  const line = p.y >= LINES_TOP + AT_LINE * TERM_LINE_H ? AT_LINE : ERR_LINE;
  const col = Math.round((p.x - TEXT_LEFT) / CHAR_W);
  if (line === ERR_LINE) {
    const to = Math.min(ERR_LEN, col);
    return to > ERR_FROM ? [{ line: ERR_LINE, from: ERR_FROM, to }] : [];
  }
  return [
    { line: ERR_LINE, from: ERR_FROM, to: ERR_LEN },
    { line: AT_LINE, from: 0, to: Math.max(0, Math.min(AT_LEN, col)) },
  ];
};

const TerminalSelection = ({ ms }: { ms: number }) => {
  const spans = selectionAt(ms);
  const copyT = seg(ms, COPY_MS, 320);
  const flash = copyT > 0 && copyT < 1 ? Math.sin(copyT * Math.PI) : 0;
  const background = `rgba(${lerp(79, 200, flash)}, ${lerp(140, 222, flash)}, 255, ${lerp(0.38, 0.8, flash)})`;
  return (
    <>
      {spans.map((s) => (
        <div
          key={s.line}
          style={{
            position: "absolute",
            left: `calc(${TEXT_LEFT}px + ${s.from}ch)`,
            top: LINES_TOP + s.line * TERM_LINE_H,
            width: `${s.to - s.from}ch`,
            height: TERM_LINE_H,
            fontFamily: FONT_MONO,
            fontSize: TERM_FONT,
            background,
            pointerEvents: "none",
          }}
        />
      ))}
    </>
  );
};

const keycapStyle = {
  minWidth: 13,
  height: 13,
  padding: "0 3px",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  borderRadius: 3,
  background: "#3b404b",
  boxShadow: "inset 0 0 0 1px rgb(255 255 255 / 12%)",
  fontSize: 7.5,
  fontWeight: 600,
} as const;

/** Aviso del sistema: «Ctrl C · Copiado». */
const CopiedToast = ({ ms }: { ms: number }) => {
  const inK = EASE.smoothOut(seg(ms, COPY_MS, 160));
  const outK = seg(ms, COPY_MS + TOAST_MS - 200, 200);
  const opacity = inK * (1 - outK);
  if (opacity <= 0) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: TEXT_LEFT,
        top: TERM_TOP - 28,
        display: "flex",
        alignItems: "center",
        gap: 5,
        padding: "4px 8px 4px 5px",
        borderRadius: 7,
        background: "#2a2e37",
        color: D.edText,
        fontFamily: FONT_SANS,
        fontSize: 9,
        fontWeight: 600,
        boxShadow: "0 6px 16px rgb(0 0 0 / 38%), 0 0 0 1px rgb(255 255 255 / 10%)",
        opacity,
        transform: `translateY(${(1 - inK) * 5}px)`,
        pointerEvents: "none",
      }}
    >
      <span style={{ display: "inline-flex", gap: 2 }}>
        <span style={keycapStyle}>Ctrl</span>
        <span style={keycapStyle}>C</span>
      </span>
      <svg width={10} height={10} viewBox="0 0 24 24" fill="none" stroke="#7ee787" strokeWidth={3} strokeLinecap="round" strokeLinejoin="round">
        <path d="M20 6 9 17l-5-5" />
      </svg>
      Copiado
    </div>
  );
};

/* ---------------------------------- escena ---------------------------------- */

export const ClipboardMoment = () => {
  const ms = useMs();

  // La tira ocupa 458 de 500 px: se acerca la cámara solo cuando ya se cerró.
  const zoomK = EASE.smoothOut(seg(ms, STRIP_CLOSE_MS - 100, 850));
  const camera = {
    z: interpolate(zoomK, [0, 1], [1, 1.2]),
    fx: CX,
    fy: 0,
    ax: CX,
    ay: 0,
  };

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Clipboard" sub="Lo que copias, a mano" keys={["Ctrl", "Shift", "V"]} />
      <Screen wallpaper={false} camera={camera}>
        <Desktop>
          <BrowserApp rect={{ x: 120, y: 250, w: 366, h: 360 }} />
          <EditorApp rect={EDITOR} termLines={ERROR_LINES} termH={TERM_H} highlightLine={10} />
          <TerminalSelection ms={ms} />
          <CopiedToast ms={ms} />
        </Desktop>

        <Notch
          cx={CX}
          toggles={[STRIP_OPEN_MS, STRIP_CLOSE_MS]}
          hovers={[{ index: 2, fromMs: HOVER_CELL_MS, toMs: STRIP_CLOSE_MS }]}
          lookX={interpolate(ms, [1900, 2300], [0, 0.9], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          })}
          lid={blinkLid(ms, [1450])}
        />

        <FloatFromPill
          ms={ms}
          startMs={FLOAT_OPEN_MS}
          cx={CX}
          pillBottom={PANEL_PILL_BOTTOM}
          w={CB.w}
          h={CB.h}
          radius={CB.radius}
        >
          {() => <ClipboardStoryPanel ms={ms} cx={CX} tl={TL} pointerAt={pointerAt} />}
        </FloatFromPill>

        <FlipCursor ms={ms} keys={POINTER_KEYS} clicks={POINTER_CLICKS} size={15} />
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos [desde, hasta] en ms que, seguidos, la cuentan en 1.6 s (≈ 1.5×):
 * selección + Ctrl+C, apertura desde la pill, filtro «error» + estrella.
 */
export const CLIPBOARD_SEGMENTS: [number, number][] = [
  [750, 1450],
  [2350, 3050],
  [3780, 4880],
];
export const CLIPBOARD_HERO_MS = 3900;
