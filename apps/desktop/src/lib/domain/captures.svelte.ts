/** Capturas de pantalla recientes. */

import type { CaptureItem } from "$core/types";
import {
  activateCapture,
  cleanupCapturesNow,
  copyCaptureImage,
  deleteCapture,
  listRecentCaptures,
  ocrCaptureAndCopy,
  ocrCaptureText,
  readCaptureOcrCache,
  revealCapture,
} from "$ipc/captures";
import { subscribe } from "$ipc/events";
import type { DomainStore } from "./store";

class CapturesStore implements DomainStore {
  items = $state<CaptureItem[]>([]);

  async hydrate(): Promise<void> {
    this.items = await listRecentCaptures();
  }

  async listen(): Promise<() => void> {
    return subscribe({
      // La nueva se antepone en vez de recargar la lista entera: es el evento
      // más frecuente de la app y llega con el elemento completo.
      "screenshot-created": (item) => (this.items = [item, ...this.items]),
      "screenshot-shelf-updated": () => void this.hydrate(),
    });
  }

  open(path: string): Promise<void> {
    return activateCapture(path);
  }

  reveal(path: string): Promise<void> {
    return revealCapture(path);
  }

  copy(path: string): Promise<void> {
    return copyCaptureImage(path);
  }

  /** Lee el texto de la imagen y lo deja en el portapapeles. Devuelve lo leído. */
  ocr(path: string): Promise<string> {
    return ocrCaptureAndCopy(path);
  }

  /** Lo mismo, sin tocar el portapapeles: para mostrarlo en pantalla. */
  ocrText(path: string): Promise<string> {
    return ocrCaptureText(path);
  }

  /**
   * El texto que Rust ya leyó de esta captura, si lo tiene.
   *
   * `null` es «no está leída», no «no tiene texto»: el OCR tarda segundos y
   * la vista previa se abre en un cuadro, así que primero se pregunta por lo
   * cacheado y recién después se ofrece leerla.
   */
  ocrCached(path: string): Promise<string | null> {
    return readCaptureOcrCache(path);
  }

  async remove(path: string): Promise<void> {
    await deleteCapture(path);
    await this.hydrate();
  }

  /** Borra las vencidas según la retención configurada. Devuelve cuántas. */
  async cleanup(): Promise<number> {
    const removed = await cleanupCapturesNow();
    await this.hydrate();
    return removed;
  }
}

export const captures = new CapturesStore();
