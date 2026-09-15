/**
 * Dictado por voz.
 *
 * Estaba duplicado entre la ventana principal y la pill, igual que la
 * grabación: dos copias de la fase, cada una escuchando el mismo evento.
 */

import type { DictationPhase } from "$core/types";
import { dictationLastText, dictationPhase, toggleDictation } from "$ipc/dictation";
import { subscribe } from "$ipc/events";
import type { DomainStore } from "./store";

class DictationStore implements DomainStore {
  phase = $state<DictationPhase>("idle");
  message = $state<string | null>(null);
  /** Último texto dictado, para mostrarlo en la ventana principal. */
  lastText = $state<string | null>(null);
  /** Nivel del micrófono mientras escucha: el medidor de la ventana principal. */
  mic = $state(0);

  get active(): boolean {
    return this.phase !== "idle";
  }

  async hydrate(): Promise<void> {
    // El texto es opcional: si el binario todavía no conoce el comando, la
    // fase igual se hidrata y el bloque del texto simplemente no aparece.
    const phase = dictationPhase().catch((): DictationPhase => "idle");
    const text = dictationLastText().catch(() => null);
    [this.phase, this.lastText] = await Promise.all([phase, text]);
  }

  async listen(): Promise<() => void> {
    return subscribe({
      "dictation-status": (status) => {
        this.phase = status.phase;
        this.message = status.message;
        if (status.text) this.lastText = status.text;
        if (status.phase !== "listening") this.mic = 0;
      },
      // Rust publica el nivel con la misma captura que usa el dictado: la
      // reunión y el dictado escuchan el mismo evento y cada superficie lo usa
      // para lo suyo.
      "audio-levels": (next) => (this.mic = next.mic),
    });
  }

  toggle(): Promise<void> {
    return toggleDictation();
  }
}

export const dictation = new DictationStore();
