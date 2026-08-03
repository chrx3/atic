/**
 * Avisos efímeros.
 *
 * Estaba implementado tres veces —en la ventana principal, en la pill y en el
 * shelf— y cada copia guardaba un solo mensaje en una variable, así que un
 * aviso pisaba al anterior aunque dijeran cosas distintas. Acá son una lista:
 * dos avisos seguidos se leen los dos.
 */

export interface Toast {
  id: number;
  message: string;
}

/** Más que esto en pantalla es ruido, no información. */
const MAX = 3;

class Toasts {
  items = $state<Toast[]>([]);

  #next = 1;
  #timers = new Map<number, ReturnType<typeof setTimeout>>();

  push(message: string, durationMs = 5000): number {
    const id = this.#next++;
    this.items = [...this.items, { id, message }].slice(-MAX);
    this.#timers.set(
      id,
      setTimeout(() => this.dismiss(id), durationMs),
    );
    return id;
  }

  dismiss(id: number): void {
    const timer = this.#timers.get(id);
    if (timer) {
      clearTimeout(timer);
      this.#timers.delete(id);
    }
    this.items = this.items.filter((t) => t.id !== id);
  }

  clear(): void {
    for (const timer of this.#timers.values()) clearTimeout(timer);
    this.#timers.clear();
    this.items = [];
  }
}

export const toasts = new Toasts();

/** Atajo para el caso más común: reportar un error que ya viene de Rust. */
export function toastError(error: unknown, durationMs?: number): void {
  toasts.push(error instanceof Error ? error.message : String(error), durationMs);
}
