import type { CSSProperties } from "react";
import { AbsoluteFill, Easing, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Caption, Keys } from "../lib/Caption";
import { FlipCursor, type CursorKey } from "../lib/FlipCursor";
import { useStageBg } from "../lib/format";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { seg, useMs } from "../lib/time";
import { Desktop, EditorApp, ERROR_LINES } from "../desk/desk";
import { STORY_CARD, StoryFlipBack, type StoryFlipTL } from "./StoryFlipBack";
import { FLIP_WINDOW, StoryFlipFront } from "./StoryFlipFront";

/** Paso 7 de la historia: Ctrl+Shift+B da vuelta el navegador; en el reverso se anota y se inserta el error. */

const CARD = STORY_CARD;

/** Volteo 3D (WindowFlipSurface.svelte): ida 400 ms, curvas por tramo dentro de los keyframes. */
const FLIP_IN_MS = 400;
const FLIP_AT = 850;
const ENTER = Easing.bezier(0.45, 0, 0.725, 0.5);
const EXIT = Easing.bezier(0.275, 0.5, 0.55, 1);
const CAMERA_DEPTH = 4.5; // en anchos de tarjeta
const SUNK = 0.5; // hundido a 90°, en anchos de tarjeta

const flipPose = (t: number) => {
  const sunk = SUNK * CARD.w;
  if (t <= 0.5) {
    const u = ENTER(t / 0.5);
    return { rot: 90 * u, z: -sunk * u };
  }
  const v = EXIT((t - 0.5) / 0.5);
  return { rot: 90 + 90 * v, z: -sunk * (1 - v) };
};

/** Guion del reverso (ms de la escena). */
const TL: StoryFlipTL = {
  emptyAt: 1250,
  cursorInAt: 1250,
  textToolAt: 1600,
  noteAt: 1850,
  noteTypeAt: 1950,
  note2At: 2500,
  note2TypeAt: 2600,
  noteMsPerChar: 16,
  pencilToolAt: 3200,
  paletteOffAt: 3650,
  drawAt: 3500,
  drawMs: 520,
  drawerAt: 4300,
  previewAt: 4700,
  addAt: 5100,
  saved: [
    { from: 3050, to: 3150 },
    { from: 4100, to: 4200 },
    { from: 5250, to: 6500 },
  ],
};

const PULL_BACK_AT = 5450;
const PULL_BACK_MS = 600;
const CHIP_IN_AT = 350;

/** Escala real → lógica y posición de la ventana en el escritorio (la misma que ocupa el navegador). */
const S = FLIP_WINDOW.w / CARD.w;
const CARD_L = (SCREEN.w - FLIP_WINDOW.w) / 2;
const CARD_T = 62;

/** Cámara: `rx, ry` son los px reales de la tarjeta que quedan en el centro de la pantalla. */
type CamKey = { ms: number; z: number; rx: number; ry: number };
const HOME: CamKey = { ms: 0, z: 1, rx: SCREEN.w / 2 / S - CARD_L / S, ry: SCREEN.h / 2 / S - CARD_T / S };
const CAM: CamKey[] = [
  HOME,
  { ...HOME, ms: 1250 },
  { ms: 1900, z: 2.2, rx: 620, ry: 400 },
  { ms: 4000, z: 2.2, rx: 620, ry: 400 },
  { ms: 4300, z: 2.2, rx: 960, ry: 400 },
  { ms: 4700, z: 2.2, rx: 960, ry: 400 },
  { ms: 5000, z: 2.2, rx: 620, ry: 400 },
  { ms: PULL_BACK_AT, z: 2.2, rx: 620, ry: 400 },
  { ...HOME, ms: PULL_BACK_AT + PULL_BACK_MS },
];
const CAM_EASE = Easing.bezier(0.45, 0, 0.2, 1);
const cameraAt = (ms: number) => {
  let i = 0;
  while (i < CAM.length - 1 && ms >= CAM[i + 1].ms) i++;
  const a = CAM[i];
  const b = CAM[Math.min(i + 1, CAM.length - 1)];
  const k = a === b ? 0 : CAM_EASE(seg(ms, a.ms, b.ms - a.ms));
  return { z: a.z + (b.z - a.z) * k, rx: a.rx + (b.rx - a.rx) * k, ry: a.ry + (b.ry - a.ry) * k };
};

