import { AbsoluteFill, Easing } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import { Notch, NOTCH_OPEN } from "../lib/Notch";
import { DESK, DESK_GEO, PizarraDesk } from "../lib/PizarraDesk";
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
import { ArrowCursor, ArrowShape, Crosshair, EllipseShape, PenShape, TextShape } from "../lib/PizarraMarks";
import { Screen } from "../lib/Screen";
import { C, FONT_SANS } from "../lib/theme";
import { seg, useMs } from "../lib/time";
import { typed } from "../lib/ui";

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
  helpFrom: 1330,
  barIn: 1270,
  /** Primer trazo: la ayuda se va. */
  firstStroke: 1700,
  /** Esc, con trazos: el botón X pide confirmar. */
  discard: 5200,
  /** Segundo Esc: la ventana desaparece de golpe. */
  close: 5500,
} as const;

const BAR_TOP = 14;
const TEXT = "¿Por qué baja?";
const TEXT_AT: Pt = [118, 268];
const STROKE_W = 4;

const ARROW = { t0: 1700, t1: 2100, from: [330, 262] as Pt, to: [286, 398] as Pt };
const CIRCLE = { t0: 2630, t1: 3090, from: [196, 116] as Pt, to: [372, 220] as Pt };
const PEN = { t0: 3340, t1: 3820 };
const TEXT_TIMES = { click: 4410, typeFrom: 4470, msPerChar: 30, enter: 4950 };

/** Lápiz: subrayado a mano alzada bajo el pedido retrasado (funciones analíticas de u). */
const penAt = (u: number): Pt => [
  62 + 236 * u + 2 * Math.sin(u * 23),
  610 + 3 * Math.sin(u * Math.PI * 2 * 3.2) + 0.9 * Math.sin(u * 41),
];

/* ---------- cursor ---------- */

const travel = Easing.bezier(0.3, 0, 0.2, 1);
const dragEase = Easing.inOut(Easing.quad);
const lerpPt = (a: Pt, b: Pt, u: number): Pt => [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u];

const L0 = DESK.cx - barWidth(false) / 2;
const R0 = barRects(false);
const toDesk = (r: Rect): Pt => {
  const c = center(r);
  return [L0 + c[0], BAR_TOP + c[1]];
};
const STYLE_BTN = toDesk(R0.style);
const SW_BLUE = toDesk(R0.swatch(3));
const SW_YELLOW = toDesk(R0.swatch(1));
const PILL_CELL: Pt = [DESK.cx + 115, 24];

type Move = { t0: number; t1: number; to: Pt; ease: (u: number) => number; pos?: (u: number) => Pt };

const MOVES: Move[] = [
  { t0: T.cursorIn, t1: 220, to: PILL_CELL, ease: travel },
  { t0: 860, t1: 1200, to: [430, 170], ease: travel },
  { t0: 1280, t1: 1500, to: ARROW.from, ease: travel },
  { t0: ARROW.t0, t1: ARROW.t1, to: ARROW.to, ease: dragEase },
  { t0: 2120, t1: 2240, to: STYLE_BTN, ease: travel },
  { t0: 2260, t1: 2360, to: SW_BLUE, ease: travel },
  { t0: 2450, t1: 2610, to: CIRCLE.from, ease: travel },
  { t0: CIRCLE.t0, t1: CIRCLE.t1, to: CIRCLE.to, ease: dragEase },
  { t0: 3110, t1: 3320, to: penAt(0), ease: travel },
  { t0: PEN.t0, t1: PEN.t1, to: penAt(1), ease: dragEase, pos: penAt },
  { t0: 3840, t1: 3990, to: STYLE_BTN, ease: travel },
  { t0: 4010, t1: 4110, to: SW_YELLOW, ease: travel },
  { t0: 4200, t1: 4390, to: TEXT_AT, ease: travel },
];
const CURSOR_START: Pt = [452, 96];

const cursorAt = (ms: number): Pt => {
  let prev = CURSOR_START;
  for (const m of MOVES) {
    if (ms >= m.t1) {
      prev = m.to;
      continue;
    }
    if (ms >= m.t0) {
      const u = m.ease(seg(ms, m.t0, m.t1 - m.t0));
      return m.pos ? m.pos(u) : lerpPt(prev, m.to, u);
    }
    break;
  }
  return prev;
};

/* ---------- estado de la barra ---------- */

const POPOVERS: [number, number][] = [
  [2250, 2450],
  [4000, 4200],
];
const PRESSES: [string, number][] = [
  ["style", 2250],
  ["swatch:3", 2380],
  ["style", 4000],
  ["swatch:1", 4130],
];

const toolAt = (ms: number): ToolId =>
  ms >= 4140 ? "text" : ms >= 3120 ? "pen" : ms >= 2390 ? "ellipse" : "arrow";
