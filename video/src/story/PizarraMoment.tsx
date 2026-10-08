import { AbsoluteFill, Easing } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Caption } from "../lib/Caption";
import { useStageBg } from "../lib/format";
import { Notch, NOTCH_OPEN } from "../lib/Notch";
import {
  BAR_H,
  PizarraBar,
  PizarraChip,
  STROKE_COLORS,
  barRects,
  barWidth,
  center,
  inRect,
  type Pt,
  type Rect,
  type ToolId,
} from "../lib/PizarraBar";
import { ArrowCursor, ArrowShape, Crosshair, EllipseShape, TextShape } from "../lib/PizarraMarks";
import { Screen } from "../lib/Screen";
import { C, FONT_SANS } from "../lib/theme";
import { seg, useMs } from "../lib/time";
import { typed } from "../lib/ui";
import { chartGeometry } from "../desk/desk";
import { BROWSER, PizarraStoryDesk, STORY_DESK, TASKBAR_TOP, browserToDesk } from "./PizarraStoryDesk";

/**
 * Paso 4 de la historia: el p95 se dispara a las 14:02. Ctrl+Shift+X congela el navegador,
 * se rodea el pico con un círculo, una flecha lo señala y el texto pregunta «¿Desde cuándo?».
 */

/* ---------- guion (ms) ---------- */

const T = {
  stripOpen: 60,
  stripClose: 880,
  cursorIn: 60,
  hoverFrom: 220,
  hoverTo: 880,
  tipFrom: 670,
  /** Ctrl + Shift + X: la pantalla queda congelada. */
  freeze: 1250,
  barIn: 1270,
  helpFrom: 1330,
  /** Primer trazo (el círculo): la ayuda se va. */
  firstStroke: 1740,
  /** Primer Esc, con trazos: el botón X pide confirmar. */
  discard: 4900,
  /** Segundo Esc: la ventana desaparece de golpe. */
  close: 5200,
} as const;

const BAR_TOP = 14;
const TEXT = "¿Desde cuándo?";
const STROKE_W = 4;

/** Herramientas de la barra que se pulsan (índices de `TOOLS`). */
const TOOL = { arrow: 1, ellipse: 2, text: 5 } as const;

/* ---------- geometría de las marcas (sale de la misma del gráfico) ---------- */

const CHART = chartGeometry(BROWSER.w);
/** Elipse alrededor del pico y del punto siguiente (980 y 940 ms). */
const CIRCLE_CENTER = browserToDesk(CHART.spike[0] + 5, CHART.spike[1] + 14);
const CIRCLE_R: Pt = [34 * BROWSER.scale, 30 * BROWSER.scale];
const CIRCLE = {
  t0: 1740,
  t1: 2200,
  from: [CIRCLE_CENTER[0] - CIRCLE_R[0], CIRCLE_CENTER[1] - CIRCLE_R[1]] as Pt,
  to: [CIRCLE_CENTER[0] + CIRCLE_R[0], CIRCLE_CENTER[1] + CIRCLE_R[1]] as Pt,
};

const TEXT_AT: Pt = [52, 408];
/** La flecha arranca junto al texto y termina 8 px antes del borde del círculo. */
const ARROW_FROM: Pt = [262, 426];
const ARROW_TO: Pt = (() => {
  const dx = CIRCLE_CENTER[0] - ARROW_FROM[0];
  const dy = CIRCLE_CENTER[1] - ARROW_FROM[1];
  const len = Math.hypot(dx, dy);
  const ux = dx / len;
  const uy = dy / len;
  const edge = 1 / Math.hypot(ux / CIRCLE_R[0], uy / CIRCLE_R[1]);
  return [CIRCLE_CENTER[0] - ux * (edge + 8), CIRCLE_CENTER[1] - uy * (edge + 8)];
})();
const ARROW = { t0: 2620, t1: 2980, from: ARROW_FROM, to: ARROW_TO };
const TEXT_TIMES = { click: 3440, typeFrom: 3500, msPerChar: 38, enter: 4400 };

/* ---------- cursor ---------- */

const travel = Easing.bezier(0.3, 0, 0.2, 1);
const dragEase = Easing.inOut(Easing.quad);
const lerpPt = (a: Pt, b: Pt, u: number): Pt => [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];

