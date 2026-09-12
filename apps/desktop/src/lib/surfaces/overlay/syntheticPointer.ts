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
