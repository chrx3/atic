/** Textos guardados a mano, y el bloc de notas. */

import { fuzzyMatch } from "$core/clipboardSearch";
import type { Scratchpad, Snippet } from "$core/types";
import { subscribe } from "$ipc/events";
import {
  deleteSnippet,
  getScratchpad,
  listSnippets,
  pasteSnippet,
  setScratchpad,
  upsertSnippet,
} from "$ipc/snippets";
import type { DomainStore } from "./store";

/** Cuánto se espera antes de guardar el bloc. Escribiendo, un guardado por
 *  tecla serían decenas de escrituras a disco por frase. */
const SCRATCH_DEBOUNCE_MS = 600;

class SnippetsStore implements DomainStore {
  items = $state<Snippet[]>([]);
  scratchpad = $state<Scratchpad | null>(null);
  query = $state("");

  #timer: ReturnType<typeof setTimeout> | null = null;
  #pending: string | null = null;

  get visible(): Snippet[] {
    const q = this.query;
    return this.items.filter((item) =>
      fuzzyMatch([item.name, item.body, ...item.aliases].join("\n"), q),
    );
  }

  async hydrate(): Promise<void> {
    [this.items, this.scratchpad] = await Promise.all([
      listSnippets(),
      getScratchpad(),
    ]);
  }

  async listen(): Promise<() => void> {
    const stop = await subscribe({ "snippets-changed": () => void this.hydrate() });
    return () => {
      stop();
      // Lo que quedó escrito y sin guardar se guarda al desmontar: si no, salir
      // de la vista antes del debounce pierde la última frase.
      this.flushScratchpad();
    };
  }

  paste(id: string): Promise<void> {
    return pasteSnippet(id);
  }

  async save(snippet: Snippet): Promise<void> {
    await upsertSnippet(snippet);
    await this.hydrate();
  }

  async remove(id: string): Promise<void> {
    await deleteSnippet(id);
    await this.hydrate();
  }

  /** Escribe en el bloc. Guarda sola, con retardo. */
  editScratchpad(body: string): void {
    this.scratchpad = { body, updatedAtMs: Date.now() };
    this.#pending = body;
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.flushScratchpad(), SCRATCH_DEBOUNCE_MS);
  }

  flushScratchpad(): void {
    if (this.#timer) {
      clearTimeout(this.#timer);
      this.#timer = null;
    }
    const body = this.#pending;
    this.#pending = null;
    if (body === null) return;
    void setScratchpad(body).catch(() => {
      // Si falla, lo escrito sigue en pantalla: perderlo sería peor que no
      // haberlo guardado.
    });
  }
}

export const snippets = new SnippetsStore();
