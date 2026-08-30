/**
 * La ventana en la que corre este webview.
 *
 * Está acá y no en un componente porque `@tauri-apps/*` solo puede importarse
 * desde `ipc/`: un patrón como `WindowFrame` recibe estas acciones por props y
 * no sabe que existe Tauri.
 */

import { LogicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";

export type WindowCursor = { x: number; y: number };

export function currentWindowLabel(): string {
  return getCurrentWindow().label;
}

export function minimizeWindow(): Promise<void> {
  return getCurrentWindow().minimize();
}

/** Alterna: si está maximizada la restaura. */
export function toggleMaximizeWindow(): Promise<void> {
  return getCurrentWindow().toggleMaximize();
}

export function startDragging(): Promise<void> {
  return getCurrentWindow().startDragging();
}

export async function setWindowLogicalSize(w: number, h: number): Promise<void> {
  await getCurrentWindow().setSize(new LogicalSize(w, h));
}

export async function setWindowMinLogicalSize(w: number, h: number): Promise<void> {
  await getCurrentWindow().setMinSize(new LogicalSize(w, h));
}

export async function windowLogicalInnerSize(): Promise<{ w: number; h: number }> {
  const win = getCurrentWindow();
  const size = await win.innerSize();
  const scale = await win.scaleFactor();
  return { w: size.width / scale, h: size.height / scale };
}

export async function windowIsMaximized(): Promise<boolean> {
  return getCurrentWindow().isMaximized();
}

export async function windowIsVisible(): Promise<boolean> {
  const win = getCurrentWindow();
  const visible = await win.isVisible();
  const minimized = await win.isMinimized();
  return visible && !minimized;
}

/** Cursor en CSS de esta ventana (no del overlay). */
export const windowCursor = () => invoke<WindowCursor | null>("window_cursor");

export async function onWindowMaximizeChange(
  cb: (maximized: boolean) => void,
): Promise<() => void> {
  const win = getCurrentWindow();
  return win.onResized(async () => {
    cb(await win.isMaximized());
  });
}

export async function onWindowResized(cb: () => void): Promise<() => void> {
  return getCurrentWindow().onResized(() => cb());
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

/**
 * La esconde sin cerrarla.
 *
 * Es lo que usan las ventanas efímeras —el estante de capturas, el lanzador—:
 * cerrarlas de verdad obligaría a recrear el webview en cada aparición, y eso
 * se nota como un parpadeo.
 */
export function hideWindow(): Promise<void> {
  return getCurrentWindow().hide();
}

/**
 * Empieza a redimensionar desde un borde o una esquina.
 *
 * Hace falta cuando la ventana no tiene decoraciones: el borde nativo de
 * Windows queda debajo del webview y es de unos pocos píxeles, así que un tirón
 * desde el contenido no lo agarra. Con esto, un asa dibujada en la esquina
 * redimensiona igual que el marco del sistema.
 */
export async function startResizeDragging(
  direction:
    | "North"
    | "South"
    | "East"
    | "West"
    | "NorthEast"
    | "NorthWest"
    | "SouthEast"
    | "SouthWest",
): Promise<void> {
  await getCurrentWindow().startResizeDragging(direction);
}

/**
 * Avisa cuando la ventana gana o pierde el foco.
 *
 * Las ventanas efímeras se cierran al perderlo: es lo que se espera de algo que
 * apareció encima de lo que estabas haciendo.
 */
export async function onWindowFocus(
  cb: (focused: boolean) => void,
): Promise<() => void> {
  return getCurrentWindow().onFocusChanged(({ payload }) => cb(payload));
}