const colorIndexAt = (ms: number) => (ms >= 4130 ? 1 : ms >= 2380 ? 3 : 0);

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

export const PizarraScene = () => {
  const ms = useMs();
  const cx = DESK.cx;

  const frozen = ms >= T.freeze && ms < T.close;
  const liveMs = frozen ? T.freeze : ms;
  const cursor = cursorAt(ms);
  const discard = ms >= T.discard && ms < T.close;

  /* formas en curso */
  const arrowU = dragEase(seg(ms, ARROW.t0, ARROW.t1 - ARROW.t0));
  const circleU = dragEase(seg(ms, CIRCLE.t0, CIRCLE.t1 - CIRCLE.t0));
  const penU = dragEase(seg(ms, PEN.t0, PEN.t1 - PEN.t0));
  const penPoints: Pt[] = [];
  if (ms >= PEN.t0) {
    const n = 90;
    for (let i = 0; i <= Math.floor(penU * n); i++) penPoints.push(penAt(i / n));
    penPoints.push(penAt(penU));
  }

  const textShown = ms >= TEXT_TIMES.click ? typed(TEXT, ms, TEXT_TIMES.typeFrom, TEXT_TIMES.msPerChar) : "";
  const textEditing = ms >= TEXT_TIMES.click && ms < TEXT_TIMES.enter;
  const caretOn = Math.floor((ms - TEXT_TIMES.click) / 530) % 2 === 0 || ms >= TEXT_TIMES.typeFrom;

  const hasStrokes = ms >= ARROW.t1;
  const popover = POPOVERS.find(([a, b]) => ms >= a && ms < b);
  const pressed = PRESSES.find(([, t]) => ms >= t && ms < t + 75)?.[0] ?? null;
  const colorIndex = colorIndexAt(ms);
  const width = barWidth(discard);
  const barLeft = cx - width / 2;
  const barLocal: Pt = [cursor[0] - barLeft, cursor[1] - BAR_TOP];
  const overBar =
    inRect(barLocal, { x: 0, y: 0, w: width, h: BAR_H }) ||
    (popover !== undefined && inRect(barLocal, barRects(discard).popover));

  const barOpacity = frozen ? seg(ms, T.barIn, 100) : 0;
  const helpOpacity = seg(ms, T.helpFrom, 125) * (1 - seg(ms, T.firstStroke, 125));
  const showCrosshair = frozen && !overBar;
  const cursorOpacity = seg(ms, T.cursorIn - 20, 120);

  const camera = { z: DESK.zoom, fx: 0, fy: 0, ax: 0, ay: 0 };

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Pizarra" sub="Marca la pantalla, ahí donde está" keys={["Ctrl", "Shift", "X"]} />
      <Screen camera={camera} wallpaper={false}>
        <PizarraDesk liveMs={liveMs} />

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
            width={DESK.w}
            height={DESK.h}
            style={{ position: "absolute", left: 0, top: 0, overflow: "visible" }}
          >
            {ms >= ARROW.t0 && (
              <ArrowShape
                from={ARROW.from}
                to={lerpPt(ARROW.from, ARROW.to, arrowU)}
                color={STROKE_COLORS[0].hex}
                width={STROKE_W}
              />
            )}
            {ms >= CIRCLE.t0 && (
              <EllipseShape
                from={CIRCLE.from}
                to={lerpPt(CIRCLE.from, CIRCLE.to, circleU)}
                color={STROKE_COLORS[3].hex}
                width={STROKE_W}
              />
            )}
            {ms >= PEN.t0 && <PenShape points={penPoints} color={STROKE_COLORS[3].hex} width={STROKE_W} />}
          </svg>
        )}

        {frozen && ms >= TEXT_TIMES.click ? (
          <TextShape
            at={TEXT_AT}
            text={textShown}
            color={STROKE_COLORS[1].hex}
            halo={STROKE_COLORS[1].halo}
            editing={textEditing}
            caret={caretOn}
          />
        ) : null}

        {frozen && helpOpacity > 0 && (
          <div
            style={{
              position: "absolute",
              left: cx,
              top: DESK_GEO.taskbarTop - 14,
              transform: "translate(-50%, -100%)",
            }}
          >
            <PizarraChip opacity={helpOpacity}>
              Arrastra para dibujar · Enter copia · Ctrl+Enter guarda · Esc cierra
            </PizarraChip>
          </div>
        )}

        {barOpacity > 0 && (
          <div
            style={{
              position: "absolute",
              left: barLeft,
              top: BAR_TOP,
              opacity: barOpacity,
            }}
          >
            <PizarraBar
              tool={toolAt(ms)}
              colorIndex={colorIndex}
              popoverOpen={popover !== undefined}
              popoverAgeMs={popover ? ms - popover[0] : 999}
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
