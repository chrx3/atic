/**
 * Posiciones de viewport por herramienta (fase 2).
 *
 * Solo launcher, agentes y dictado vuelan a un slot fijo. Clipboard y snippets
 * abren junto a la pill / cursor (sin slot): el atajo hace flyTo(cursor) en
 * PillSurface y recién ahí monta el float.
 *
 * Pendiente:
 * - Overrides de slot en Ajustes (API lista vía `setSlotOverrides`).
 * - Return-to-home al cerrar: cableado en la pill; puede desactivarse luego.
 *
 * Launcher: barra centrada en el work area; la pill vuela a
 * `center-left-of-launcher` (izquierda de la barra). Favs fuera del stadium
 * (hit-rect `launcher-favs`; gap > REACH para cortar el goo).
 */

import type { ToolId } from "$core/tools";
import type { Area } from "$ipc/overlay";
import { MARGIN } from "./contract";

export type SlotId =
  | "center"
  | "bottom-center"
  | "center-right"
  | "center-left"
  | "center-left-of-launcher"
  | "bottom-right";

export type Point = { x: number; y: number };
export type Size = { w: number; h: number };

/** Ancho de la barra stadium (alineado a `LAUNCHER_SHAPE.w` en launcher.rs). */
export const LAUNCHER_BAR_W = 292;
/** Hueco entre el borde derecho de la pill y el izquierdo de la barra. */
export const LAUNCHER_PILL_GAP = 16;
/**
 * Hueco bajo slots `bottom-*` respecto al borde de pantalla.
 *
 * `overlay_work_areas` devuelve **bounds** (pantalla completa, modo DI), no el
 * `work_area` de Win32. Sin este inset, dictado queda montado sobre
 * la taskbar. ~48px barra + 8px de aire (mismo espíritu que `CORNER_MARGIN`
 * del shelf en `floating.rs`).
 */
export const BOTTOM_SLOT_INSET = 56;

/** Defaults. Tools sin entrada quedan sin vuelo (abre junto a la pill/cursor). */
const DEFAULT_SLOTS: Partial<Record<ToolId, SlotId>> = {
  launcher: "center-left-of-launcher",
  dictation: "bottom-center",
};

/** Overrides opcionales (Ajustes / tests). Vacío = solo defaults. */
let slotOverrides: Partial<Record<ToolId, SlotId>> = {};

/** Sustituye overrides; pasar `{}` o `null` limpia. */
export function setSlotOverrides(
  next: Partial<Record<ToolId, SlotId>> | null,
): void {
  slotOverrides = next ? { ...next } : {};
}

export function slotForTool(tool: ToolId): SlotId | null {
  return slotOverrides[tool] ?? DEFAULT_SLOTS[tool] ?? null;
}

export function hasToolSlot(tool: ToolId): boolean {
  return slotForTool(tool) != null;
}

/** Tools espaciales: floats + launcher (exclusive entre sí). */
export function isSpatialTool(tool: ToolId): boolean {
  return (
    tool === "launcher" ||
    tool === "clipboard" ||
    tool === "snippets" ||
    tool === "agents"
  );
}

function areaAt(areas: Area[], point: Point): Area | null {
  if (areas.length === 0) return null;
  return (
    areas.find(
      (a) =>
        point.x >= a.x &&
        point.x <= a.x + a.w &&
        point.y >= a.y &&
        point.y <= a.y + a.h,
    ) ?? areas[0]
  );
}

function resolveInArea(slot: SlotId, area: Area, size: Size): Point {
  const maxX = Math.max(area.x + area.w - size.w - MARGIN, area.x + MARGIN);
  const maxY = Math.max(area.y + area.h - size.h - MARGIN, area.y + MARGIN);
  let x: number;
  let y: number;
  switch (slot) {
    case "center":
      x = area.x + (area.w - size.w) / 2;
      y = area.y + (area.h - size.h) / 2;
      break;
    case "bottom-center":
      x = area.x + (area.w - size.w) / 2;
      y = area.y + area.h - size.h - MARGIN - BOTTOM_SLOT_INSET;
      break;
    case "center-right":
      x = area.x + area.w - size.w - MARGIN;
      y = area.y + (area.h - size.h) / 2;
      break;
    case "center-left":
      x = area.x + MARGIN;
      y = area.y + (area.h - size.h) / 2;
      break;
    case "center-left-of-launcher": {
      // Pill a la izquierda de la barra centrada: centerX - barW/2 - gap - pillW.
      const cx = area.x + area.w / 2;
      const cy = area.y + area.h / 2;
      x = cx - LAUNCHER_BAR_W / 2 - LAUNCHER_PILL_GAP - size.w;
      y = cy - size.h / 2;
      break;
    }
    case "bottom-right":
      x = area.x + area.w - size.w - MARGIN;
      y = area.y + area.h - size.h - MARGIN - BOTTOM_SLOT_INSET;
      break;
  }
  return {
    x: Math.min(Math.max(x, area.x + MARGIN), maxX),
    y: Math.min(Math.max(y, area.y + MARGIN), maxY),
  };
}

/**
 * Esquina superior-izquierda de `size` en el slot pedido.
 *
 * `anchor` elige el monitor (mouse / ventana con foco, no la pill).
 */
export function resolveSlot(
  slot: SlotId,
  areas: Area[],
  size: Size,
  anchor: Point,
): Point {
  const area = areaAt(areas, anchor);
  if (!area) {
    const vw = typeof window !== "undefined" ? window.innerWidth : 800;
    const vh = typeof window !== "undefined" ? window.innerHeight : 600;
    return resolveInArea(slot, { x: 0, y: 0, w: vw, h: vh }, size);
  }
  return resolveInArea(slot, area, size);
}
