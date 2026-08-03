/**
 * La ventana en la que corre este webview.
 *
 * Está acá y no en un componente porque `@tauri-apps/*` solo puede importarse
 * desde `ipc/`: un patrón como `WindowFrame` recibe estas acciones por props y
 * no sabe que existe Tauri.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

export function minimizeWindow(): Promise<void> {
  return getCurrentWindow().minimize();
}

/** Alterna: si está maximizada la restaura. */
export function toggleMaximizeWindow(): Promise<void> {
  return getCurrentWindow().toggleMaximize();
}

/**
 * Cierra la ventana.
 *
 * En la principal esto termina la app; en una flotante solo la esconde, según
 * lo que decida Rust en su manejador de cierre.
 */
export function closeWindow(): Promise<void> {
  return getCurrentWindow().close();
}
