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
 * Con la app al frente los eventos reales vuelven a fluir; `document.hasFocus()`
 * apaga el puente para no duplicar `pointerenter`.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { Point } from "$ipc/overlay";

let chain: Element[] = [];

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
}

/** Escucha el cursor y devuelve la baja. */
export function startSyntheticHover(): () => void {
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
