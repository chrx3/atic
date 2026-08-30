/**
 * Silueta líquida acoplada al morph visual de `.float-emerge`.
 *
 * El ancla lógica del bubble no se mueve al cerrar; el contenido sí
 * (scale + travel + opacity). Publicar solo el ancla deja el blob a tamaño
 * lleno mientras el texto se repliega. Hay que muestrear el rectángulo visual
 * (`getBoundingClientRect`) cuadro a cuadro mientras el morph corre, y parar
 * cuando la geometría se quieta — mismo criterio que `RectTracker`.
 *
 * Uso dentro de un `$effect`: devolver el cleanup. El caller debe depender de
 * `shown` / ancla para despertar el seguimiento al abrir, cerrar o mover.
 *
 * Durante drag el goo sigue publicado: el Skin traslada el path si el
 * grupo se mueve rígido, y remeshea más grueso solo si el cuello se estira.
 *
 * Idle abierto: **no** debe quedar un rAF eterno. Tope duro por si el
 * subpíxel (WebView2) nunca se quieta y satura el remesh del Skin — salvo
 * a mitad de un drag, que puede durar más de dos segundos.
 */
import { boxShape } from "$lib/liquid/geometry";
import type { Shape } from "$lib/liquid/sdf";
import { liquid } from "$surfaces/overlay/group.svelte";
import { surfaces } from "$surfaces/overlay/surfaces.svelte";
import {
  IDLE_FRAMES,
  MAX_TRACK_FRAMES,
  rectKey,
  sameRect,
  type SkinRect,
} from "./floatEmergeSkinMath";

export {
  IDLE_FRAMES,
  MAX_TRACK_FRAMES,
  rectKey,
  sameRect,
} from "./floatEmergeSkinMath";

function publishFromEl(
  id: string,
  el: HTMLElement,
  corner: number,
  r: DOMRect,
  group?: string,
): SkinRect {
  const layoutW = el.offsetWidth || r.width;
  const layoutH = el.offsetHeight || r.height;
  const k = Math.min(r.width / layoutW, r.height / layoutH, 1);
  const rect = { x: r.x, y: r.y, w: r.width, h: r.height };
  liquid.publish(id, [boxShape(rect, corner * k)], group);
  return rect;
}

export function publishEmergeSkin(
  id: string,
  el: HTMLElement,
  corner: number,
  group?: string,
): () => void {
  let raf = 0;
  let still = 0;
  let frames = 0;
  let last: SkinRect | null = null;

  const tick = () => {
    frames += 1;
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) {
      liquid.publish(id, []);
      still = IDLE_FRAMES;
      last = null;
    } else {
      const cur = { x: r.x, y: r.y, w: r.width, h: r.height };
      if (last === null || !sameRect(last, cur)) {
        last = publishFromEl(id, el, corner, r, group);
        still = 0;
        // Morph no dispara ResizeObserver: hits solo cuando el rect cambió
        // (layoutRect usa `--x/--y`, no el bounding escalado).
        if (!surfaces.dragging) surfaces.schedule();
      } else {
        still += 1;
      }
    }
    if (still < IDLE_FRAMES && (surfaces.dragging || frames < MAX_TRACK_FRAMES)) {
      raf = requestAnimationFrame(tick);
    }
  };

  tick();
  return () => cancelAnimationFrame(raf);
}

/**
 * Sigue el DOM cuadro a cuadro (grow/separate, morph en curso). El `$effect`
 * no debe leer `bubble.anchor` cada frame o se reinicia. Idle-stop + tope
 * duro: no dejar rAF eterno por jitter o un follow colgado.
 */
export function publishFollowSkin(
  id: string,
  el: HTMLElement,
  corner: number,
  group?: string,
): () => void {
  let raf = 0;
  let still = 0;
  let last: SkinRect | null = null;
  let frames = 0;

  const tick = () => {
    frames += 1;
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) {
      if (last !== null) {
        liquid.publish(id, []);
        last = null;
      }
      still = IDLE_FRAMES;
    } else {
      const cur = { x: r.x, y: r.y, w: r.width, h: r.height };
      if (last === null || !sameRect(last, cur)) {
        last = publishFromEl(id, el, corner, r, group);
        still = 0;
      } else {
        still += 1;
      }
    }
    if (still < IDLE_FRAMES && (surfaces.dragging || frames < MAX_TRACK_FRAMES)) {
      raf = requestAnimationFrame(tick);
    }
  };

  tick();
  return () => cancelAnimationFrame(raf);
}

/**
 * Varias formas (barra + dots del launcher). Solo republica si la clave cambia;
 * idle-stop + tope duro como `publishEmergeSkin`.
 */
export function publishMeasuredSkin(
  id: string,
  measure: () => { key: string; shapes: Shape[] },
  group?: string,
): () => void {
  let raf = 0;
  let still = 0;
  let frames = 0;
  let lastKey = "";

  const tick = () => {
    frames += 1;
    const { key, shapes } = measure();
    if (key !== lastKey) {
      lastKey = key;
      still = 0;
      liquid.publish(id, shapes, group);
    } else {
      still += 1;
    }
    if (still < IDLE_FRAMES && (surfaces.dragging || frames < MAX_TRACK_FRAMES)) {
      raf = requestAnimationFrame(tick);
    }
  };

  tick();
  return () => cancelAnimationFrame(raf);
}
