import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import { FlipCursor, type CursorKey } from "../lib/FlipCursor";
import { FloatFromPill } from "../lib/Float";
import { NOTCH_OPEN, Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { SNIPPETS, TEXTOS_H, TEXTOS_W, TextosPanel, type TabSwitch } from "../lib/TextosPanel";
import { C, EASE, FONT_SANS } from "../lib/theme";
import { seg, useMs } from "../lib/time";

/** Guion (ms). El clic en la celda Textos dispara el float; luego una pestaña cada ~1 s. */
const TL = {
  stripAt: 300,
  hoverAt: 1000,
  openClickAt: 1250,
  floatAt: 1270,
  snippetsTabAt: 1950,
  pasteAt: 2750,
  notesTabAt: 3250,
  notesTypeAt: 3500,
  boardTabAt: 4850,
  hoverPageAt: 5250,
};

const SWITCHES: TabSwitch[] = [
  { at: -1, tab: "board" },
  { at: TL.snippetsTabAt, tab: "snippets" },
  { at: TL.notesTabAt, tab: "notes" },
  { at: TL.boardTabAt, tab: "board" },
];

const PASTE_INDEX = 1;
const FLOAT_LEFT = SCREEN.w / 2 - TEXTOS_W / 2;
const FLOAT_TOP = NOTCH_OPEN.h + 16;

/** Posiciones (lógicas de pantalla) de los blancos del puntero. */
const TARGET = {
  cellTextos: { x: SCREEN.w / 2 - NOTCH_OPEN.w / 2 + 3 * 46 + 22, y: 27 },
  tabBoard: { x: FLOAT_LEFT + 37, y: FLOAT_TOP + 23 },
  tabSnippets: { x: FLOAT_LEFT + 93, y: FLOAT_TOP + 23 },
  tabNotes: { x: FLOAT_LEFT + 148, y: FLOAT_TOP + 23 },
  snippet: { x: FLOAT_LEFT + 150, y: FLOAT_TOP + 140 },
  page: { x: FLOAT_LEFT + 290, y: FLOAT_TOP + 84 },
};

const CURSOR: CursorKey[] = [
  { ms: 500, x: 330, y: 150 },
  { ms: 1050, ...TARGET.cellTextos },
  { ms: 1700, ...TARGET.cellTextos },
  { ms: 1940, ...TARGET.tabSnippets },
  { ms: 2400, ...TARGET.snippet },
  { ms: 2740, ...TARGET.snippet },
  { ms: 3240, ...TARGET.tabNotes },
  { ms: 4200, x: TARGET.tabNotes.x + 30, y: TARGET.tabNotes.y + 60 },
  { ms: 4840, ...TARGET.tabBoard },
  { ms: 5230, ...TARGET.page },
];
const CLICKS = [TL.openClickAt, TL.snippetsTabAt, TL.pasteAt, TL.notesTabAt, TL.boardTabAt];

/** App de destino del pegado: cuadro de respuesta genérico detrás del float. */
const ReplyBox = ({ ms }: { ms: number }) => {
  const body = SNIPPETS[PASTE_INDEX].body;
  const pasted = seg(ms, TL.pasteAt, 120);
  return (
    <div
      style={{
        position: "absolute",
        left: 52,
        top: 456,
        width: 396,
        height: 78,
        boxSizing: "border-box",
        borderRadius: 10,
        background: "rgb(255 255 255 / 92%)",
        boxShadow: "0 6px 20px rgb(20 24 30 / 14%)",
        padding: "9px 12px",
        fontFamily: FONT_SANS,
      }}
    >
      <div style={{ fontSize: 9.5, fontWeight: 600, color: "#7b838b", letterSpacing: "0.04em", textTransform: "uppercase" }}>
        Responder al cliente
      </div>
      <div style={{ position: "relative", marginTop: 6, fontSize: 12, lineHeight: 1.4, color: "#23272b" }}>
        <span style={{ color: "#9aa1a8", opacity: pasted > 0 ? 0 : 1 }}>Escribe tu respuesta…</span>
        <span style={{ position: "absolute", left: 0, top: 0, opacity: pasted }}>{body}</span>
      </div>
    </div>
  );
};

export const TextosScene = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;

  const z = interpolate(EASE.smoothOut(seg(ms, 1100, 3600)), [0, 1], [1, 1.05]);
  const camera = { z, fx: cx, fy: 0, ax: cx, ay: 0 };

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Textos" sub="Tus textos, listos para pegar" keys={["Ctrl", "Shift", "S"]} />
      <Screen camera={camera}>
        <ReplyBox ms={ms} />

        <FloatFromPill
          ms={ms}
          startMs={TL.floatAt}
          cx={cx}
          pillBottom={NOTCH_OPEN.h}
          w={TEXTOS_W}
          h={TEXTOS_H}
          radius={20}
        >
          {() => (
            <TextosPanel
              ms={ms}
              switches={SWITCHES}
              pasteIndex={PASTE_INDEX}
              pastedAt={TL.pasteAt}
              notesTypeAt={TL.notesTypeAt}
              hoverPage={{ index: 0, fromMs: TL.hoverPageAt }}
            />
          )}
        </FloatFromPill>

        <Notch
          cx={cx}
          toggles={[TL.stripAt]}
          hovers={[{ index: 3, fromMs: TL.hoverAt }]}
          activeIndex={ms >= TL.openClickAt ? 3 : null}
          lookX={interpolate(ms, [700, 1100], [0, -0.9], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
          lid={blinkLid(ms, [2600])}
        />

        <FlipCursor ms={ms} keys={CURSOR} clicks={CLICKS} size={13} />
      </Screen>
    </AbsoluteFill>
  );
};
