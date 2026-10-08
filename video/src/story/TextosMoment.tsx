import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Caption, Keys } from "../lib/Caption";
import { FlipCursor, type CursorKey } from "../lib/FlipCursor";
import { FloatFromPill } from "../lib/Float";
import { useStageBg } from "../lib/format";
import { NOTCH_OPEN, Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { EASE } from "../lib/theme";
import { seg, useMs } from "../lib/time";
import { ChatApp, Desktop, EditorApp, ERROR_LINES } from "../desk/desk";
import {
  STORY_SNIPPETS,
  STORY_TEXTOS_H,
  STORY_TEXTOS_W,
  StoryTextosPanel,
  type StoryTabSwitch,
} from "./StoryTextosPanel";

/** Paso 6 de la historia: en el chat se pega «Respuesta incidente» desde Textos y se envía. */

/** Guion (ms de la escena). */
const TL = {
  stripAt: 400,
  hoverAt: 1000,
  openClickAt: 1300,
  floatAt: 1320,
  snippetsTabAt: 2100,
  pasteAt: 3000,
  /** El float se cierra al pegar: se acerca a la pill (90 ms) y se apaga (100 ms). */
  closeAt: 3120,
  textAt: 3320,
  enterAt: 4300,
  stripCloseAt: 3260,
  endCursorAt: 4700,
};

const PASTE_INDEX = 1;
const BODY = STORY_SNIPPETS[PASTE_INDEX].body;

const SWITCHES: StoryTabSwitch[] = [
  { at: -1, tab: "board" },
  { at: TL.snippetsTabAt, tab: "snippets" },
];

const FLOAT_LEFT = SCREEN.w / 2 - STORY_TEXTOS_W / 2;
const FLOAT_TOP = NOTCH_OPEN.h + 16;

/** Posiciones (lógicas de pantalla) de los blancos del puntero. */
const TARGET = {
  cellTextos: { x: SCREEN.w / 2 - NOTCH_OPEN.w / 2 + 3 * 46 + 22, y: 27 },
  tabSnippets: { x: FLOAT_LEFT + 93, y: FLOAT_TOP + 23 },
  snippet: { x: FLOAT_LEFT + 150, y: FLOAT_TOP + 165 },
  composer: { x: 330, y: 578 },
};

const CURSOR: CursorKey[] = [
  { ms: 500, x: 372, y: 250 },
  { ms: 1050, ...TARGET.cellTextos },
  { ms: 1700, ...TARGET.cellTextos },
  { ms: 2090, ...TARGET.tabSnippets },
  { ms: 2140, ...TARGET.tabSnippets },
  { ms: 2650, ...TARGET.snippet },
  { ms: 3140, ...TARGET.snippet },
  { ms: 3800, ...TARGET.composer },
];
const CLICKS = [TL.openClickAt, TL.snippetsTabAt, TL.pasteAt];

/** Ventanas del escritorio: el chat al frente y el editor con la prueba fallando detrás. */
const CHAT_RECT = { x: 60, y: 222, w: 426, h: 386 };
const EDITOR_RECT = { x: 14, y: 58, w: 340, h: 300 };

export const TextosMoment = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;

  const camera = { z: 1, fx: cx, fy: 0, ax: cx, ay: 0 };

  // Cierre del float al pegar (Svelte: approach 90 ms + fade 100 ms).
  const approach = EASE.smoothOut(seg(ms, TL.closeAt, 90));
  const dismiss = seg(ms, TL.closeAt + 90, 100);
  const floatOpen = ms < TL.closeAt + 190;

  const sent = ms >= TL.enterAt;
  const pasted = ms >= TL.textAt && !sent;
  const caretBlink = Math.floor(ms / 530) % 2 === 0;
  const showCaret = ms < TL.openClickAt || (ms >= TL.textAt && ms < TL.enterAt + 200) ? caretBlink : false;

  const enterChip = seg(ms, TL.textAt + 500, 200) * (1 - seg(ms, TL.enterAt + 60, 160));
  const press = seg(ms, TL.enterAt - 40, 60) * (1 - seg(ms, TL.enterAt + 20, 120));

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Textos" sub="Tus textos, listos para pegar" keys={["Ctrl", "Shift", "S"]} />
      <Screen wallpaper={false} camera={camera}>
        <Desktop>
          <EditorApp rect={EDITOR_RECT} termLines={ERROR_LINES} termH={92} />
          <ChatApp rect={CHAT_RECT} composer={pasted ? BODY : ""} showCaret={showCaret} extraMessage={sent ? BODY : undefined} />
        </Desktop>

        {floatOpen && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              opacity: 1 - dismiss,
              transform: `translateY(${-14 * approach}px)`,
              pointerEvents: "none",
            }}
          >
            <FloatFromPill
              ms={ms}
              startMs={TL.floatAt}
              cx={cx}
              pillBottom={NOTCH_OPEN.h}
              w={STORY_TEXTOS_W}
              h={STORY_TEXTOS_H}
              radius={20}
            >
              {() => <StoryTextosPanel ms={ms} switches={SWITCHES} pasteIndex={PASTE_INDEX} pastedAt={TL.pasteAt} />}
            </FloatFromPill>
          </div>
        )}

        {/* Enter (capa del video): avisa que el mensaje se envía */}
        <div
          style={{
            position: "absolute",
            left: 0,
            width: SCREEN.w,
            top: 528,
            display: "flex",
            justifyContent: "flex-end",
            paddingRight: 34,
            boxSizing: "border-box",
            opacity: enterChip,
            transform: `scale(${1 - 0.06 * press}) translateY(${(1 - enterChip) * 5}px)`,
          }}
        >
          <Keys keys={["Enter"]} size={0.4} />
        </div>

        <Notch
          cx={cx}
          toggles={[TL.stripAt, TL.stripCloseAt]}
          hovers={[{ index: 3, fromMs: TL.hoverAt, toMs: TL.closeAt }]}
          activeIndex={ms >= TL.openClickAt && ms < TL.closeAt ? 3 : null}
          lookX={interpolate(ms, [700, 1100], [0, -0.9], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
          lid={blinkLid(ms, [2600])}
        />

        <FlipCursor ms={ms} keys={CURSOR} clicks={CLICKS} size={13} fadeOutAt={TL.endCursorAt} />
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos (ms de la escena) para el montaje: 1.6 s a ≈1.4×.
 * Tira + hover + float / pestaña Textos y elegir / pegar / Enter y mensaje enviado.
 */
export const TEXTOS_SEGMENTS: [number, number][] = [
  [1000, 1600],
  [2050, 2650],
  [2850, 3450],
  [4200, 4600],
];

/** Float con la lista de textos y el cursor sobre «Respuesta incidente». */
export const TEXTOS_HERO_MS = 2700;
