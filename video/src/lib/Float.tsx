import type { ReactNode } from "react";
import { C, EASE, SHADOW_GOO, lerp } from "./theme";
import { seg } from "./time";

/** «Acto B» de pill-liquid-emerge: el float nace de la pill, crece y se separa. */
export const FLOAT_T = {
  seedHoldMs: 60,
  growMs: 100,
  transformMs: 150,
  separateMs: 90,
  gap: 16,
  overlap: 20,
  seed: 40,
} as const;

/** Duración total de la apertura (~0.4 s incluyendo el reposo de la semilla). */
export const FLOAT_OPEN_MS =
  FLOAT_T.seedHoldMs + FLOAT_T.growMs + FLOAT_T.separateMs;

type Props = {
  /** Ms actuales de la escena y ms en que se dispara la apertura. */
  ms: number;
  startMs: number;
  /** Centro horizontal del float y borde inferior de la pill a la que se pega. */
  cx: number;
  pillBottom: number;
  w: number;
  h: number;
  /** Radio final (18 en clipboard/textos). */
  radius?: number;
  /** Renderiza el contenido con opacidad 0..1 (aparece al separarse). */
  children?: (reveal: number) => ReactNode;
};

/**
 * Float pegado a la pill por debajo. Devuelve el contenedor ya posicionado
 * en coordenadas de pantalla; el contenido llena `w × h` desde (0,0).
 */
export const FloatFromPill = ({
  ms,
  startMs,
  cx,
  pillBottom,
  w,
  h,
  radius = 18,
  children,
}: Props) => {
  const t = ms - startMs;
  if (t < 0) return null;

  const grow = EASE.smoothOut(seg(t, FLOAT_T.seedHoldMs, FLOAT_T.growMs));
  const sep = EASE.smoothOut(
    seg(t, FLOAT_T.seedHoldMs + FLOAT_T.growMs, FLOAT_T.separateMs),
  );
  const fly = EASE.smoothOut(seg(t, 0, FLOAT_T.transformMs));

  const width = lerp(FLOAT_T.seed, w, grow);
  const height = lerp(FLOAT_T.seed, h, grow);
  const seedTop = pillBottom - FLOAT_T.overlap;
  const top = lerp(seedTop, pillBottom + FLOAT_T.gap, sep);
  const r = lerp(FLOAT_T.seed / 2, radius, grow);
  const reveal = seg(t, FLOAT_T.seedHoldMs + FLOAT_T.growMs, FLOAT_T.separateMs + 60);

  const fade = Math.min(1, seg(t, 0, 100) * 1.4);
  const boxTransform = `translateY(${(1 - fly) * -18}px) scale(${lerp(0.55, 1, fly)})`;

  return (
    <div
      style={{
        position: "absolute",
        left: cx - width / 2,
        top,
        width,
        height,
        opacity: fade,
        transform: boxTransform,
        transformOrigin: "50% 0",
        color: C.text,
      }}
    >
      {/* silueta con la sombra goo: sin hijos para que el filtro no toque el texto */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: r,
          background: C.skin,
          filter: SHADOW_GOO,
        }}
      />
      {/* el contenido se dibuja a tamaño final, recortado por la silueta */}
      <div
        style={{
          position: "absolute",
          inset: 0,
          borderRadius: r,
          overflow: "hidden",
        }}
      >
        <div style={{ position: "absolute", left: (width - w) / 2, top: 0, width: w, height: h, opacity: reveal }}>
          {children?.(reveal)}
        </div>
      </div>
    </div>
  );
};
