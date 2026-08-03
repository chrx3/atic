/** Historial del portapapeles. */

import { clipboardItemMatches } from "$core/clipboardSearch";
import type { ClipboardItem } from "$core/types";
import { subscribe } from "$ipc/events";
import {
  deleteClipboardItem,
  listClipboardHistory,
  pasteClipboardItem,
  pinClipboardItem,
} from "$ipc/clipboard";
import type { DomainStore } from "./store";

class ClipboardStore implements DomainStore {
  items = $state<ClipboardItem[]>([]);
  /** Filtro de la vista. Vive acá y no en el componente para que la pill y la
   *  ventana principal no se pisen el uno al otro al abrirse a la vez. */
  query = $state("");

  get visible(): ClipboardItem[] {
    const q = this.query;
    // Los fijados primero: el orden por fecha los hundiría justo a los que se
    // fijaron para tenerlos a mano.
    return this.items
      .filter((item) => clipboardItemMatches(item, q))
      .sort((a, b) => Number(b.pinned) - Number(a.pinned));
  }

  async hydrate(): Promise<void> {
    this.items = await listClipboardHistory();
  }

  async listen(): Promise<() => void> {
    return subscribe({ "clipboard-history-changed": () => void this.hydrate() });
  }

  /** Pega en la app que tenga el foco. Rust se encarga del resto. */
  paste(id: string): Promise<void> {
    return pasteClipboardItem(id);
  }

  async pin(id: string, pinned: boolean): Promise<void> {
    await pinClipboardItem(id, pinned);
    await this.hydrate();
  }

  async remove(id: string): Promise<void> {
    await deleteClipboardItem(id);
    await this.hydrate();
  }
}

export const clipboard = new ClipboardStore();
