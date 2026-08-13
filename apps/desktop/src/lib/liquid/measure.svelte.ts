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

  /**
   * Dónde está la referencia dentro del viewport.
   *
   * Lo necesita quien publica sus siluetas a un grupo compartido: ahí las
   * formas tienen que estar en coordenadas comunes, y estas lo están en las de
   * la referencia. Se mide en el mismo cuadro que el resto, no aparte: pedirlo
   * después daría la posición de otro instante durante una animación.
   */
  originAt = $state<{ x: number; y: number }>({ x: 0, y: 0 });

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

  /**
   * Algo va a moverse: volver a mirar. Idempotente.
   *
   * Por defecto espera al próximo cuadro: medir y remeshear el SDF en el
   * mismo flush que un `pointerdown` congela el picker (el gesto se corta y
   * la rueda “parpadea”). `sync` es para el overlay, donde el path se
   * traslada con el drag y un cuadro de retraso se nota.
   */
  wake(sync = false): void {
    this.#still = 0;
    if (this.#frame) return;
    if (sync) {
      this.#tick();
      return;
    }
    this.#frame = requestAnimationFrame(() => this.#tick());
  }

  stop(): void {
    if (this.#frame) cancelAnimationFrame(this.#frame);
    this.#frame = 0;
  }

  #tick(): void {
    this.#frame = 0;
    const origin = this.#origin;
    // Todavía sin referencia: se vuelve a mirar en el próximo cuadro en vez de
    // abandonar. Cortar acá dejaba el seguimiento muerto hasta el siguiente
    // `wake()`, que en el peor caso no llega nunca.
    if (!origin) {
      if (this.#still++ < IDLE_FRAMES) {
        this.#frame = requestAnimationFrame(() => this.#tick());
      }
      return;
    }

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

    // La referencia también se mueve: durante un vuelo cambia de sitio sin
    // cambiar de tamaño, y quien publica al grupo compartido necesita las dos
    // cosas del MISMO cuadro.
    const moved =
      Math.abs(this.originAt.x - base.x) >= EPSILON ||
      Math.abs(this.originAt.y - base.y) >= EPSILON;
    if (moved) this.originAt = { x: base.x, y: base.y };

    if (
      changed ||
      moved ||
      Object.keys(next).length !== Object.keys(this.rects).length
    ) {
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
