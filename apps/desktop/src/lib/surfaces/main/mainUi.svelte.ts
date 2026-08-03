/**
 * Qué se está mirando en la ventana principal.
 *
 * Es estado de UI, no de dominio, y por eso NO es un singleton de módulo: se
 * instancia y se baja por contexto. Un singleton acá sobreviviría al reemplazo
 * en caliente —dejando la vista anterior pegada tras cada guardado— y haría
 * imposible montar dos veces la superficie en un test.
 */

import { getContext, setContext } from "svelte";
import type { ToolId } from "$core/tools";

export type View = "hub" | "tool";

export class MainUi {
  view = $state<View>("hub");
  activeTool = $state<ToolId>("meetings");
  /** Con qué pestaña abre la herramienta de textos cuando se entra desde fuera. */
  snippetsTab = $state<"snippets" | "scratchpad">("snippets");

  openTool(tool: ToolId): void {
    this.activeTool = tool;
    this.view = "tool";
  }

  backToHub(): void {
    this.view = "hub";
  }
}

const KEY = Symbol("atic:main-ui");

export function provideMainUi(): MainUi {
  return setContext(KEY, new MainUi());
}

export function useMainUi(): MainUi {
  const ui = getContext<MainUi | undefined>(KEY);
  if (!ui) throw new Error("useMainUi() fuera de MainSurface");
  return ui;
}
