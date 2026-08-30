/**
 * CLIs vivas en las consolas del overlay, para la pestaña de la pill.
 *
 * ConsolePanel no comparte el árbol con la pill: sin este espejo, un Codex
 * corriendo en PTY se veía como consola genérica.
 *
 * Varias instancias (float + ventana principal) publican por dueño y se
 * unen; al desmontar, esa fuente se borra y no pisa a la otra.
 */

class ConsoleCue {
  #by = new Map<object, string[]>();
  clis = $state<string[]>([]);

  publish(owner: object, clis: string[]) {
    this.#by.set(owner, clis);
    this.#flush();
  }

  clear(owner: object) {
    this.#by.delete(owner);
    this.#flush();
  }

  #flush() {
    const next: string[] = [];
    for (const list of this.#by.values()) {
      for (const id of list) {
        if (!next.includes(id)) next.push(id);
      }
    }
    this.clis = next;
  }
}

export const consoleCue = new ConsoleCue();
