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
 * barras, tonos y una fila que a veces lleva on-demand porque Cursor cobra
 * extra. `tipState` guarda un `string` y lo pintan ~50 anclas; meterle
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
 * # Hit-rect
 *
 * El puntero se queda en el botón, pero el panel publica hit-rect y puede
 * tomar el mouse: quien baja a leerlo lo mantiene abierto. Por eso la gracia
 * corta al salir del botón —cruzar el hueco no puede cerrarlo—.
 */
import { agentQuotas } from "$domain/agentQuotas.svelte";

/** Caja del ancla en px CSS del viewport, copiada (el DOMRect vive poco). */
export type QuotaAnchor = { x: number; y: number; w: number; h: number };

class QuotaHoverState {
  open = $state(false);
  anchor = $state<QuotaAnchor | null>(null);
  /**
   * Gotas de la rueda, en coords del overlay. El hilo cuelga de estas y no
   * de `pill-skin`: con la rueda abierta esa piel sigue siendo el stack
   * arriba-izquierda, y el cuello se pintaba como un palo al lado de la flor.
   */
  parts = $state<QuotaAnchor[] | null>(null);
  /** Lo que dice el panel cuando no hay ningún cupo que mostrar. */
  fallback = $state("");

  show(anchor: QuotaAnchor, fallback: string, parts?: QuotaAnchor[] | null) {
    cancelHide();
    this.anchor = anchor;
    this.parts = parts ?? null;
    this.fallback = fallback;
    this.open = true;
  }

  hide() {
    this.open = false;
    this.anchor = null;
    this.parts = null;
  }
}

export const quotaHoverState = new QuotaHoverState();

/** Espera antes de aparecer, en frío. Gemela de `SHOW_DELAY_MS` en `tip`. */
const SHOW_DELAY_MS = 450;
/** Tiempo para cruzar el hueco isla→panel y llegar al pin. */
const HIDE_GRACE_MS = 400;

let timer = 0;
let hideTimer = 0;
let owner: HTMLElement | null = null;
let globalsInstalled = false;

function cancelHide() {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = 0;
  }
}

function cancelShow() {
  if (timer) {
    clearTimeout(timer);
    timer = 0;
  }
}

function hideNow() {
  cancelHide();
  cancelShow();
  owner = null;
  quotaHoverState.hide();
}

function scheduleHide() {
  cancelHide();
  hideTimer = window.setTimeout(() => {
    hideTimer = 0;
    hideNow();
  }, HIDE_GRACE_MS);
}

export function enterQuotaPanel() {
  cancelHide();
}

export function leaveQuotaPanel() {
  scheduleHide();
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") hideNow();
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
  window.addEventListener("keydown", onKey, true);
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
    cancelHide();
    cancelShow();
    // Se pide apenas entra el puntero, no al abrirse: los 450 ms de espera
    // son justo el tiempo que tarda la consulta, así que el panel abre lleno.
    void agentQuotas.ensure();
    timer = window.setTimeout(() => {
      timer = 0;
      // El overlay es click-through fuera de la pill: a veces el leave no
      // llega y este timer abriría el panel sobre el escritorio.
      if (!node.isConnected || !node.matches(":hover")) return;
      owner = node;
      const box = node.getBoundingClientRect();
      quotaHoverState.show(
        { x: box.left, y: box.top, w: box.width, h: box.height },
        current,
      );
    }, SHOW_DELAY_MS);
  };

  const close = () => {
    // Como `use:tip`: salir cancela la apertura. Si no, el timer dispara
    // después y el panel queda abierto sin mouse encima.
    cancelShow();
    if (owner === node || owner === null) scheduleHide();
  };

  node.addEventListener("pointerenter", open);
  node.addEventListener("pointerleave", close);

  return {
    update(next: string | null) {
      current = (next ?? "").trim();
      if (!current) {
        hideNow();
        return;
      }
      if (owner === node && quotaHoverState.open) {
        quotaHoverState.fallback = current;
      }
    },
    destroy() {
      node.removeEventListener("pointerenter", open);
      node.removeEventListener("pointerleave", close);
      // El botón se va del DOM al cerrarse la isla; el panel no puede quedar
      // flotando sobre su hueco.
      hideNow();
    },
  };
}