const L0 = STORY_DESK.cx - barWidth(false) / 2;
const R0 = barRects(false);
const toDesk = (r: Rect): Pt => {
  const c = center(r);
  return [L0 + c[0], BAR_TOP + c[1]];
};
const toolButton = (i: number) => toDesk(R0.tool(i));
/** Celda de Pizarra en la tira de la pill (índice 7). */
const PILL_CELL: Pt = [STORY_DESK.cx + 115, 24];

type Move = { t0: number; t1: number; to: Pt; ease: (u: number) => number };

const MOVES: Move[] = [
  { t0: T.cursorIn, t1: 220, to: PILL_CELL, ease: travel },
  { t0: 860, t1: 1200, to: [410, 170], ease: travel },
  { t0: 1290, t1: 1460, to: toolButton(TOOL.ellipse), ease: travel },
  { t0: 1540, t1: 1700, to: CIRCLE.from, ease: travel },
  { t0: CIRCLE.t0, t1: CIRCLE.t1, to: CIRCLE.to, ease: dragEase },
  { t0: 2240, t1: 2380, to: toolButton(TOOL.arrow), ease: travel },
  { t0: 2450, t1: 2590, to: ARROW.from, ease: travel },
  { t0: ARROW.t0, t1: ARROW.t1, to: ARROW.to, ease: dragEase },
  { t0: 3020, t1: 3180, to: toolButton(TOOL.text), ease: travel },
  { t0: 3250, t1: 3420, to: TEXT_AT, ease: travel },
  { t0: 4080, t1: 4300, to: [250, 505], ease: travel },
];
const CURSOR_START: Pt = [452, 96];

const cursorAt = (ms: number): Pt => {
  let prev = CURSOR_START;
  for (const m of MOVES) {
    if (ms >= m.t1) {
      prev = m.to;
      continue;
    }
    if (ms >= m.t0) return lerpPt(prev, m.to, m.ease(seg(ms, m.t0, m.t1 - m.t0)));
    break;
  }
  return prev;
};

/* ---------- estado de la barra ---------- */

const PRESSES: [string, number][] = [
  [`tool:${TOOL.ellipse}`, 1480],
  [`tool:${TOOL.arrow}`, 2400],
  [`tool:${TOOL.text}`, 3200],
];

/** Flecha por defecto; luego cada clic en la barra cambia la herramienta. */
const toolAt = (ms: number): ToolId => (ms >= 3210 ? "text" : ms >= 2410 ? "arrow" : ms >= 1490 ? "ellipse" : "arrow");

const Tip = ({ ms }: { ms: number }) => {
  const t = Math.min(seg(ms, T.tipFrom, 125), 1 - seg(ms, T.stripClose, 125));
  if (t <= 0) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: PILL_CELL[0],
        top: NOTCH_OPEN.h + 8,
        transform: "translateX(-50%)",
        padding: "4.2px 7.4px",
        borderRadius: 6.4,
        border: `1px solid rgb(240 240 234 / 8%)`,
        background: "#191917",
        boxShadow: "0 8px 22px rgb(0 0 0 / 32%)",
        fontFamily: FONT_SANS,
        fontSize: 11.5,
        fontWeight: 500,
        lineHeight: 1.3,
        color: C.text,
        whiteSpace: "nowrap",
        opacity: Math.max(0, t),
      }}
    >
      Pizarra — Marcar la pantalla
    </div>
  );
};

