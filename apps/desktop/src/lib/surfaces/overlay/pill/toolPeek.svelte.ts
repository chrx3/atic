/**
 * El vistazo de una herramienta: lo importante sin un solo clic.
 *
 * Nació para los cupos de «Agentes» y ahora lo usa cualquier herramienta que
 * tenga algo que mirar (`PEEK_TOOLS`). Tres niveles de la misma herramienta:
 * tooltip → vistazo al pasar el mouse → cara completa al hacer clic.
 *
 * # Por qué cuelga de la herramienta y no de la pill
 *
 * Colgaba del disco, y el disco solo existe con la pill suelta: acoplada al
 * borde la silueta es la isla y no había dónde pasar el mouse. Anclarlo a la
 * herramienta lo pone donde el usuario ya va a buscar lo suyo, y de paso lo
 * vuelve alcanzable en la isla —que se abre con el puntero, sin clic.
 *
 * # Por qué no es un `use:tip`
 *
 * Es el mismo gesto y casi el mismo globo, pero el contenido no es texto:
 * barras, filas, miniaturas. `tipState` guarda un `string` y lo pintan ~50
 * anclas; meterle una variante rica obligaría a tocar el camino de todas ellas.
 *
 * Reemplaza al tooltip del botón en vez de sumarse: dos globos sobre el mismo
 * ancla se tapan. Por eso `fallback` — sin nada que mostrar, el vistazo dice
 * lo que habría dicho el tooltip.
 *
 * Lo que sí se copia de `tip.svelte.ts` es la temporización, y no por comodidad:
 * si el vistazo apareciera con otro ritmo que los tooltips de al lado, la pill
 * tendría dos velocidades de hover para el mismo gesto.
 *
 * # Hit-rect
 *
 * El puntero se queda en el botón, pero el vistazo publica hit-rect y puede
 * tomar el mouse: quien baja a leerlo —o a tocar una fila— lo mantiene
 * abierto. Por eso la gracia corta al salir del botón: cruzar el hueco no
 * puede cerrarlo.
 */
import { agentQuotas } from "$domain/agentQuotas.svelte";
import { captures } from "$domain/captures.svelte";
import { clipboard } from "$domain/clipboard.svelte";
import { snippets } from "$domain/snippets.svelte";
import { system } from "$domain/system.svelte";
import { isSyntheticHovered } from "../syntheticHover";
import type { PeekSpec, PeekTool } from "./peekTools";

export { isPeekTool, peekFor, type PeekSpec, type PeekTool } from "./peekTools";

/** Caja del ancla en px CSS del viewport, copiada (el DOMRect vive poco). */
export type PeekAnchor = { x: number; y: number; w: number; h: number };

/** El marcador del panel: un toque adentro no es un clic afuera. */
export const PEEK_PANEL_ATTR = "data-quota-panel";

class ToolPeekState {
  open = $state(false);
  tool = $state<PeekTool>("agents");
  anchor = $state<PeekAnchor | null>(null);
  /**
   * Gotas de la rueda, en coords del overlay. El hilo cuelga de estas y no
   * de `pill-skin`: con la rueda abierta esa piel sigue siendo el stack
   * arriba-izquierda, y el cuello se pintaba como un palo al lado de la flor.
   */
  parts = $state<PeekAnchor[] | null>(null);
  /** Lo que dice el vistazo cuando no hay nada que mostrar. */
  fallback = $state("");

  show(spec: PeekSpec, anchor: PeekAnchor, parts?: PeekAnchor[] | null) {
    cancelHide();
    this.tool = spec.tool;
    this.anchor = anchor;
    this.parts = parts ?? null;
    this.fallback = spec.fallback;
    this.open = true;
  }

  hide() {
    this.open = false;
    this.anchor = null;
    this.parts = null;
  }
}

export const toolPeekState = new ToolPeekState();

/**
 * Se pide apenas entra el puntero, no al abrirse: los 450 ms de espera son
 * justo el tiempo que tarda la consulta, así que el vistazo abre lleno.
 */
