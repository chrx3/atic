/**
 * Hover y clic sintéticos para macOS.
 *
 * El WKWebView no entrega `mouseMoved`/`pointerenter` mientras la ventana del
 * overlay no es key, así que los nodos de la rueda y los `onpointerenter` de
 * los floats no reaccionan al pasar el mouse: hay que hacer clic primero.
 *
 * El clic tiene el mismo hueco, y en el notch (techo o canto) es peor: la
 * pestaña vive encima del menú / Dock y AppKit a veces se queda el
 * `mouseDown`. El hover sí se ve porque Rust ya muestrea el cursor; el clic
 * nativo a veces llega y a veces no. La rueda, más adentro de la pantalla,
 * suele recibir el evento de verdad.
 *
 * Rust manda `overlay-cursor` (posición) y `overlay-pointer` (down/up del
 * botón principal sobre un hit-rect). Acá se traducen a pointer/mouse sobre el
 * elemento bajo el punto. Si el DOM ya recibió el clic de verdad, no se
 * duplica.
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

import type { OverlayPointer, Point } from "$ipc/overlay";
import { nativePointerAlreadyHandled } from "./syntheticPointer";

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
  press: { button: number; buttons: number; cancelable: boolean } = {
    button: -1,
    buttons: 0,
    cancelable: false,
  },
): void {
  el.dispatchEvent(
    new PointerEvent(type, {
      bubbles,
      composed: true,
      cancelable: press.cancelable,
      view: window,
      clientX: x,
      clientY: y,
      pointerId: 1,
      pointerType: "mouse",
      isPrimary: true,
      button: press.button,
      buttons: press.buttons,
    }),
  );
}

function fireMouse(
  el: Element,
  type: string,
  x: number,
  y: number,
  press: { button: number; buttons: number; detail: number },
): void {
  el.dispatchEvent(
    new MouseEvent(type, {
      bubbles: true,
      composed: true,
      cancelable: true,
      view: window,
      clientX: x,
      clientY: y,
      button: press.button,
      buttons: press.buttons,
      detail: press.detail,
    }),
  );
}

let lastTrustedDown = 0;
let lastTrustedUp = 0;
let synthHeld = false;
let synthDownTarget: Element | null = null;

function applyClick(point: OverlayPointer): void {
  if (document.hasFocus()) {
    synthHeld = false;
    synthDownTarget = null;
    return;
  }
  const now = performance.now();
  if (point.down) {
    if (nativePointerAlreadyHandled(lastTrustedDown, now)) return;
    const target = document.elementFromPoint(point.x, point.y);
    if (!target) return;
    synthHeld = true;
    synthDownTarget = target;
    const press = { button: 0, buttons: 1, cancelable: true };
    fire(target, "pointerdown", point.x, point.y, true, press);
    fireMouse(target, "mousedown", point.x, point.y, {
      button: 0,
      buttons: 1,
      detail: 1,
    });
    return;
  }
  if (!synthHeld) return;
  synthHeld = false;
  const held = synthDownTarget;
  synthDownTarget = null;
  if (nativePointerAlreadyHandled(lastTrustedUp, now)) return;
  const target =
    (point.x >= 0 && point.y >= 0
      ? document.elementFromPoint(point.x, point.y)
      : null) ?? held;
  if (!target) return;
  const x = point.x >= 0 ? point.x : 0;
  const y = point.y >= 0 ? point.y : 0;
  const release = { button: 0, buttons: 0, cancelable: true };
  fire(target, "pointerup", x, y, true, release);
  fireMouse(target, "mouseup", x, y, {
    button: 0,
    buttons: 0,
    detail: 1,
  });
  fireMouse(target, "click", x, y, {
    button: 0,
    buttons: 0,
    detail: 1,
  });
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

function onTrustedPointer(event: PointerEvent): void {
  if (!event.isTrusted || event.button !== 0) return;
  // El IPC a veces gana al mouseDown nativo: si ya sintetizamos, no dejar
  // que el evento real dispare el mismo gesto otra vez.
  if (synthHeld) {
    event.stopImmediatePropagation();
    event.preventDefault();
    return;
  }
  const now = performance.now();
  if (event.type === "pointerdown") lastTrustedDown = now;
  else lastTrustedUp = now;
}

/** Escucha el cursor y los clics, y devuelve la baja. */
export function startSyntheticHover(): () => void {
  installHoverCss();
  lastTrustedDown = 0;
  lastTrustedUp = 0;
  synthHeld = false;
  synthDownTarget = null;
  let disposed = false;
  let unlistenCursor: UnlistenFn | null = null;
  let unlistenPointer: UnlistenFn | null = null;
  window.addEventListener("pointerdown", onTrustedPointer, true);
  window.addEventListener("pointerup", onTrustedPointer, true);
  void listen<Point>("overlay-cursor", (event) => applyPoint(event.payload)).then(
    (off) => {
      if (disposed) {
        off();
      } else {
        unlistenCursor = off;
      }
    },
  );
  void listen<OverlayPointer>("overlay-pointer", (event) =>
    applyClick(event.payload),
  ).then((off) => {
    if (disposed) {
      off();
    } else {
      unlistenPointer = off;
    }
  });
  return () => {
    disposed = true;
    unlistenCursor?.();
    unlistenCursor = null;
    unlistenPointer?.();
    unlistenPointer = null;
    window.removeEventListener("pointerdown", onTrustedPointer, true);
    window.removeEventListener("pointerup", onTrustedPointer, true);
    synthHeld = false;
    synthDownTarget = null;
    clearHover();
  };
}
