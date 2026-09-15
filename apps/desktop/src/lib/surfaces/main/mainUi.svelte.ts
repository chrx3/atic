/**
 * Qué se está mirando en la ventana principal.
 *
 * Es estado de UI, no de dominio, y por eso NO es un singleton de módulo: se
 * instancia y se baja por contexto. Un singleton acá sobreviviría al reemplazo
 * en caliente —dejando la vista anterior pegada tras cada guardado— y haría
 * imposible montar dos veces la superficie en un test.
 *
 * La ventana ya no tiene picker: siempre muestra el cuerpo de una herramienta
 * (Reuniones por defecto) y las pestañas del workspace cambian entre las que
 * tienen vista. Las que son acción (Pizarra, Color, Apps) viven en la pill.
 */

import { getContext, setContext } from "svelte";
import { config } from "$domain/config.svelte";
import { BODIED_TOOLS, type ToolId } from "$core/tools";
import type { SettingsSectionId } from "$features/settings/settingsSections";

export type DetailTab = "detail" | "settings";

/** ¿Esta tool tiene cuerpo que mostrar en la ventana? */
function hasBody(id: ToolId): boolean {
  return BODIED_TOOLS.some((tool) => tool.id === id);
}

export class MainUi {
  activeTool = $state<ToolId>("meetings");
  /** Con qué pestaña abre la herramienta de textos cuando se entra desde fuera. */
  snippetsTab = $state<"snippets" | "scratchpad">("snippets");
  detailTab = $state<DetailTab>("detail");

  /** Modal de búsqueda global (SearchModal / Ctrl+K). */
  searchOpen = $state(false);

  /** Modal de Ajustes generales (SettingsPanel). */
  settingsOpen = $state(false);
  settingsSection = $state<SettingsSectionId>("general");
  /** Pantalla de permisos de macOS: arranque y acceso desde Ajustes. */
  permissionsOpen = $state(false);
  /** Remonta el tutorial si ya estaba a mitad. */
  onboardingReplay = $state(0);
  /**
   * El birrete o Ajustes pidieron repetirlo: se puede cerrar. El primer uso
   * no: el consentimiento no es opcional.
   */
  replayingOnboarding = $state(false);

  openTool(tool: ToolId): void {
    if (hasBody(tool)) this.activeTool = tool;
  }

  openDetail(tool: ToolId, tab: DetailTab = "detail"): void {
    if (!hasBody(tool)) return;
    this.activeTool = tool;
    this.detailTab = tab;
  }

  /**
   * Buscador in-app (Ctrl+K). No es el launcher de apps del sistema.
   *
   * Ya no cierra la herramienta abierta: eso hacía falta cuando el detalle era
   * otro modal y se tapaban entre sí. Con el workspace debajo, un resultado
   * puede navegar sin haber perdido antes el sitio donde se estaba.
   */
  openSearch(): void {
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

  openPermissions(): void {
    this.permissionsOpen = true;
  }

  closePermissions(): void {
    this.permissionsOpen = false;
  }

  /** Vuelve a mostrar el tutorial de primer uso, desde el principio. */
  async replayOnboarding(): Promise<void> {
    this.replayingOnboarding = true;
    await config.patch({ onboarding_done: false, onboarding_practice_done: false });
    this.closeSearch();
    this.closeSettings();
    this.onboardingReplay += 1;
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
