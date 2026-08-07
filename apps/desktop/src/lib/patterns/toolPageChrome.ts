/**
 * Cómo se presenta el marco de una herramienta.
 *
 * En la ventana / rutas standalone, `ToolPage` muestra título, blurb e icono.
 * Dentro de `ToolDetailModal` ese chrome ya vive en el modal: el contexto
 * `modal` evita repetirlo sin que cada feature pase props a mano.
 */
import { getContext, setContext } from "svelte";

export type ToolPageChrome = "page" | "modal";

const KEY = Symbol("atic:tool-page-chrome");

export function provideToolPageChrome(chrome: ToolPageChrome): ToolPageChrome {
  return setContext(KEY, chrome);
}

export function useToolPageChrome(): ToolPageChrome {
  return getContext<ToolPageChrome | undefined>(KEY) ?? "page";
}
