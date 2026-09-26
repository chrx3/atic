/**
 * xterm bajo un `transform: scale()` (la pizarra de agentes con zoom).
 *
 * xterm pasa el mouse a celdas restando `getBoundingClientRect()` —que ya
 * viene escalado— y dividiendo por el ancho de celda SIN escalar. Con zoom
 * distinto de 1 la selección y los clics quedan corridos, y el error crece
 * hacia la derecha y hacia abajo. xterm no tiene opción para esto: se le
 * entrega el mismo evento con el punto llevado al espacio sin escalar.
 */

/** Punto de pantalla → el que vería la pantalla del xterm sin escalar. */
export function unscalePoint(
  point: { x: number; y: number },
  origin: { left: number; top: number },
  scale: number,
): { x: number; y: number } {
  return {
    x: origin.left + (point.x - origin.left) / scale,
    y: origin.top + (point.y - origin.top) / scale,
  };
}

/** Solo lo que xterm convierte a celdas; menús y clics usan coordenadas reales. */
const REMAPPED = ["mousedown", "mousemove", "mouseup"] as const;

/**
 * Corrige el mouse del xterm montado en `host` mientras algún ancestro lo
 * escale. Sin escala no toca nada. Devuelve la función que lo desengancha.
 */
export function attachScaledMouse(host: HTMLElement): () => void {
  const clones = new WeakSet<Event>();
  // La selección sigue al mouse fuera del terminal (xterm escucha en document).
  let dragging = false;

  const onMouse = (ev: MouseEvent) => {
    if (clones.has(ev)) return;
    const screen = host.querySelector<HTMLElement>(".xterm-screen");
    if (!screen) return;
    const inside = ev.target instanceof Node && screen.contains(ev.target);
    if (ev.type === "mousedown") dragging = inside;
    const tracked = inside || dragging;
    if (ev.type === "mouseup") dragging = false;
    if (!tracked || !(ev.target instanceof EventTarget)) return;

    const rect = screen.getBoundingClientRect();
    const scale = screen.offsetWidth > 0 ? rect.width / screen.offsetWidth : 1;
    if (!Number.isFinite(scale) || Math.abs(scale - 1) < 0.01) return;

    const point = unscalePoint({ x: ev.clientX, y: ev.clientY }, rect, scale);
    ev.stopImmediatePropagation();
    const clone = new MouseEvent(ev.type, {
      bubbles: ev.bubbles,
      cancelable: ev.cancelable,
      composed: ev.composed,
      view: ev.view,
      detail: ev.detail,
      screenX: ev.screenX,
      screenY: ev.screenY,
      clientX: point.x,
      clientY: point.y,
      ctrlKey: ev.ctrlKey,
      shiftKey: ev.shiftKey,
      altKey: ev.altKey,
      metaKey: ev.metaKey,
      button: ev.button,
      buttons: ev.buttons,
      relatedTarget: ev.relatedTarget,
    });
    clones.add(clone);
    if (!ev.target.dispatchEvent(clone)) ev.preventDefault();
  };

  for (const type of REMAPPED) window.addEventListener(type, onMouse, true);
  return () => {
    for (const type of REMAPPED) window.removeEventListener(type, onMouse, true);
  };
}
