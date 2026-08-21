/**
 * La cola de pegado: lo que quedó esperando a que haya dónde pegarlo.
 *
 * Existe porque pegar necesita un destino con foco, y muchas veces no lo hay —
 * dictas mirando otra cosa, copias algo desde la consola de agentes—. En vez de
 * perder el texto, se encola y la pill lo ofrece cuando vuelves.
 *
 * Solo se muestra el primero. Es una cola, no una bandeja: si se acumulan tres,
 * lo útil es despachar el de adelante, no elegir entre ellos.
 */

import type { PasteQueueItem } from "$core/types";
import { subscribe } from "$ipc/events";
import { dismissPasteQueueItem, listPasteQueue, pasteQueueItemNow } from "$ipc/paste";
import type { DomainStore } from "./store";

class PasteStore implements DomainStore {
  items = $state<PasteQueueItem[]>([]);

  /** Una acción en vuelo. Evita despachar dos veces el mismo de adelante. */
  busy = $state(false);

  get front(): PasteQueueItem | null {
    return this.items[0] ?? null;
  }

  get count(): number {
    return this.items.length;
  }

  async hydrate(): Promise<void> {
    this.items = await listPasteQueue();
  }

  async listen(): Promise<() => void> {
    // Los dos eventos hacen lo mismo: `paste-queued` trae además la vista
    // previa, que acá no hace falta porque la lista entera se vuelve a leer.
    return subscribe({
      "paste-queue-changed": () => void this.hydrate(),
      "paste-queued": () => void this.hydrate(),
    });
  }

  /** Pega el de adelante ahora. */
  paste(): Promise<void> {
    return this.#run(pasteQueueItemNow);
  }

  /** Descarta el de adelante. */
  dismiss(): Promise<void> {
    return this.#run(dismissPasteQueueItem);
  }

  async #run(action: (id: string) => Promise<void>): Promise<void> {
    const front = this.front;
    if (!front || this.busy) return;
    this.busy = true;
    try {
      await action(front.id);
      // Se relee en vez de sacarlo de la lista a mano: Rust puede haber
      // despachado más de uno si el destino apareció mientras tanto.
      await this.hydrate();
    } finally {
      this.busy = false;
    }
  }
}

export const paste = new PasteStore();
