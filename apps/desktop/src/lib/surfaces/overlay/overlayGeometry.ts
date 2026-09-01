/**
 * El viewport CSS del overlay, independiente de Tauri.
 *
 * WebView2 a veces agranda el HWND sin disparar `resize`. El overlay tiene
 * que enterarse igual: Rust mapea cursor y áreas contra este tamaño, y la
 * pill reasienta los imanes. El evento es del documento, no IPC, para no
 * emitir `overlay-ready` (eso subía el overlay sobre `main`).
 */

/** `window` avisa que `innerWidth/Height` cambió de verdad. */
export const OVERLAY_GEOMETRY = "atic-overlay-geometry";

/** ¿El recuadro creció o se encogió lo bastante como para reasentar? */
export function viewportShifted(
  prev: { w: number; h: number },
  next: { w: number; h: number },
  minPx = 8,
): boolean {
  return Math.abs(next.w - prev.w) >= minPx || Math.abs(next.h - prev.h) >= minPx;
}
