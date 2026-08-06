/**
 * El grupo líquido del overlay: todas las siluetas que se funden entre sí.
 *
 * Existe porque **un campo de distancia solo funde lo que está en el mismo
 * campo**. Mientras la pill trazaba su contorno por su cuenta y la consola el
 * suyo, no había forma de que se unieran: eran dos dibujos separados, y el
 * cuello entre ambas había que pintarlo a mano.
 *
 * Cada superficie publica sus formas y se olvida. `OverlaySurface` traza el
 * conjunto una sola vez y la unión —el cuello, el corte cuando se alejan— sale
 * de la mezcla, no de un dibujo.
 *
 * Las formas van en **coordenadas del overlay**, que es el viewport de esta
 * ventana. Es lo único que hace comparables dos superficies que no comparten
 * ni padre ni sistema de posicionamiento.
 */

import type { Shape } from "$liquid/sdf";

class LiquidGroup {
  /**
   * El conjunto ya aplanado. Es lo ÚNICO reactivo de acá, y a propósito.
   *
   * El registro por superficie es un objeto plano: si fuera `$state`, publicar
   * lo leería y lo escribiría en el mismo paso, y como publicar ocurre dentro
   * de un `$effect` eso es un efecto que depende de sí mismo. Svelte corta la
   * actualización, las formas no llegan nunca al trazado y la pill —que ya no
   * se pinta sola— desaparece. Pasó exactamente así.
   */
  shapes = $state<Shape[]>([]);

  /** Por superficie, no en una sola lista: cada una reemplaza lo suyo. */
  #parts: Record<string, Shape[]> = {};

  /**
   * Publica las formas de una superficie. Devuelve la baja, con la forma que
   * espera el `return` de un `$effect`.
   */
  publish(id: string, shapes: Shape[]): () => void {
    this.#parts[id] = shapes;
    this.#flush();
    return () => {
      delete this.#parts[id];
      this.#flush();
    };
  }

  #flush(): void {
    this.shapes = Object.values(this.#parts).flat();
  }
}

export const liquid = new LiquidGroup();
