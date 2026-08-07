/**
 * El escenario de la pill, dentro del overlay.
 *
 * Reemplaza a `createStage` pieza por pieza: **misma interfaz, otro ejecutor**.
 * Donde aquel mandaba un IPC que movía una ventana del sistema, este escribe
 * `left`/`top` en un elemento. Todo lo de arriba —`reconcile`, `pivotFor`,
 * `collapsingFrom`, la regla de crecer-antes/encoger-después— sigue igual, y no
 * es casualidad: esa lógica es conocimiento ganado a golpes sobre CUÁNDO y CON
 * QUÉ PUNTO FIJO reencuadrar, y eso no cambia porque el rectángulo pase de ser
 * una ventana a ser un div.
 *
 * Lo que sí desaparece es la coreografía asíncrona. Mover un div es síncrono,
 * así que ya no hay dos escritores compitiendo por un rectángulo: se van el
 * contador de generaciones, los destinos obsoletos y `adopt()`. `resize` sigue
 * siendo `async` solo porque el pivote `cursor` tiene que preguntarle la
 * posición del puntero a Rust.
 */

import { overlayCursor, overlayWorkAreas, type Area, type Point } from "$ipc/overlay";

import { MARGIN } from "./contract";
import { sameSize, type Pivot, type ResizeOutcome, type Size } from "./pillStage";

export type { Point };

/**
 * Encaja un rectángulo dentro del área útil del monitor que le corresponde.
 *
 * Es el `clamp` de Rust, con su misma regla: el monitor se elige por el
 * **centro** del rectángulo, no por su esquina. Con la esquina, algo a caballo
 * entre dos pantallas saltaba a la otra.
 *
 * Hace falta acá porque el overlay abarca el escritorio virtual entero: su
 * propio borde no dice nada sobre dónde termina la pantalla en la que estás.
 */
function clampTo(areas: Area[], p: Point, size: Size): Point {
  if (areas.length === 0) return p;
  const cx = p.x + size.w / 2;
  const cy = p.y + size.h / 2;
  const area =
    areas.find((a) => cx >= a.x && cx <= a.x + a.w && cy >= a.y && cy <= a.y + a.h) ??
    areas[0];

  // max contra el mínimo: en un monitor más chico que la superficie, el clamp
  // invertido la mandaba fuera de pantalla.
  const maxX = Math.max(area.x + area.w - size.w - MARGIN, area.x + MARGIN);
  const maxY = Math.max(area.y + area.h - size.h - MARGIN, area.y + MARGIN);
  return {
    x: Math.min(Math.max(p.x, area.x + MARGIN), maxX),
    y: Math.min(Math.max(p.y, area.y + MARGIN), maxY),
  };
}

/** ¿Entra el panel hacia abajo desde `y`? */
function fitsBelow(areas: Area[], p: Point, size: Size): boolean {
  const cx = p.x + size.w / 2;
  const area = areas.find((a) => cx >= a.x && cx <= a.x + a.w) ?? areas[0];
  if (!area) return true;
  return p.y + size.h + MARGIN <= area.y + area.h;
}

export function createCssStage() {
  let current: Size | null = null;
  /** Esquina de la superficie, en px CSS del overlay. */
  let origin: Point = { x: 0, y: 0 };
  let areas: Area[] = [];

  /** Se refresca al arrancar y cuando cambian los monitores. */
  async function loadAreas(): Promise<void> {
    try {
      areas = await overlayWorkAreas();
    } catch {
      areas = [];
    }
  }

  function applied(): Size | null {
    return current;
  }

  /**
   * Copia y no la referencia interna, a propósito.
   *
   * Quien llama guarda esto en `$state`. Devolver el mismo objeto hacía que
   * asignarlo fuera un no-op para Svelte —misma referencia, sin cambio— y la
   * pill se quedaba dibujada donde estaba aunque el escenario ya la hubiera
   * movido.
   */
  function at(): Point {
    return { ...origin };
  }

  /** Coloca la esquina sin tocar el tamaño (arrastre, hogar). */
  function moveTo(p: Point): void {
    origin = current ? clampTo(areas, p, current) : p;
  }

  /**
   * Lleva la superficie a `target` conservando el punto que indica `pivot`.
   *
   * Devuelve siempre `ok: true`: no hay destinos obsoletos que descartar
   * porque escribir en el DOM no es asíncrono. La firma conserva el campo para
   * que `reconcile()` no tenga que cambiar.
   */
  async function resize(
    target: Size,
    pivot: Pivot = "topLeft",
    _animate = false,
  ): Promise<ResizeOutcome> {
    if (pivot !== "cursor" && current && sameSize(current, target)) {
      return { ok: true, up: false };
    }

    const from = current;
    let next: Point = { ...origin };
    let up = false;

    switch (pivot) {
      case "cursor": {
        // Teleport al puntero (centrando el rectángulo). La apertura normal
        // de la rueda ya no lo usa: morflea in-situ con `center`. El summon
        // (`pill-reset`) va por `flyTo` + `overlayCursor` en el componente.
        const cursor = await overlayCursor().catch(() => null);
        const c = cursor ?? { x: origin.x, y: origin.y };
        next = { x: c.x - target.w / 2, y: c.y - target.h / 2 };
        break;
      }
      case "center": {
        // Conserva el centro: la marca del medio es el punto fijo del morph.
        if (from) {
          next = {
            x: origin.x + (from.w - target.w) / 2,
            y: origin.y + (from.h - target.h) / 2,
          };
        }
        break;
      }
      case "bottomLeft": {
        // Al encoger, el tamaño chico entra "hacia arriba": la barra queda
        // clavada donde estaba, abajo de la ventana.
        if (from) next = { x: origin.x, y: origin.y + (from.h - target.h) };
        break;
      }
      case "panel": {
        // Abrir el panel: la barra no se mueve si cae hacia abajo, y si no
        // entra, el panel sale hacia arriba y la barra queda abajo.
        const barTop = origin;
        up = !fitsBelow(areas, barTop, target);
        next = up ? { x: origin.x, y: origin.y - (target.h - (from?.h ?? 0)) } : barTop;
        break;
      }
      default:
        // topLeft: la esquina se queda donde está. Es el caso de la barra
        // compacta, donde el ancho cambia solo —entra el timer, tictaquea,
        // aparece el badge— y cualquier otro pivote la haría derivar.
        break;
    }

    origin = clampTo(areas, next, target);
    current = target;
    return { ok: true, up };
  }

  /** Copia de las áreas útiles (para decidir lado de consola, etc.). */
  function workAreas(): Area[] {
    return areas.map((a) => ({ ...a }));
  }

  return { resize, applied, at, moveTo, loadAreas, workAreas };
}
