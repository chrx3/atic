import { createContext, createElement, useContext, type ReactNode } from "react";
import { useCurrentFrame, useVideoConfig } from "remotion";

/** Traduce el tiempo local del plano al tiempo de la escena original (cortes y velocidad). */
const TimeMapContext = createContext<((localMs: number) => number) | null>(null);

/** Milisegundos transcurridos en la escena actual. */
export const useMs = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const map = useContext(TimeMapContext);
  const local = (frame / fps) * 1000;
  return map ? map(local) : local;
};

/**
 * Reproduce tramos `[desde, hasta]` (ms de la escena original) uno tras otro a
 * velocidad constante dentro de `durationMs`, con cortes secos entre ellos.
 */
export const TimeRemap = ({
  segments,
  durationMs,
  children,
}: {
  segments: [number, number][];
  durationMs: number;
  children: ReactNode;
}) => {
  const total = segments.reduce((n, [a, b]) => n + (b - a), 0);
  const map = (localMs: number) => {
    let u = Math.min(1, Math.max(0, localMs / durationMs)) * total;
    for (const [a, b] of segments) {
      if (u <= b - a) return a + u;
      u -= b - a;
    }
    return segments[segments.length - 1][1];
  };
  return createElement(TimeMapContext.Provider, { value: map }, children);
};

/** Progreso lineal 0..1 de un tramo que empieza en `startMs` y dura `durMs`. */
export const seg = (ms: number, startMs: number, durMs: number) =>
  Math.min(1, Math.max(0, (ms - startMs) / durMs));

/**
 * Valor animado que salta entre 0 y 1 cada vez que pasa un instante de `toggles`
 * (el primero abre, el segundo cierra, ...). Parte desde el valor alcanzado.
 */
export const toggled = (
  ms: number,
  toggles: number[],
  durMs: number,
  ease: (t: number) => number,
  delayMs = 0,
  /** Retardo para los tramos de cierre; por defecto el mismo que al abrir. */
  closeDelayMs = delayMs,
) => {
  let value = 0;
  for (let i = 0; i < toggles.length; i++) {
    const start = toggles[i] + (i % 2 === 0 ? delayMs : closeDelayMs);
    if (ms < start) break;
    const target = i % 2 === 0 ? 1 : 0;
    const raw = seg(ms, start, durMs);
    value = value + (target - value) * ease(raw);
    if (raw < 1) break;
    value = target;
  }
  return value;
};
