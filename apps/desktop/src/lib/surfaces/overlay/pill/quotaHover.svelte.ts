/**
 * El hover de la herramienta «Agentes»: cupos sin un solo clic.
 *
 * # Por qué cuelga de Agentes y no de la pill
 *
 * Colgaba del disco, y el disco solo existe con la pill suelta: acoplada al
 * borde la silueta es la isla y no había dónde pasar el mouse. Anclarlo a la
 * herramienta lo pone donde el usuario ya va a buscar lo de agentes, y de paso
 * lo vuelve alcanzable en la isla —que se abre con el puntero, sin clic.
 *
 * # Por qué no es un `use:tip`
 *
 * Es el mismo gesto y casi el mismo globo, pero el contenido no es texto: son
 * barras, tonos y una fila que se dibuja distinta porque su proveedor no
 * publica cupo. `tipState` guarda un `string` y lo pintan ~50 anclas; meterle
 * una variante rica obligaría a tocar el camino de todas ellas para servir a
 * una sola.
 *
 * Reemplaza al tooltip del botón en vez de sumarse: dos globos sobre el mismo
 * ancla se tapan. Por eso `fallback` — sin cupos que mostrar, el panel dice lo
 * que habría dicho el tooltip.
 *
 * Lo que sí se copia de `tip.svelte.ts` es la temporización, y no por comodidad:
 * si el panel apareciera con otro ritmo que los tooltips de al lado, la pill
 * tendría dos velocidades de hover para el mismo gesto.
 *
 * # Por qué no hace falta hit-rect
 *
 * El overlay es click-through salvo en los rectángulos que publica. El panel
 * se dibuja con `pointer-events: none` y el puntero nunca lo pisa —se queda en
 * el botón, que sí es zona viva—, así que no hay nada que registrar.
 */
import { agentQuotas } from "$domain/agentQuotas.svelte";

/** Caja del ancla en px CSS del viewport, copiada (el DOMRect vive poco). */
export type QuotaAnchor = { x: number; y: number; w: number; h: number };

class QuotaHoverState {
  open = $state(false);
  anchor = $state<QuotaAnchor | null>(null);
  /** Lo que dice el panel cuando no hay ningún cupo que mostrar. */
  fallback = $state("");

  show(anchor: QuotaAnchor, fallback: string) {
    this.anchor = anchor;
    this.fallback = fallback;
    this.open = true;
  }

  hide() {
    this.open = false;
    this.anchor = null;
  }
}

export const quotaHoverState = new QuotaHoverState();

/** Espera antes de aparecer, en frío. Gemela de `SHOW_DELAY_MS` en `tip`. */
const SHOW_DELAY_MS = 450;

let timer = 0;
let owner: HTMLElement | null = null;
let globalsInstalled = false;

function hideNow() {
  if (timer) {
    clearTimeout(timer);
    timer = 0;
  }
  owner = null;
  quotaHoverState.hide();
}

/**
 * Un solo juego de oyentes, en captura: el panel tiene que bajar aunque
 * alguien detenga el evento antes, y sobre todo cuando el disco se convierte
 * en otra cosa —la rueda al hacer clic— sin que llegue un `pointerleave`.
 */
function installGlobals() {
  if (globalsInstalled || typeof window === "undefined") return;
  globalsInstalled = true;
  window.addEventListener("pointerdown", hideNow, true);
  window.addEventListener("wheel", hideNow, true);
  window.addEventListener("blur", hideNow);
}

/**
 * `fallback` vacío apaga la acción, igual que en `use:tip`.
 *
 * La tira de la isla pinta todas sus herramientas con el mismo `<button>`, así
 * que la directiva va en todas y solo Agentes trae texto. Sin esta salida, las
 * demás abrirían un panel vacío al pasarles el mouse.
 */
export function quotaHover(node: HTMLElement, fallback: string | null) {
  installGlobals();
  let current = (fallback ?? "").trim();

  const open = () => {
    if (!current) return;
    if (timer) clearTimeout(timer);
    // Se pide apenas entra el puntero, no al abrirse: los 450 ms de espera
    // son justo el tiempo que tarda la consulta, así que el panel abre lleno.
    void agentQuotas.ensure();
    timer = window.setTimeout(() => {
      timer = 0;
      owner = node;
      const box = node.getBoundingClientRect();
      quotaHoverState.show(
        { x: box.left, y: box.top, w: box.width, h: box.height },
        current,
      );
    }, SHOW_DELAY_MS);
  };

  const close = () => {
    if (owner === node || owner === null) hideNow();
  };

  node.addEventListener("pointerenter", open);
  node.addEventListener("pointerleave", close);

  return {
    update(next: string | null) {
      current = (next ?? "").trim();
      if (!current) {
        close();
        return;
      }
      if (owner === node && quotaHoverState.open) {
        quotaHoverState.fallback = current;
      }
    },
    destroy() {
      node.removeEventListener("pointerenter", open);
      node.removeEventListener("pointerleave", close);
      // El botón se va del DOM al cerrarse la isla; el panel no puede
      // quedar flotando sobre su hueco.
      close();
    },
  };
}
