import type { CSSProperties } from "react";
import { AbsoluteFill, Easing } from "remotion";
import { useStageBg } from "../lib/format";
import { Caption, Keys } from "../lib/Caption";
import { CARD, FlipBack, type FlipTL } from "../lib/FlipBack";
import { FlipFront } from "../lib/FlipFront";
import { Screen, SCREEN } from "../lib/Screen";
import { C } from "../lib/theme";
import { seg, useMs } from "../lib/time";

/** Volteo 3D (WindowFlipSurface.svelte): ida 400 ms, curvas por tramo dentro de los keyframes. */
const FLIP_IN_MS = 400;
const FLIP_AT = 1000;
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

/** Guion del reverso (ms de escena). */
const TL: FlipTL = {
  emptyAt: 1350,
  cursorInAt: 1350,
  textToolAt: 1800,
  noteAt: 2250,
  noteTypeAt: 2380,
  noteMsPerChar: 20,
  pencilToolAt: 3000,
  paletteOffAt: 3550,
  drawAt: 3400,
  drawMs: 560,
  drawerAt: 4200,
  drawerTabAt: 4550,
  previewAt: 4900,
  addAt: 5350,
  saved: [
    { from: 3350, to: 4950 },
    { from: 5700, to: 7500 },
  ],
};

/** Real (px de la tarjeta) → lógico (px de la pantalla del video). */
const S = 0.36;
const CARD_L = (SCREEN.w - CARD.w * S) / 2;
const CARD_T = (SCREEN.h - CARD.h * S) / 2;

/** Cámara: `rx` es la x (px reales de la tarjeta) que queda en el centro de la pantalla. */
const CAM: { ms: number; z: number; rx: number }[] = [
  { ms: 0, z: 1.06, rx: 640 },
  { ms: 1300, z: 1.06, rx: 640 },
  { ms: 2100, z: 2.3, rx: 585 },
  { ms: 4000, z: 2.3, rx: 585 },
  { ms: 4550, z: 2.2, rx: 960 },
  { ms: 4850, z: 2.2, rx: 960 },
  { ms: 5250, z: 2, rx: 700 },
  { ms: 5430, z: 2, rx: 700 },
  { ms: 6000, z: 1.06, rx: 640 },
];
const CAM_EASE = Easing.bezier(0.45, 0, 0.2, 1);
const cameraAt = (ms: number) => {
  let i = 0;
  while (i < CAM.length - 1 && ms >= CAM[i + 1].ms) i++;
  const a = CAM[i];
  const b = CAM[Math.min(i + 1, CAM.length - 1)];
  const k = a === b ? 0 : CAM_EASE(seg(ms, a.ms, b.ms - a.ms));
  return { z: a.z + (b.z - a.z) * k, rx: a.rx + (b.rx - a.rx) * k };
};

const Wallpaper = () => (
  <div
    style={{
      position: "absolute",
      left: -500,
      top: -500,
      width: 1500,
      height: 1660,
      background: "radial-gradient(120% 90% at 20% 0%, #e9e3d6 0%, #cfd3d1 45%, #aeb9c2 100%)",
    }}
  />
);

const FACE: CSSProperties = {
  position: "absolute",
  inset: 0,
  overflow: "hidden",
  borderRadius: 8,
  backfaceVisibility: "hidden",
  boxShadow: "0 22px 48px rgb(0 0 0 / 32%), 0 2px 8px rgb(0 0 0 / 18%)",
};

export const FlipboardScene = () => {
  const ms = useMs();
  const t = seg(ms, FLIP_AT, FLIP_IN_MS);
  const { rot, z } = flipPose(t);

  const cam = cameraAt(ms);
  const camera = {
    z: cam.z,
    fx: CARD_L + cam.rx * S,
    fy: CARD_T + (CARD.h / 2) * S,
    ax: SCREEN.w / 2,
    ay: SCREEN.h / 2,
  };

  // Chip del atajo (capa del video): aparece antes del giro y se va al terminar.
  const chipIn = seg(ms, 500, 200);
  const chipOut = 1 - seg(ms, 1350, 200);
  const press = seg(ms, 960, 60) * (1 - seg(ms, 1020, 120));

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Flipboard" sub="Da vuelta la ventana y anota" keys={["Ctrl", "Shift", "B"]} />
      <Screen camera={camera} wallpaper={false}>
        <Wallpaper />

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
                <FlipFront />
              </div>
              <div style={{ ...FACE, transform: "rotateY(180deg) translateZ(1px)" }}>
                <FlipBack ms={ms} tl={TL} />
              </div>
            </div>
          </div>
        </div>

        <div
          style={{
            position: "absolute",
            left: 0,
            width: SCREEN.w,
            top: 545,
            display: "flex",
            justifyContent: "center",
            opacity: chipIn * chipOut,
            transform: `scale(${1 - 0.05 * press}) translateY(${(1 - chipIn) * 6}px)`,
          }}
        >
          <Keys keys={["Ctrl", "Shift", "B"]} size={0.42} />
        </div>
      </Screen>
    </AbsoluteFill>
  );
};
