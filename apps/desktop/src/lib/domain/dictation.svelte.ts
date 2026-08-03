/**
 * Dictado por voz.
 *
 * Estaba duplicado entre la ventana principal y la pill, igual que la
 * grabación: dos copias de la fase, cada una escuchando el mismo evento.
 */

import type { DictationPhase } from "$core/types";
import { dictationPhase, onDictationStatus, toggleDictation } from "$ipc/dictation";
import type { DomainStore } from "./store";

class DictationStore implements DomainStore {
  phase = $state<DictationPhase>("idle");
  message = $state<string | null>(null);

  get active(): boolean {
    return this.phase !== "idle";
  }

  async hydrate(): Promise<void> {
    try {
      this.phase = await dictationPhase();
    } catch {
      // Un dictado en curso es una condición transitoria: si Rust no contesta,
      // asumir «no está dictando» es el único estado seguro.
      this.phase = "idle";
    }
  }

  async listen(): Promise<() => void> {
    return onDictationStatus((status) => {
      this.phase = status.phase;
      this.message = status.message;
    });
  }

  toggle(): Promise<void> {
    return toggleDictation();
  }
}

export const dictation = new DictationStore();
