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

/**
 * Saca un archivo de la app hacia otra aplicación.
 *
 * Es un arrastre nativo del sistema, no HTML5: soltar en el Explorador o en un
 * chat solo funciona si lo inicia el SO. El plugin se carga en el momento
 * porque la mayoría de las sesiones nunca arrastra nada.
 */
export async function dragOut(path: string): Promise<void> {
  const { startDrag } = await import("@crabnebula/tauri-plugin-drag");
  await startDrag({ item: [path], icon: path });
}
