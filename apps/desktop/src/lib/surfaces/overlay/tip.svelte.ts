/**
 * Tooltips propios, dibujados por nosotros.
 *
 * # Por qué no `title`
 *
 * El overlay es UNA ventana que cubre el escritorio virtual entero, y ese
 * escritorio puede tener monitores con escalas distintas. `scale_factor()` de
 * la ventana dice 1.25 mientras el viewport CSS termina 1:1 con los píxeles
 * físicos (visto en el log: `css_w=3840 phys_w=3840`). La app ya compensa ese
 * desfase en todo su código —`physical_client_to_css`, `CSS_VIEW_W_BITS`, la
 * escala inferida de `overlay_covers_topo`—, pero el tooltip nativo lo ubica
 * Chromium POR DENTRO, con su propia idea del origen y la escala del widget.
 * No hay forma de corregirlo desde afuera: sale corrido cientos de píxeles del
 * elemento señalado, incluso en otro monitor.
 *
 * Dibujarlo nosotros lo devuelve al mismo espacio CSS donde la pill, los
 * floats y los hit-rects ya caen bien.
 *
 * # Cómo se usa
 *
 * ```svelte
 * <button use:tip={"Cerrar pestaña"}>…</button>
 * <button use:tip={{ text: label, placement: "top" }}>…</button>
 * ```
 *
 * `null`, `undefined` y `""` no muestran nada, así que un texto condicional
 * puede apagarse solo sin sacar la directiva.
 *
 * El que lo PINTA es `TipHost.svelte` —no `Tip.svelte`: en Windows un nombre
 * que solo difiere en mayúsculas de este módulo hace que TypeScript resuelva
 * el import al componente—, montado una vez por ventana en
 * `routes/+layout.svelte`: varios de estos componentes (la rueda, la consola
 * de agentes) se renderizan tanto en el overlay como en `main`, y sin un
 * anfitrión en cada ventana se quedarían mudos en la otra.
 */

export type TipPlacement = "auto" | "top" | "bottom";

export type TipInput =
  | string
  | null
  | undefined
  | { text?: string | null; placement?: TipPlacement };

/** Caja del ancla en px CSS del viewport, copiada (el DOMRect vive poco). */
export type TipAnchor = { x: number; y: number; w: number; h: number };

class TipState {
  text = $state("");
  placement = $state<TipPlacement>("auto");
  anchor = $state<TipAnchor | null>(null);
  open = $state(false);

  show(text: string, anchor: TipAnchor, placement: TipPlacement) {
    this.text = text;
    this.anchor = anchor;
    this.placement = placement;
    this.open = true;
  }

  hide() {
    this.open = false;
    this.anchor = null;
  }
}

export const tipState = new TipState();

/** Espera antes de aparecer, en frío. */
const SHOW_DELAY_MS = 450;
/**
 * Después de esconderse sigue "tibio" un rato: pasar de un botón al de al
 * lado muestra el siguiente al toque, como un grupo de tooltips de escritorio.
 */
const WARM_MS = 320;

let warmUntil = 0;
let timer = 0;
/** Quién pidió el tooltip visible. Solo ese puede bajarlo. */
let owner: HTMLElement | null = null;
let globalsInstalled = false;

function normalize(input: TipInput): { text: string; placement: TipPlacement } {
  if (typeof input === "string") return { text: input.trim(), placement: "auto" };
  if (input && typeof input === "object") {
    return {
      text: (input.text ?? "").trim(),
      placement: input.placement ?? "auto",
    };
  }
  return { text: "", placement: "auto" };
}

function clearTimer() {
  if (!timer) return;
  clearTimeout(timer);
  timer = 0;
}

function hideNow() {
  clearTimer();
  if (tipState.open) warmUntil = Date.now() + WARM_MS;
  owner = null;
  tipState.hide();
}

/**
 * Un solo juego de oyentes para toda la ventana: son ~50 anclas y ponerle
 * cuatro oyentes de `window` a cada una sería tirar memoria a la basura.
 *
 * En captura porque el tooltip tiene que bajar aunque alguien detenga el
 * evento en el camino —y porque `scroll` no burbujea desde un contenedor
 * anidado (el rail de consolas, la lista de carpetas): sin esto quedaría
 * flotando sobre un elemento que ya se movió.
 */
function installGlobals() {
  if (globalsInstalled || typeof window === "undefined") return;
  globalsInstalled = true;
  window.addEventListener("pointerdown", hideNow, true);
  window.addEventListener("wheel", hideNow, true);
  window.addEventListener("scroll", hideNow, true);
  window.addEventListener("blur", hideNow);
}

/**
 * Repone el nombre accesible si el `title` era el ÚNICO que había.
 *
 * Un botón de puro icono se quedaba anónimo para un lector de pantalla al
 * sacarle el `title`. Cuando ya hay nombre —`aria-label` propio o texto
 * visible— no se toca: pisar el texto visible con el tooltip es peor que no
 * hacer nada (rompe "label in name").
 */
function syncAriaLabel(node: HTMLElement, text: string) {
  if (!text) return;
  if (node.dataset.tipLabel !== "1") {
    if (node.hasAttribute("aria-label") || node.hasAttribute("aria-labelledby")) {
      return;
    }
    if (node.textContent?.trim()) return;
    node.dataset.tipLabel = "1";
  }
  node.setAttribute("aria-label", text);
}

export function tip(node: HTMLElement, input: TipInput) {
  installGlobals();
  let current = normalize(input);
  syncAriaLabel(node, current.text);

  const open = () => {
    if (!current.text) return;
    clearTimer();
    const run = () => {
      timer = 0;
      owner = node;
      const r = node.getBoundingClientRect();
      tipState.show(
        current.text,
        { x: r.left, y: r.top, w: r.width, h: r.height },
        current.placement,
      );
    };
    if (tipState.open || Date.now() < warmUntil) run();
    else timer = window.setTimeout(run, SHOW_DELAY_MS);
  };

  const close = () => {
    clearTimer();
    if (owner === node) hideNow();
  };

  // Solo teclado: con el mouse ya lo cubre `pointerenter`, y mostrarlo en el
  // foco que deja un clic taparía el control recién apretado.
  const onFocus = () => {
    if (node.matches(":focus-visible")) open();
  };

  node.addEventListener("pointerenter", open);
  node.addEventListener("pointerleave", close);
  node.addEventListener("focus", onFocus);
  node.addEventListener("blur", close);

  return {
    update(next: TipInput) {
      current = normalize(next);
      syncAriaLabel(node, current.text);
      if (!current.text) {
        close();
        return;
      }
      if (owner === node && tipState.open) tipState.text = current.text;
    },
    destroy() {
      node.removeEventListener("pointerenter", open);
      node.removeEventListener("pointerleave", close);
      node.removeEventListener("focus", onFocus);
      node.removeEventListener("blur", close);
      // El ancla se va del DOM —una pestaña que se cierra, un menú que se
      // desmonta— y el tooltip no puede quedar colgado sobre su hueco.
      close();
    },
  };
}