export function prefetchPeek(tool: PeekTool): void {
  if (tool === "agents") void agentQuotas.ensure();
  else if (tool === "system") void system.hydrate("resources");
  else if (tool === "clipboard") void clipboard.hydrate();
  else if (tool === "captures") void captures.hydrate().catch(() => {});
  else if (tool === "snippets") void snippets.hydrate().catch(() => {});
  // Color lee sus recientes del almacenamiento al montar: no hay nada que pedir.
}

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

/** Cierra ya, sin gracia. Lo usan también las acciones del vistazo. */
export function hideToolPeek() {
  cancelHide();
  cancelShow();
  owner = null;
  toolPeekState.hide();
}

function scheduleHide() {
  cancelHide();
  hideTimer = window.setTimeout(() => {
    hideTimer = 0;
    hideToolPeek();
  }, HIDE_GRACE_MS);
}

export function enterPeekPanel() {
  cancelHide();
}

export function leavePeekPanel() {
  scheduleHide();
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") hideToolPeek();
}

/** Tocar DENTRO del vistazo es usarlo (pegar, abrir): no lo cierra. */
function onPointerDown(event: PointerEvent) {
  const target = event.target;
  if (target instanceof Element && target.closest(`[${PEEK_PANEL_ATTR}]`)) return;
  hideToolPeek();
}

/**
 * Un solo juego de oyentes, en captura: el vistazo tiene que bajar aunque
 * alguien detenga el evento antes, y sobre todo cuando el disco se convierte
 * en otra cosa —la rueda al hacer clic— sin que llegue un `pointerleave`.
 */
function installGlobals() {
  if (globalsInstalled || typeof window === "undefined") return;
  globalsInstalled = true;
  window.addEventListener("pointerdown", onPointerDown, true);
  window.addEventListener("wheel", hideToolPeek, true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("blur", hideToolPeek);
}

/**
 * `null` apaga la acción, igual que un `use:tip` vacío.
 *
 * La tira de la isla pinta todas sus herramientas con el mismo `<button>`, así
 * que la directiva va en todas y solo las de `PEEK_TOOLS` traen vistazo. Sin
 * esta salida, las demás abrirían un panel vacío al pasarles el mouse.
 */
export function toolPeek(node: HTMLElement, spec: PeekSpec | null) {
  installGlobals();
  let current = spec;

  const open = () => {
    const peek = current;
    if (!peek) return;
    cancelHide();
    cancelShow();
    prefetchPeek(peek.tool);
    timer = window.setTimeout(() => {
      timer = 0;
      // El overlay es click-through fuera de la pill: a veces el leave no
      // llega y este timer abriría el vistazo sobre el escritorio. En Mac el
      // `:hover` real no se actualiza con eventos sintéticos: alcanza con que
      // la cadena sintética cubra el nodo.
      if (!node.isConnected || !(node.matches(":hover") || isSyntheticHovered(node))) {
        return;
      }
      owner = node;
      const box = node.getBoundingClientRect();
      toolPeekState.show(peek, {
        x: box.left,
        y: box.top,
        w: box.width,
        h: box.height,
      });
    }, SHOW_DELAY_MS);
  };

  const close = () => {
    // Como `use:tip`: salir cancela la apertura. Si no, el timer dispara
    // después y el vistazo queda abierto sin mouse encima.
    cancelShow();
    if (owner === node || owner === null) scheduleHide();
  };

  node.addEventListener("pointerenter", open);
  node.addEventListener("pointerleave", close);

  return {
    update(next: PeekSpec | null) {
      current = next;
      if (!next) {
        hideToolPeek();
        return;
      }
      if (owner === node && toolPeekState.open) {
        toolPeekState.fallback = next.fallback;
      }
    },
    destroy() {
      node.removeEventListener("pointerenter", open);
      node.removeEventListener("pointerleave", close);
      // El botón se va del DOM al cerrarse la isla; el vistazo no puede quedar
      // flotando sobre su hueco.
      hideToolPeek();
    },
  };
}
