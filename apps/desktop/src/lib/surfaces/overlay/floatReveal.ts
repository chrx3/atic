/**
 * Utilidades compartidas del acto pill-liquid-emerge (open y close reverse).
 */

/** Espera N animation frames (armar CSS transition antes de mover ancla). */
export function waitFrames(n = 2): Promise<void> {
  return new Promise((resolve) => {
    const step = (left: number) => {
      if (left <= 0) resolve();
      else requestAnimationFrame(() => step(left - 1));
    };
    requestAnimationFrame(() => step(n - 1));
  });
}

/**
 * Propiedad CSS que suele moverse al approach/separate según el lado del cuello.
 * top/bottom → `top`; left/right → `left`.
 */
export function separateAxisProp(
  side: string | undefined,
): "left" | "top" {
  return side === "left" || side === "right" ? "left" : "top";
}

/** Fases de cierre reverse (espejo de expand/separate; tuck = peels). */
export type CloseRevealPhase = "tuck" | "approach" | "shrink";

export function isCloseRevealPhase(
  phase: string,
): phase is CloseRevealPhase {
  return phase === "tuck" || phase === "approach" || phase === "shrink";
}
