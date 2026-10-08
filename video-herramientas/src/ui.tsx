import React from "react";
import { Easing, interpolate, OffthreadVideo, Sequence, staticFile, useCurrentFrame } from "remotion";
import type { Crop } from "./timeline";

export const FONT = '"Segoe UI Variable Display", "Segoe UI", system-ui, sans-serif';
export const INK = "#0b0b0c";
export const MUTED = "#6e6e73";

const easeOut = Easing.bezier(0.16, 1, 0.3, 1);
export const ease = (f: number, from: number, dur: number) =>
  interpolate(f, [from, from + dur], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: easeOut,
  });

/** Texto que entra palabra por palabra: sube, se enfoca y aparece. */
export const Words: React.FC<{
  text: string;
  at?: number;
  stagger?: number;
  size: number;
  color?: string;
  weight?: number;
  style?: React.CSSProperties;
}> = ({ text, at = 0, stagger = 3, size, color = INK, weight = 700, style }) => {
  const f = useCurrentFrame();
  const lines = text.split("\n");
  let i = 0;
  return (
    <div
      style={{
        fontFamily: FONT,
        fontSize: size,
        fontWeight: weight,
        letterSpacing: "-0.035em",
        lineHeight: 1.04,
        color,
        textAlign: "center",
        ...style,
      }}
    >
      {lines.map((line, li) => (
        <div key={li}>
          {line.split(" ").map((word, wi) => {
            const t = ease(f, at + i++ * stagger, 16);
            return (
              <span
                key={wi}
                style={{
                  display: "inline-block",
                  marginRight: "0.24em",
                  opacity: t,
                  transform: `translateY(${(1 - t) * 0.45}em)`,
                  filter: `blur(${(1 - t) * 14}px)`,
                }}
              >
                {word}
              </span>
            );
          })}
        </div>
      ))}
    </div>
  );
};

/**
 * Un clip grabado de la UI real, encuadrado en una caja.
 *
 * Los clips son la pantalla simulada de 1080×1080 px CSS pintada a 2x; el
 * recorte se da en esos px CSS y se anima de `crop` a `cropEnd`.
 */
export const Clip: React.FC<{
  src: string;
  from: number;
  width: number;
  height: number;
  crop: Crop;
  cropEnd?: Crop;
  duration: number;
  /** Cuadro (relativo a la secuencia) en que el clip empieza a correr. */
  startAt?: number;
}> = ({ src, from, width, height, crop, cropEnd, duration, startAt = 0 }) => {
  const f = useCurrentFrame();
  const t = interpolate(f, [startAt, duration], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: Easing.bezier(0.45, 0, 0.55, 1),
  });
  const end = cropEnd ?? crop;
  const c = {
    x: crop.x + (end.x - crop.x) * t,
    y: crop.y + (end.y - crop.y) * t,
    w: crop.w + (end.w - crop.w) * t,
    h: crop.h + (end.h - crop.h) * t,
  };
  const scale = Math.max(width / c.w, height / c.h);
  const tx = -c.x * scale + (width - c.w * scale) / 2;
  const ty = -c.y * scale + (height - c.h * scale) / 2;
  return (
    <div style={{ position: "absolute", inset: 0, overflow: "hidden", background: "#0c1022" }}>
      <Sequence from={startAt} layout="none">
      <OffthreadVideo
        src={staticFile(`clips/${src}`)}
        startFrom={Math.round(from * 60)}
        muted
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: 1080,
          height: 1080,
          transformOrigin: "0 0",
          transform: `translate(${tx}px, ${ty}px) scale(${scale})`,
        }}
      />
      </Sequence>
    </div>
  );
};
