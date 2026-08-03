/**
 * Seguir con números una geometría que decide el CSS.
 *
 * El campo de distancia pide medidas; la pill las tiene en su layout —la barra
 * se mide sola con `max-content`, el panel se derrama con una animación de
 * `transform`, la gota viaja con `left`/`right`—. Reescribir todo eso a
 * números sería tirar el conocimiento que hay en esas reglas, así que en vez de
 * eso se lo mide.
 *
 * El truco está en cuándo medir. Un `ResizeObserver` avisa de los cambios de
 * tamaño pero no de los de posición, y ninguna de las dos cosas llega cuadro a
 * cuadro durante una animación. Así que se mide en cada cuadro mientras algo se
 * mueva, y se para cuando la geometría lleva un rato quieta: un `rAF` eterno
 * sobre una pill en reposo es exactamente el gasto que este sistema no puede
 * permitirse.
 */

import type { Rect } from "./geometry";

/** Cuántos cuadros seguidos sin cambios antes de dejar de mirar. */
const IDLE_FRAMES = 3;

/** Bajo esto, dos medidas son la misma: el sub-píxel no mueve la silueta. */
const EPSILON = 0.25;

function same(a: Rect | undefined, b: Rect): boolean {
  return (
    a !== undefined &&
    Math.abs(a.x - b.x) < EPSILON &&
    Math.abs(a.y - b.y) < EPSILON &&
    Math.abs(a.w - b.w) < EPSILON &&
    Math.abs(a.h - b.h) < EPSILON
  );
}

export class RectTracker {
  /** Lo último medido, en coordenadas del elemento de referencia. */
  rects = $state<Record<string, Rect>>({});

  /** Registro plano, igual que `rects`: se recorre, no se observa. */
  #els: Record<string, HTMLElement> = {};
  #origin: HTMLElement | null = null;
  #frame = 0;
  #still = 0;

  /** Contra qué elemento se miden las coordenadas. */
  set origin(el: HTMLElement | null) {
    this.#origin = el;
    this.wake();
  }

  /**
   * Registra un elemento a seguir. Devuelve la baja, con la forma que espera
   * el `return` de un `$effect`.
   */
  track(id: string, el: HTMLElement): () => void {
    this.#els[id] = el;
    this.wake();
    return () => {
      delete this.#els[id];
      // Se saca del resultado además del registro: si no, la silueta seguiría
      // dibujando una forma que ya no existe.
      const { [id]: _gone, ...rest } = this.rects;
      this.rects = rest;
      this.wake();
    };
  }

  /** Algo va a moverse: volver a mirar. Idempotente. */
  wake(): void {
    this.#still = 0;
    if (this.#frame) return;
    this.#frame = requestAnimationFrame(() => this.#tick());
  }

  stop(): void {
    if (this.#frame) cancelAnimationFrame(this.#frame);
    this.#frame = 0;
  }

  #tick(): void {
    this.#frame = 0;
    const origin = this.#origin;
    if (!origin) return;

    const base = origin.getBoundingClientRect();
    const next: Record<string, Rect> = {};
    let changed = false;

    for (const [id, el] of Object.entries(this.#els)) {
      const r = el.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) continue;
      const rect: Rect = {
        x: r.x - base.x,
        y: r.y - base.y,
        w: r.width,
        h: r.height,
      };
      next[id] = rect;
      if (!same(this.rects[id], rect)) changed = true;
    }

    if (changed || Object.keys(next).length !== Object.keys(this.rects).length) {
      this.rects = next;
      this.#still = 0;
    } else {
      this.#still++;
    }

    // Se sigue mirando un poco después de la última diferencia: una transición
    // puede tener un cuadro sin cambio visible en el medio, y cortar ahí
    // dejaría la silueta congelada a mitad del movimiento.
    if (this.#still < IDLE_FRAMES) {
      this.#frame = requestAnimationFrame(() => this.#tick());
    }
  }
}
