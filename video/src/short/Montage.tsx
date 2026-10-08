import { Freeze } from "remotion";
import { Cam3DProvider } from "../lib/cam3d";
import { FormatProvider, useFormat, type Format } from "../lib/format";
import { SCREEN } from "../lib/Screen";
import { lerp } from "../lib/theme";
import { seg } from "../lib/time";
import type { ComponentType } from "react";
import { FPS, MONTAGE_CARDS } from "./shots";

/** Cada plano del montaje pinta el monitor a 2× en su propio lienzo de 1000×1320. */
const FORMAT_CARD: Format = {
  id: "v",
  width: SCREEN.w * 2,
  height: SCREEN.h * 2,
  screenLeft: 0,
  screenTop: 0,
  scale: 2,
  showCaption: false,
};

const easeInOut = (t: number) => (t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2);

const SPACING = 560;

/**
 * Vista explotada: cada herramienta es un plano flotando a distinta
 * profundidad y la cámara los atraviesa en línea recta.
 */
export const Montage = ({
  ms,
  durMs,
  cards = MONTAGE_CARDS,
}: {
  ms: number;
  durMs: number;
  cards?: { scene: ComponentType; atMs: number }[];
}) => {
  const format = useFormat();
  const h = format.id === "h";
  const n = cards.length;
  const cs = h ? 0.56 : 0.66;
  const camZ = lerp(-260, SPACING * (n - 1) + 320, easeInOut(seg(ms, 0, durMs)));

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        perspective: 1250,
        perspectiveOrigin: h ? "70% 48%" : "50% 48%",
        overflow: "hidden",
      }}
    >
      <div
        style={{
          position: "absolute",
          left: h ? "70%" : "50%",
          top: "50%",
          width: 0,
          height: 0,
          transformStyle: "preserve-3d",
          transform: `translate3d(0, 0, ${camZ}px)`,
        }}
      >
        {cards.map(({ scene: Scene, atMs }, i) => {
          const z = -i * SPACING;
          const rel = z + camZ; // > 0: ya pasó la cámara
          const side = i % 2 === 0 ? -1 : 1;
          const opacity = Math.min(
            1,
            Math.max(0, 1 - (rel - 480) / 140),
            Math.max(0, 1 + (rel + 2600) / 700),
          );
          if (opacity <= 0.01) return null;
          return (
            <div
              key={i}
              style={{
                position: "absolute",
                left: -SCREEN.w,
                top: -SCREEN.h,
                width: SCREEN.w * 2,
                height: SCREEN.h * 2,
                borderRadius: 34,
                overflow: "hidden",
                opacity,
                transform: `translate3d(${side * (h ? 120 : 150)}px, ${(i % 3) * 26 - 26}px, ${z}px) rotateY(${-side * 20}deg) rotateX(${(i % 2 ? 1 : -1) * 3}deg) scale(${cs})`,
                boxShadow: "0 40px 120px rgb(0 0 0 / 55%)",
              }}
            >
              <Cam3DProvider cam={null}>
                <FormatProvider format={FORMAT_CARD}>
                  <Freeze frame={Math.round((atMs * FPS) / 1000)}>
                    <Scene />
                  </Freeze>
                </FormatProvider>
              </Cam3DProvider>
            </div>
          );
        })}
      </div>
    </div>
  );
};