/** Puntero del escritorio (coordenadas lógicas): busca el pico y se desvanece al voltear. */
const SPIKE = { x: CARD_L + 321, y: CARD_T + 24 + 138 };
const DESK_CURSOR: CursorKey[] = [
  { ms: 250, x: 420, y: 330 },
  { ms: 750, x: SPIKE.x + 10, y: SPIKE.y + 22 },
];

const FACE: CSSProperties = {
  position: "absolute",
  inset: 0,
  overflow: "hidden",
  borderRadius: 8 / S,
  backfaceVisibility: "hidden",
  boxShadow: `0 ${18 / S}px ${44 / S}px rgb(15 25 55 / 34%), 0 0 0 ${1 / S}px rgb(15 25 55 / 18%)`,
};

export const FlipboardMoment = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;
  const t = seg(ms, FLIP_AT, FLIP_IN_MS);
  const { rot, z } = flipPose(t);

  const cam = cameraAt(ms);
  const camera = {
    z: cam.z,
    fx: CARD_L + cam.rx * S,
    fy: CARD_T + cam.ry * S,
    ax: SCREEN.w / 2,
    ay: SCREEN.h / 2,
  };

  // Chip del atajo (capa del video): aparece antes del giro y se va al terminar.
  const chipIn = seg(ms, CHIP_IN_AT, 200);
  const chipOut = 1 - seg(ms, FLIP_AT + 300, 200);
  const press = seg(ms, FLIP_AT - 60, 60) * (1 - seg(ms, FLIP_AT, 120));

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Flipboard" sub="Da vuelta la ventana y anota" keys={["Ctrl", "Shift", "B"]} />
      <Screen wallpaper={false} camera={camera}>
        <Desktop>
          <EditorApp rect={{ x: 20, y: 240, w: 360, h: 368 }} termLines={ERROR_LINES} termH={92} />
        </Desktop>

        <div
          style={{
            position: "absolute",
            left: CARD_L,
            top: CARD_T,
            width: CARD.w,
            height: CARD.h,
            transform: `scale(${S})`,
            transformOrigin: "0 0",
          }}
        >
          <div
            style={{
              position: "absolute",
              inset: 0,
              perspective: `${Math.max(1200, CAMERA_DEPTH * CARD.w)}px`,
              perspectiveOrigin: "50% 50%",
            }}
          >
            <div
              style={{
                position: "absolute",
                inset: 0,
                transformStyle: "preserve-3d",
                transformOrigin: "center center",
                transform: `translateZ(${z}px) rotateY(${rot}deg)`,
              }}
            >
              <div style={{ ...FACE, transform: "rotateY(0deg) translateZ(1px)" }}>
                <StoryFlipFront cardW={CARD.w} />
              </div>
              <div style={{ ...FACE, transform: "rotateY(180deg) translateZ(1px)" }}>
                <StoryFlipBack ms={ms} tl={TL} />
              </div>
            </div>
          </div>
        </div>

        <div
          style={{
            position: "absolute",
            left: 0,
            width: SCREEN.w,
            top: 476,
            display: "flex",
            justifyContent: "center",
            opacity: chipIn * chipOut,
            transform: `scale(${1 - 0.05 * press}) translateY(${(1 - chipIn) * 6}px)`,
          }}
        >
          <Keys keys={["Ctrl", "Shift", "B"]} size={0.42} />
        </div>

        <Notch
          cx={cx}
          lookX={interpolate(ms, [300, 800], [0, 0.6], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
          lookY={interpolate(ms, [300, 800], [0, 0.5], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
          lid={blinkLid(ms, [2600])}
        />

        <FlipCursor ms={ms} keys={DESK_CURSOR} size={13} fadeOutAt={FLIP_AT - 100} />
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos (ms de la escena) para el montaje: 3.2 s a ≈1.44×.
 * Atajo y volteo / notas «Causa» y «Fix» / lápiz sobre la causa / cajón e inserción del error / tablero completo.
 */
export const FLIPBOARD_SEGMENTS: [number, number][] = [
  [700, 1450],
  [1800, 3050],
  [3150, 4100],
  [4250, 5300],
  [5450, 6050],
];

/** El navegador a medio giro: la mejor imagen de «esto es una ventana que se da vuelta». */
export const FLIPBOARD_HERO_MS = 1000;
