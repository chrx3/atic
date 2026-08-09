/**
 * Qué se está mirando en la ventana principal.
 *
 * Es estado de UI, no de dominio, y por eso NO es un singleton de módulo: se
 * instancia y se baja por contexto. Un singleton acá sobreviviría al reemplazo
 * en caliente —dejando la vista anterior pegada tras cada guardado— y haría
 * imposible montar dos veces la superficie en un test.
 *
 * El picker (rueda + cards) es la vista permanente. `openTool` centra la
 * herramienta; el detalle/config vive en un modal.
 */

import { getContext, setContext } from "svelte";
import type { ToolId } from "$core/tools";
import type { SettingsSectionId } from "$features/settings/settingsSections";

export type DetailTab = "detail" | "settings";

export class MainUi {
  activeTool = $state<ToolId>("meetings");
  /** Con qué pestaña abre la herramienta de textos cuando se entra desde fuera. */
  snippetsTab = $state<"snippets" | "scratchpad">("snippets");
  detailTool = $state<ToolId | null>(null);
  detailTab = $state<DetailTab>("detail");

  /** Modal de búsqueda global (SearchModal / Ctrl+K). */
  searchOpen = $state(false);

  /** Modal de Ajustes generales (SettingsPanel). */
  settingsOpen = $state(false);
  settingsSection = $state<SettingsSectionId>("general");

  openTool(tool: ToolId): void {
    this.activeTool = tool;
  }

  openDetail(tool: ToolId, tab: DetailTab = "detail"): void {
    this.activeTool = tool;
    this.detailTool = tool;
    this.detailTab = tab;
  }

  closeDetail(): void {
    this.detailTool = null;
    this.detailTab = "detail";
  }

  /** Buscador in-app (Ctrl+K). No es el launcher de apps del sistema. */
  openSearch(): void {
    this.detailTool = null;
    this.searchOpen = true;
  }

  closeSearch(): void {
    this.searchOpen = false;
  }

  openSettings(section: SettingsSectionId = "general"): void {
    this.settingsSection = section;
    this.settingsOpen = true;
  }

  closeSettings(): void {
    this.settingsOpen = false;
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

/** Como `useMainUi`, pero `null` fuera de la ventana principal (p. ej. float). */
export function tryMainUi(): MainUi | null {
  return getContext<MainUi | undefined>(KEY) ?? null;
}
