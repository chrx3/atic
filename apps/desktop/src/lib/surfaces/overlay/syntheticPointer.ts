/** Si el DOM ya vio el clic, el IPC de Rust llega un poco después: no repetir. */
export const SYNTH_TRUST_MS = 80;

/**
 * ¿El DOM ya recibió este flanco? El IPC de Rust llega después del NSEvent:
 * si el WKWebView sí lo entregó, no hay que sintetizar otro.
 */
export function nativePointerAlreadyHandled(
  lastTrusted: number,
  now: number,
  windowMs = SYNTH_TRUST_MS,
): boolean {
  return lastTrusted > 0 && now - lastTrusted < windowMs;
}

/** Tolerancia en píxeles para reconocer el eco del mismo clic físico. */
export const SYNTH_ECHO_PX = 10;

export type SynthEcho = { x: number; y: number } | null;

/**
 * El eco nativo del clic que acabamos de sintetizar.
 *
 * Cuando el IPC de Rust gana la carrera al NSEvent, el clic nativo del **mismo
 * gesto físico** llega después de que terminamos de sintetizarlo: en macOS
 * medimos el `click` ~190 ms más tarde. Ese eco vuelve a disparar el botón —un
 * menú que abre y cierra, un agente que arranca dos veces—, así que hay que
 * reconocerlo y tragárselo.
 *
 * Reglas (el gesto se identifica por punto, no por reloj):
 * - `pointerup`/`mouseup`/`click` en el mismo punto: es eco, se consume.
 *   El `click` es el último del gesto, así que además cierra la espera; el
 *   `up` la deja abierta porque el `click` todavía tiene que llegar.
 * - Un `pointerdown`/`mousedown` en el mismo punto es un gesto nuevo:
 *   cierra la espera y pasa.
 * - Cualquier evento en otro punto es del usuario: cierra la espera y pasa.
 */
export function resolveSynthEcho(
  echo: SynthEcho,
  event: { type: string; clientX: number; clientY: number },
  tolerancePx = SYNTH_ECHO_PX,
): { echo: SynthEcho; consume: boolean } {
  if (!echo) return { echo: null, consume: false };
  const sameSpot =
    Math.abs(event.clientX - echo.x) <= tolerancePx &&
    Math.abs(event.clientY - echo.y) <= tolerancePx;
  if (!sameSpot) return { echo: null, consume: false };
  if (event.type === "pointerdown" || event.type === "mousedown") {
    return { echo: null, consume: false };
  }
  if (event.type === "click") return { echo: null, consume: true };
  if (event.type === "pointerup" || event.type === "mouseup") {
    return { echo, consume: true };
  }
  return { echo: null, consume: false };
}
