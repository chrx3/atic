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
 *
 * Afinidad: solo se funden superficies del mismo grupo. La pill, la gota de
 * cupos y la tarjeta de auth viven en `LIQUID_HUB`. Un float entra al hub
 * mientras nace o vuelve a la pill; en reposo es una isla propia, aunque
 * esté pegado a otra ventana flotante.
 */

import { INFLUENCE } from "$liquid/constants";
import { clusterParts, type Island } from "$liquid/motion";
import type { Shape } from "$liquid/sdf";

/** Grupo de la pill: lo que sale de ella se funde acá. */
export const LIQUID_HUB = "hub";

const HUB_IDS = new Set(["pill", "quota", "agent-auth"]);

function defaultGroup(id: string): string {
  return HUB_IDS.has(id) ? LIQUID_HUB : id;
}

function shapeFinger(s: Shape): string {
  if (s.kind === "box") {
    return `b:${s.cx.toFixed(1)},${s.cy.toFixed(1)},${s.hw.toFixed(1)},${s.hh.toFixed(1)},${s.r.toFixed(1)}`;
  }
  return `c:${s.ax.toFixed(1)},${s.ay.toFixed(1)},${s.bx.toFixed(1)},${s.by.toFixed(1)},${s.r.toFixed(1)}`;
}

function partsFinger(
  parts: Record<string, Shape[]>,
  groups: Record<string, string>,
): string {
  return Object.keys(parts)
    .sort()
    .map(
      (id) =>
        `${id}:${groups[id] ?? defaultGroup(id)}=${(parts[id] ?? []).map(shapeFinger).join(";")}`,
    )
    .join("|");
}

class LiquidGroup {
  /**
   * Islas que ya no pueden influirse (hueco > INFLUENCE). Cada una es un Skin:
   * arrastrar la pill no remuestrea el launcher del otro lado de la pantalla.
   *
   * El umbral es `INFLUENCE` (= BLEND) y no `REACH` (= BLEND/2). Con `REACH`
   * las dos formas se ignoraban por completo hasta el hueco exacto en que el
   * cuello ya cerraba, así que la fusión entraba de un frame al otro —incluido
   * el bulto de 1.5 px que el `smin` ya tenía acumulado ahí—. Compartiendo
   * campo desde `INFLUENCE`, las siluetas se estiran una hacia la otra durante
   * todo el acercamiento y el cuello nace desde cero.
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
  /**
   * La gota respira: grabación o dictado. Lo lee el Skin, no cada superficie,
   * para que el pulso sea uno solo y no un LED por chip.
   */
  breathe = $state(false);

  /** Por superficie, no en una sola lista: cada una reemplaza lo suyo. */
  #parts: Record<string, Shape[]> = {};
  /** Afinidad de fusión. Ausente = `defaultGroup(id)`. */
  #groups: Record<string, string> = {};
  /** Evita remesh del Skin cuando el SDF no cambió de verdad. */
  #finger = "";

  /**
   * Publica las formas de una superficie. Devuelve la baja, con la forma que
   * espera el `return` de un `$effect`.
   *
   * `group` opcional: `LIQUID_HUB` para fundirse con la pill. Omitido, un
   * float queda en su propio grupo y no se mezcla con otras ventanas.
   */
  publish(id: string, shapes: Shape[], group?: string): () => void {
    this.#parts[id] = shapes;
    if (shapes.length === 0) {
      delete this.#groups[id];
    } else {
      this.#groups[id] = group ?? defaultGroup(id);
    }
    this.#flush();
    return () => {
      delete this.#parts[id];
      delete this.#groups[id];
      this.#flush();
    };
  }

  #flush(): void {
    const finger = partsFinger(this.#parts, this.#groups);
    if (finger === this.#finger) return;
    this.#finger = finger;
    const islands = clusterParts(this.#parts, INFLUENCE, this.#groups);
    this.islands = islands;
    this.shapes = islands.flatMap((island) => island.shapes);
  }
}

export const liquid = new LiquidGroup();
