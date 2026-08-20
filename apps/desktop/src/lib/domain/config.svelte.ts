/** Las preferencias de la app, y si el resumen está configurado. */

import {
  getConfig,
  setConfig,
  failedShortcuts,
  onShortcutsFailed,
  secretsStatus,
} from "$ipc/config";
import { on } from "$ipc/events";
import { ollamaAvailable } from "$ipc/summaries";
import type { AppConfig } from "$core/types";
import type { DomainStore } from "./store";
import { applyUiLocale } from "./i18n.svelte";

class ConfigStore implements DomainStore {
  current = $state<AppConfig | null>(null);

  /** Atajos globales que otra app ya tenía tomados. */
  conflicts = $state<string[]>([]);

  /** El proveedor de resumen elegido no tiene clave, o no está corriendo. */
  summarySetupNeeded = $state(false);

  async hydrate(): Promise<void> {
    this.current = await getConfig();
    applyUiLocale(this.current.ui_language);
    // El llavero puede estar bloqueado y el proveedor caído: ninguna de las
    // dos cosas debe impedir que el usuario vea sus grabaciones.
    void this.#refreshSummarySetup().catch(() => {});
    void failedShortcuts()
      .then((names) => (this.conflicts = names))
      .catch(() => {});
  }

  async listen(): Promise<() => void> {
    // Se emite en cada registro, también vacío, para poder limpiar el aviso.
    const unConflicts = await onShortcutsFailed((names) => (this.conflicts = names));
    const unPractice = await on("onboarding-practice", () => {
      void this.hydrate().catch(() => {});
    });
    return () => {
      unConflicts();
      unPractice();
    };
  }

  /**
   * Cambia unas pocas claves.
   *
   * Optimista y con vuelta atrás: la UI refleja el cambio en el acto, y si
   * Rust lo rechaza el estado anterior vuelve. Sin esto, cada interruptor de
   * Ajustes tendría que esperar un viaje de ida y vuelta antes de moverse.
   */
  async patch(changes: Partial<AppConfig>): Promise<void> {
    const before = this.current;
    if (!before) return;
    const next = { ...before, ...changes };
    this.current = next;
    if (changes.ui_language !== undefined) {
      applyUiLocale(changes.ui_language);
    }
    try {
      await setConfig(next);
    } catch (error) {
      this.current = before;
      applyUiLocale(before.ui_language);
      throw error;
    }
    void this.#refreshSummarySetup().catch(() => {});
  }

  async #refreshSummarySetup(): Promise<void> {
    const cfg = this.current;
    if (!cfg) return;
    if (cfg.summary_backend === "ollama") {
      this.summarySetupNeeded = !(await ollamaAvailable());
      return;
    }
    const status = await secretsStatus();
    this.summarySetupNeeded = !status.providers?.[cfg.summary_backend];
  }
}

export const config = new ConfigStore();
