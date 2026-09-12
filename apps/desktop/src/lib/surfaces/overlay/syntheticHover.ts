/**
 * Hover sintético para macOS.
 *
 * El WKWebView no entrega `mouseMoved`/`pointerenter` mientras la ventana del
 * overlay no es key, así que los nodos de la rueda y los `onpointerenter` de
 * los floats no reaccionan al pasar el mouse: hay que hacer clic primero.
 *
 * Rust ya muestrea el cursor para armar el overlay, así que mientras es
 * interactivo manda `overlay-cursor` con la posición CSS. Acá se traduce a
 * eventos pointer/mouse sobre el elemento bajo el punto, respetando entrar y
 * salir de la cadena de ancestros.
 *
 * Los eventos sintéticos no actualizan el estado `:hover` real del motor, así
 * que además se espejan las reglas CSS `:hover` a un atributo que se pone en la
 * misma cadena. Eso es lo que hace andar el notch y el side notch, que se
 * pintan con CSS y no con clases desde JS.
 *
 * Con la app al frente los eventos reales vuelven a fluir; `document.hasFocus()`
 * apaga el puente para no duplicar `pointerenter`.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { Point } from "$ipc/overlay";

const HOVER_ATTR = "data-synth-hover";

let chain: Element[] = [];
let marked: Element[] = [];
let cssInstalled = false;

/** Devuelve las reglas `:hover` de `rules` copiadas a `[data-synth-hover]`. */
function mirrorRules(rules: CSSRuleList): string {
  const out: string[] = [];
  for (const rule of Array.from(rules)) {
    if (rule instanceof CSSStyleRule) {
      if (!rule.selectorText.includes(":hover")) continue;
      const selector = rule.selectorText
        .split(",")
        .map((part) => part.replace(/:hover/g, `[${HOVER_ATTR}]`).trim())
        .join(",");
      out.push(`${selector}{${rule.style.cssText}}`);
    } else if (rule instanceof CSSMediaRule || rule instanceof CSSSupportsRule) {
      const inner = mirrorRules(rule.cssRules);
      if (inner) {
        const at = rule instanceof CSSMediaRule ? "media" : "supports";
        out.push(`@${at} ${rule.conditionText}{${inner}}`);
      }
    }
  }
  return out.join("\n");
}

/** Espeja una sola vez todas las reglas `:hover` de la página. */
function installHoverCss(): void {
  if (cssInstalled) return;
  cssInstalled = true;
  const chunks: string[] = [];
  for (const sheet of Array.from(document.styleSheets)) {
    try {
      if (sheet.cssRules) chunks.push(mirrorRules(sheet.cssRules));
    } catch {
      // Hoja de otro origen: no se puede leer.
    }
  }
  const css = chunks.filter(Boolean).join("\n");
  if (!css) return;
  const style = document.createElement("style");
  style.textContent = css;
  document.head.appendChild(style);
}

function markChain(next: Element[]): void {
  const chainSet = new Set(next);
  for (const el of marked) {
    if (!chainSet.has(el)) el.removeAttribute(HOVER_ATTR);
  }
  for (const el of next) el.setAttribute(HOVER_ATTR, "");
  marked = next.slice();
}

function unmarkAll(): void {
  for (const el of marked) el.removeAttribute(HOVER_ATTR);
  marked = [];
}

function ancestors(el: Element | null): Element[] {
  const out: Element[] = [];
  while (el && el !== document.documentElement) {
    out.push(el);
    el = el.parentElement;
  }
  return out;
}

function fire(
  el: Element,
  type: string,
  x: number,
  y: number,
  bubbles: boolean,
): void {
  el.dispatchEvent(
    new PointerEvent(type, {
      bubbles,
      composed: true,
      cancelable: false,
      clientX: x,
      clientY: y,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      button: -1,
      buttons: 0,
    }),
  );
}

function clearHover(): void {
  const gone = chain;
  chain = [];
  unmarkAll();
  for (const el of gone) {
    fire(el, "pointerout", 0, 0, true);
    fire(el, "pointerleave", 0, 0, false);
    fire(el, "mouseout", 0, 0, true);
    fire(el, "mouseleave", 0, 0, false);
  }
}

function applyPoint(point: Point): void {
  if (document.hasFocus() || point.x < 0 || point.y < 0) {
    clearHover();
    return;
  }
  const target = document.elementFromPoint(point.x, point.y);
  if (!target) {
    clearHover();
    return;
  }
  const next = ancestors(target);
  // Prefijo común desde la raíz: lo compartido no entra ni sale.
  let common = 0;
  while (
    common < Math.min(chain.length, next.length) &&
    chain[chain.length - 1 - common] === next[next.length - 1 - common]
  ) {
    common++;
  }
  for (let i = 0; i < chain.length - common; i++) {
    const el = chain[i];
    fire(el, "pointerout", point.x, point.y, true);
    fire(el, "pointerleave", point.x, point.y, false);
    fire(el, "mouseout", point.x, point.y, true);
    fire(el, "mouseleave", point.x, point.y, false);
  }
  for (let i = next.length - common - 1; i >= 0; i--) {
    const el = next[i];
    fire(el, "pointerover", point.x, point.y, true);
    fire(el, "pointerenter", point.x, point.y, false);
    fire(el, "mouseover", point.x, point.y, true);
    fire(el, "mouseenter", point.x, point.y, false);
  }
  fire(target, "pointermove", point.x, point.y, true);
  fire(target, "mousemove", point.x, point.y, true);
  chain = next;
  markChain(next);
}

/** Escucha el cursor y devuelve la baja. */
export function startSyntheticHover(): () => void {
  installHoverCss();
  let disposed = false;
  let unlisten: UnlistenFn | null = null;
  void listen<Point>("overlay-cursor", (event) => applyPoint(event.payload)).then(
    (off) => {
      if (disposed) {
        off();
      } else {
        unlisten = off;
      }
    },
  );
  return () => {
    disposed = true;
    unlisten?.();
    unlisten = null;
    clearHover();
  };
}
