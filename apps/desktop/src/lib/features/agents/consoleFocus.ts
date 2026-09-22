/**
 * Pedir que una consola muestre una pestaña concreta.
 *
 * La pill sabe QUÉ sesión de consola corresponde a un aviso (Rust la resuelve
 * por el árbol de procesos) pero no tiene la consola: esa vive en la isla o en
 * el float. Se avisa por un evento de ventana y la que tenga esa sesión la
 * muestra; las demás lo ignoran.
 */
export const CONSOLE_FOCUS_EVENT = "atic-console-focus";

export type ConsoleFocusDetail = { session: string };

export function requestConsoleFocus(session: string): void {
  window.dispatchEvent(
    new CustomEvent<ConsoleFocusDetail>(CONSOLE_FOCUS_EVENT, { detail: { session } }),
  );
}
