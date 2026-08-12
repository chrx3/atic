/**
 * El grupo líquido del overlay: todas las siluetas que se funden entre sí.
 *
 * Existe porque **un campo de distancia solo funde lo que está en el mismo
 * campo**. Mientras la pill trazaba su contorno por su cuenta y la consola el
 * suyo, no había forma de que se unieran: eran dos dibujos separados, y el
 * cuello entre ambas había que pintarlo a mano.
 *
 * Cada superficie publica sus formas y se olvida. `OverlaySurface` traza
 * cada isla; la unión —el cuello, el corte cuando se alejan— sale de la
 * mezcla, no de un dibujo.
 *
 * Las formas van en **coordenadas del overlay**, que es el viewport de esta
 * ventana. Es lo único que hace comparables dos superficies que no comparten
 * ni padre ni sistema de posicionamiento.
 */

import { REACH } from "$liquid/constants";
import { clusterParts, type Island } from "$liquid/motion";
import type { Shape } from "$liquid/sdf";

function shapeFinger(s: Shape): string {
  if (s.kind === "box") {
    return `b:${s.cx.toFixed(1)},${s.cy.toFixed(1)},${s.hw.toFixed(1)},${s.hh.toFixed(1)},${s.r.toFixed(1)}`;
  }
  return `c:${s.ax.toFixed(1)},${s.ay.toFixed(1)},${s.bx.toFixed(1)},${s.by.toFixed(1)},${s.r.toFixed(1)}`;
}

function partsFinger(parts: Record<string, Shape[]>): string {
  return Object.keys(parts)
    .sort()
    .map((id) => `${id}=${(parts[id] ?? []).map(shapeFinger).join(";")}`)
    .join("|");
}

class LiquidGroup {
  /**
   * Islas que no se funden entre sí (hueco > REACH). Cada una es un Skin:
   * arrastrar la pill no remuestrea el launcher del otro lado de la pantalla.
   *
   * El registro por superficie (`#parts`) NO es `$state`: publicar ocurre
   * dentro de un `$effect`, y si el registro fuera reactivo el efecto se
   * leería y escribiría a sí mismo. Svelte corta esa actualización y la pill
   * desaparece. Pasó exactamente así.
   *
   * `$state.raw` en la salida: se reemplaza el array entero, no se muta por
   * dentro. El proxy profundo de `$state` era costo de más a 60 Hz.
   */
  islands = $state.raw<Island[]>([]);
  /** Plano, para quien no necesita islas. Misma regla: raw. */
  shapes = $state.raw<Shape[]>([]);

  /** Por superficie, no en una sola lista: cada una reemplaza lo suyo. */
  #parts: Record<string, Shape[]> = {};
  /** Evita remesh del Skin cuando el SDF no cambió de verdad. */
  #finger = "";

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
    const finger = partsFinger(this.#parts);
    if (finger === this.#finger) return;
    this.#finger = finger;
    const islands = clusterParts(this.#parts, REACH);
    this.islands = islands;
    this.shapes = islands.flatMap((island) => island.shapes);
  }
}

export const liquid = new LiquidGroup();
