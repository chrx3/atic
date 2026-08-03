/** Las grabaciones guardadas y el progreso de su transcripción. */

import {
  deleteRecording,
  importAudio,
  listRecordings,
  renameRecording,
} from "$ipc/recordings";
import { getTranscript, saveTranscript, transcribeRecording } from "$ipc/transcripts";
import { subscribe } from "$ipc/events";
import type { Recording, Transcript } from "$core/types";
import { toasts } from "./toasts.svelte";
import type { DomainStore } from "./store";

class RecordingsStore implements DomainStore {
  items = $state<Recording[]>([]);
  selectedId = $state<string | null>(null);

  /** Progreso 0..1 por grabación, solo mientras se transcribe. */
  progress = $state<Record<string, number>>({});

  /**
   * Transcripciones ya leídas, por id.
   *
   * Se cachean porque son el dato más pesado que devuelve Rust y volver a una
   * grabación es lo más común que se hace. `null` significa «se preguntó y no
   * hay», que no es lo mismo que «todavía no se preguntó» (ausente).
   */
  transcripts = $state<Record<string, Transcript | null>>({});

  get selected(): Recording | null {
    return this.items.find((item) => item.id === this.selectedId) ?? null;
  }

  async hydrate(): Promise<void> {
    const next = await listRecordings();
    this.items = next;
    // La selección tiene que sobrevivir al refresco, pero no puede apuntar a
    // algo que ya no está: si desapareció, cae sobre la más reciente.
    if (!this.selectedId || !next.some((item) => item.id === this.selectedId)) {
      this.selectedId = next[0]?.id ?? null;
    }
  }

  async listen(): Promise<() => void> {
    return subscribe({
      "recordings-changed": () => void this.hydrate(),
      "transcribe-progress": (p) => {
        this.progress = { ...this.progress, [p.id]: p.progress };
      },
      "transcript-ready": (p) => {
        this.#clearProgress(p.id);
        // La cacheada quedó vieja: se tira para que la vista vuelva a pedirla.
        const { [p.id]: _stale, ...rest } = this.transcripts;
        this.transcripts = rest;
        void this.hydrate();
      },
      "transcribe-error": (p) => {
        this.#clearProgress(p.id);
        toasts.push(p.message);
        void this.hydrate();
      },
      "summary-ready": () => void this.hydrate(),
      "summarize-error": (p) => {
        toasts.push(p.message);
        void this.hydrate();
      },
    });
  }

  select(id: string | null): void {
    this.selectedId = id;
  }

  async rename(id: string, title: string): Promise<void> {
    await renameRecording(id, title);
    await this.hydrate();
  }

  async remove(id: string): Promise<void> {
    await deleteRecording(id);
    await this.hydrate();
  }

  async transcribe(id: string): Promise<void> {
    // El progreso se marca acá y no al llegar el primer evento: entre el
    // comando y ese evento hay un hueco en el que la fila no mostraría nada.
    this.progress = { ...this.progress, [id]: 0 };
    try {
      await transcribeRecording(id);
    } catch (error) {
      this.#clearProgress(id);
      throw error;
    }
  }

  /** Lee la transcripción si no está cacheada. Idempotente y sin condición
   *  de carrera: si ya está, no vuelve a preguntar. */
  async loadTranscript(id: string): Promise<void> {
    if (id in this.transcripts) return;
    const loaded = await getTranscript(id);
    this.transcripts = { ...this.transcripts, [id]: loaded };
  }

  /**
   * Guarda una transcripción corregida a mano.
   *
   * Se relee en vez de cachear lo enviado: Rust limpia los fragmentos vacíos y
   * marca el resumen anterior como pendiente, así que lo guardado no es
   * exactamente lo que se mandó.
   */
  async saveEditedTranscript(id: string, transcript: Transcript): Promise<void> {
    await saveTranscript(id, transcript);
    this.transcripts = { ...this.transcripts, [id]: await getTranscript(id) };
    await this.hydrate();
  }

  async importFiles(paths: string[]): Promise<Recording[]> {
    const imported = await importAudio(paths);
    await this.hydrate();
    return imported;
  }

  #clearProgress(id: string): void {
    const { [id]: _done, ...rest } = this.progress;
    this.progress = rest;
  }
}

export const recordings = new RecordingsStore();
