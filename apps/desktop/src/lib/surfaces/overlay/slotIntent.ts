/**
 * Intención de un atajo / activate-tool-slot.
 *
 * El atajo nombra un destino: abrir esa tool, o cerrarla si ya es la activa
 * y el cursor sigue ahí. Clipboard / textos se anclan al cursor: si ya
 * están abiertos y el puntero se fue, el atajo los reubica (cierra y abre
 * donde estás), no los apaga. No es “toggle del overlay”.
 *
 * La rueda entra por el mismo camino, pero con `force`: apuntar a un gajo y
 * soltar es pedir «abrí esto», nunca «alterná esto». Sin el flag, elegir
 * Clipboard en la rueda con el clipboard ya abierto lo habría cerrado.
 */

import type { ToolId } from "$core/tools";
import { isSpatialTool } from "./toolSlots";

export type SlotIntent = "close" | "show" | "relocate";

/**
 * Un pedido de activación, con su origen.
 *
 * Es lo que viaja por la cola: sin el `force` adentro, un pedido de la rueda
 * encolado detrás de un atajo se ejecutaba después con las reglas del atajo.
 */
export type SlotRequest = { id: ToolId; force?: boolean };

const SPATIAL_ORDER: ToolId[] = [
  "clipboard",
  "snippets",
  "agents",
  "launcher",
];

/** Clipboard y textos vuelan al cursor; launcher / agentes tienen slot fijo. */
export function isCursorAnchored(id: ToolId): boolean {
  return id === "clipboard" || id === "snippets";
}

/** Distancia (px) entre la pill y el destino centrado en el cursor. */
export function pillToCursorMovePx(
  pill: { x: number; y: number },
  size: { w: number; h: number },
  cursor: { x: number; y: number } | null,
): number {
  if (!cursor) return 0;
  return Math.hypot(
    cursor.x - size.w / 2 - pill.x,
    cursor.y - size.h / 2 - pill.y,
  );
}

/**
 * Misma tool espacial ya visible → cerrar, salvo clipboard/textos con el
 * cursor lejos: entonces relocate. Cualquier otro pedido → mostrar.
 *
 * `force` corta antes de todo: el pedido vino de la rueda, donde el usuario
 * ya señaló qué quiere abierto.
 */
export function slotIntent(
  requested: ToolId,
  requestedIsOpen: boolean,
  movePx = 0,
  skipIfNear = 48,
  opts: { force?: boolean } = {},
): SlotIntent {
  if (opts.force) return "show";
  if (!isSpatialTool(requested) || !requestedIsOpen) return "show";
  if (isCursorAnchored(requested) && movePx >= skipIfNear) return "relocate";
  return "close";
}

/**
 * Floats a cerrar al mostrar `keep`. El pin conserva clipboard / textos /
 * agentes; el launcher no se fija y siempre cede.
 */
export function spatialDismissTargets(
  keep: ToolId | undefined,
  pinned: {
    clipboard?: boolean;
    snippets?: boolean;
    agents?: boolean;
  },
): ToolId[] {
  return SPATIAL_ORDER.filter((id) => {
    if (id === keep) return false;
    if (id === "clipboard" && pinned.clipboard) return false;
    if (id === "snippets" && pinned.snippets) return false;
    if (id === "agents" && pinned.agents) return false;
    return true;
  });
}

/**
 * Mientras hay un acto en curso, el último pedido pisa el pendiente.
 * Si está idle, hay que arrancar ya (`pending` queda vacío).
 *
 * Genérico en el payload: la cola guarda el `SlotRequest` entero (id + force),
 * no solo el id. Los tests siguen pasando `ToolId` sueltos.
 */
export function enqueueActivate<T>(
  busy: boolean,
  next: T,
): { start: boolean; pending: T | null } {
  if (busy) return { start: false, pending: next };
  return { start: true, pending: null };
}

/** No abrir A si el usuario ya pidió B a mitad del vuelo. */
export function shouldCommitShow(pending: unknown): boolean {
  return pending == null;
}

/** Volver a casa solo si nadie pidió otra tool durante el cierre. */
export function shouldReturnHomeAfterClose(pending: unknown): boolean {
  return pending == null;
}
