/** Modelos de transcripción: cuáles hay, cuáles faltan y su descarga. */

import {
  currentModelReady,
  downloadModel,
  listModels,
  onModelDownloadDone,
  onModelDownloadError,
  onModelDownloadProgress,
} from "$ipc/models";
import { subscribe } from "$ipc/events";
import { groqModelLabel, whisperModelLabel } from "$domain/i18n.svelte";
import type { ModelStatus } from "$core/types";
import { config } from "./config.svelte";
import { toasts } from "./toasts.svelte";
import type { DomainStore } from "./store";

class ModelsStore implements DomainStore {
  items = $state<ModelStatus[]>([]);
  /** El modelo elegido para reuniones ya está en disco. */
  ready = $state(false);
  downloading = $state<{ downloaded: number; total: number } | null>(null);

  get percent(): number {
    const d = this.downloading;
    return d && d.total > 0 ? Math.round((d.downloaded / d.total) * 100) : 0;
  }

  /** El modelo local de reuniones elegido ahora. */
  get meetingModel(): ModelStatus | undefined {
    const id = config.current?.whisper_model ?? "base";
    return this.items.find((m) => m.id === id);
  }

  get meetingUsesGroq(): boolean {
    return config.current?.meeting_backend === "groq";
  }

  /** Hay motor listo: Groq (key se valida al transcribir) o modelo local en disco. */
  get meetingCanTranscribe(): boolean {
    if (this.meetingUsesGroq) return true;
    return this.meetingModel?.downloaded === true;
  }

  get meetingProgressLabel(): string {
    if (this.meetingUsesGroq) {
      return groqModelLabel(config.current?.meeting_groq_model ?? "whisper-large-v3-turbo");
    }
    return this.meetingModel ? whisperModelLabel(this.meetingModel.id) : "Whisper";
  }

  /**
   * Los modelos que la configuración pide y no están descargados.
   *
   * Son hasta tres —dictado, reuniones y, solo si está activa, la vista en
   * vivo— y pueden ser el mismo, así que se deduplica: avisar dos veces de la
   * misma descarga es lo que hacía que el aviso se leyera como un error.
   */
  get missing(): ModelStatus[] {
    const cfg = config.current;
    const find = (id: string) => this.items.find((m) => m.id === id);
    const wanted = [
      ...(cfg?.dictation_backend === "groq"
        ? []
        : [find(cfg?.dictation_whisper_model ?? "base")]),
      ...(cfg?.meeting_backend === "groq" ? [] : [find(cfg?.whisper_model ?? "base")]),
      ...(cfg?.live_transcription ? [find(cfg.live_whisper_model ?? "small")] : []),
    ];

    const missing: ModelStatus[] = [];
    for (const model of wanted) {
      if (model && !model.downloaded && !missing.some((m) => m.id === model.id)) {
        missing.push(model);
      }
    }
    return missing;
  }

  async hydrate(): Promise<void> {
    [this.ready, this.items] = await Promise.all([currentModelReady(), listModels()]);
  }

  async listen(): Promise<() => void> {
    return subscribe({
      "model-download-progress": (p) => {
        this.downloading = { downloaded: p.downloaded, total: p.total };
      },
      "model-download-done": () => {
        this.downloading = null;
        void this.hydrate();
      },
      "model-download-error": (p) => {
        this.downloading = null;
        toasts.push(p.message);
      },
    });
  }

  download(id: string): Promise<void> {
    return downloadModel(id);
  }
}

export const models = new ModelsStore();

// Los envoltorios sueltos siguen existiendo para quien necesite escuchar una
// descarga puntual sin montar el store entero.
export { onModelDownloadDone, onModelDownloadError, onModelDownloadProgress };