export const PizarraMoment = () => {
  const ms = useMs();
  const cx = STORY_DESK.cx;

  const frozen = ms >= T.freeze && ms < T.close;
  const liveMs = frozen ? T.freeze : ms;
  const cursor = cursorAt(ms);
  const discard = ms >= T.discard && ms < T.close;

  /* formas en curso */
  const circleU = dragEase(seg(ms, CIRCLE.t0, CIRCLE.t1 - CIRCLE.t0));
  const arrowU = dragEase(seg(ms, ARROW.t0, ARROW.t1 - ARROW.t0));

  const textShown = ms >= TEXT_TIMES.click ? typed(TEXT, ms, TEXT_TIMES.typeFrom, TEXT_TIMES.msPerChar) : "";
  const textEditing = ms >= TEXT_TIMES.click && ms < TEXT_TIMES.enter;
  const caretOn = Math.floor((ms - TEXT_TIMES.click) / 530) % 2 === 0 || ms >= TEXT_TIMES.typeFrom;

  const hasStrokes = ms >= CIRCLE.t1;
  const pressed = PRESSES.find(([, t]) => ms >= t && ms < t + 75)?.[0] ?? null;
  const width = barWidth(discard);
  const barLeft = cx - width / 2;
  const barLocal: Pt = [cursor[0] - barLeft, cursor[1] - BAR_TOP];
  const overBar = inRect(barLocal, { x: 0, y: 0, w: width, h: BAR_H });

  const barOpacity = frozen ? seg(ms, T.barIn, 100) : 0;
  const helpOpacity = seg(ms, T.helpFrom, 125) * (1 - seg(ms, T.firstStroke, 125));
  const showCrosshair = frozen && !overBar;
  const cursorOpacity = seg(ms, T.cursorIn - 20, 120);

  const camera = { z: STORY_DESK.zoom, fx: 0, fy: 0, ax: 0, ay: 0 };

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Pizarra" sub="Marca la pantalla, ahí donde está" keys={["Ctrl", "Shift", "X"]} />
      <Screen camera={camera} wallpaper={false}>
        <PizarraStoryDesk liveMs={liveMs} />

        <Notch
          cx={cx}
          toggles={[T.stripOpen, T.stripClose]}
          hovers={[{ index: 7, fromMs: T.hoverFrom, toMs: T.hoverTo }]}
          lookX={Math.max(-1, Math.min(1, (cursor[0] - cx) / 160)) * 0.9}
          lookY={Math.max(0, Math.min(1, (cursor[1] - 20) / 240)) * 0.7}
          lid={blinkLid(ms, [620])}
        />

        <Tip ms={ms} />

        {frozen && (
          <svg
            width={STORY_DESK.w}
            height={STORY_DESK.h}
            style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}
          >
            {ms >= CIRCLE.t0 && (
              <EllipseShape
                from={CIRCLE.from}
                to={lerpPt(CIRCLE.from, CIRCLE.to, circleU)}
                color={STROKE_COLORS[0].hex}
                width={STROKE_W}
              />
            )}
            {ms >= ARROW.t0 && (
              <ArrowShape
                from={ARROW.from}
                to={lerpPt(ARROW.from, ARROW.to, arrowU)}
                color={STROKE_COLORS[0].hex}
                width={STROKE_W}
              />
            )}
          </svg>
        )}

        {frozen && ms >= TEXT_TIMES.click ? (
          <TextShape
            at={TEXT_AT}
            text={textShown}
            color={STROKE_COLORS[0].hex}
            halo={STROKE_COLORS[0].halo}
            editing={textEditing}
            caret={caretOn}
          />
        ) : null}

        {frozen && helpOpacity > 0 && (
          <div
            style={{
              position: "absolute",
              left: cx,
              top: TASKBAR_TOP - 14,
              transform: "translate(-50%, -100%)",
            }}
          >
            <PizarraChip opacity={helpOpacity}>
              Arrastra para dibujar · Enter copia · Ctrl+Enter guarda · Esc cierra
            </PizarraChip>
          </div>
        )}

        {barOpacity > 0 && (
          <div style={{ position: "absolute", left: barLeft, top: BAR_TOP, opacity: barOpacity }}>
            <PizarraBar
              tool={toolAt(ms)}
              colorIndex={0}
              hasStrokes={hasStrokes}
              discard={discard}
              cursor={barLocal}
              pressed={pressed}
            />
          </div>
        )}

        <div style={{ opacity: cursorOpacity }}>
          {showCrosshair ? <Crosshair x={cursor[0]} y={cursor[1]} /> : <ArrowCursor x={cursor[0]} y={cursor[1]} />}
        </div>
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos de la escena (ms) que, seguidos, la cuentan en 1.6 s (≈1.5×):
 * congelado y barra, círculo, flecha y el texto escrito hasta confirmarse.
 */
export const PIZARRA_SEGMENTS: [number, number][] = [
  [1100, 1560],
  [1700, 2220],
  [2560, 3000],
  [3420, 4460],
];
/** Todas las marcas ya confirmadas: círculo, flecha y «¿Desde cuándo?». */
export const PIZARRA_HERO_MS = 4460;
