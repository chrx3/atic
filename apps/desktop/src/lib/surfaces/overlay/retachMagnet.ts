/**
 * Imán del re-acople: un panel despegado que se suelta cerca de la pill
 * vuelve a la isla.
 *
 * Dos reglas, y las dos hacen falta:
 *
 * 1. **Decide solo al soltar.** Absorber durante el arrastre se traga el
 *    panel cuando la mano pasa rápido cerca de la pill.
 * 2. **Y solo con el gesto ARMADO.** El panel despegado nace en el rect de la
 *    cara, pegado al notch (gap ≈ 0): sin armar, el propio gesto del despegue
 *    lo volvería a pegar al soltar. Se arma al empezar lejos, o al salir de la
 *    zona durante el arrastre.
 *
 * Aparte, soltar con el CURSOR sobre la pill también coloca: apuntar a la pill
 * es una intención clara aunque el marco nunca haya salido de la zona. Se arma
 * igual que el marco —el cursor tiene que haber estado fuera de la pill en el
 * gesto—, porque el despegue arranca con la mano sobre la cara que colapsa.
 *
 * Las superficies despegables (historial, textos, sistema, agentes) comparten
 * estas reglas: el imán no puede leerse distinto según el panel.
 */
import { gapBetween } from "$liquid/geometry";

export type MagnetRect = { x: number; y: number; w: number; h: number };
export type MagnetPoint = { x: number; y: number };

/** Gap (px) desde el cual el panel deja de contar como pegado a la pill. */
export const RETACH_GAP_PX = 64;

/**
 * ¿El marco está fuera del alcance del imán?
 *
 * Sin pill o sin marco medidos se asume que sí: no hay nada de qué despegarse,
 * y un imán que se arma con medidas que no tenemos pega paneles sin pedido.
 */
export function awayFromPill(
  pill: MagnetRect | null | undefined,
  frame: MagnetRect | null | undefined,
  gap = RETACH_GAP_PX,
): boolean {
  if (!pill || !frame) return true;
  return gapBetween(pill, frame) > gap;
}

/** Soltado: ¿se coloca en la isla? Solo armado Y dentro del alcance. */
export function retachesOnDrop(
  armed: boolean,
  pill: MagnetRect | null | undefined,
  frame: MagnetRect | null | undefined,
  gap = RETACH_GAP_PX,
): boolean {
  if (!armed || !pill || !frame) return false;
  return gapBetween(pill, frame) <= gap;
}

/** ¿El cursor está sobre la pill? Sin medidas, no. */
export function cursorOnPill(
  pill: MagnetRect | null | undefined,
  cursor: MagnetPoint | null | undefined,
): boolean {
  if (!pill || !cursor) return false;
  return (
    cursor.x >= pill.x &&
    cursor.x <= pill.x + pill.w &&
    cursor.y >= pill.y &&
    cursor.y <= pill.y + pill.h
  );
}

/** Armado del imán durante UN gesto: por el marco y por el cursor. */
export type RetachGesture = { frameArmed: boolean; cursorArmed: boolean };

export function createRetachGesture(): RetachGesture {
  return { frameArmed: false, cursorArmed: false };
}

/** Suma la muestra actual al armado. Una vez armado, queda armado. */
export function trackRetach(
  gesture: RetachGesture,
  pill: MagnetRect | null | undefined,
  frame: MagnetRect | null | undefined,
  cursor: MagnetPoint | null | undefined,
): void {
  if (!gesture.frameArmed) gesture.frameArmed = awayFromPill(pill, frame);
  if (!gesture.cursorArmed && cursor) gesture.cursorArmed = !cursorOnPill(pill, cursor);
}

/** ¿Soltar ahora coloca el panel? Sirve para decidir al soltar y para la vista previa. */
export function retachReady(
  gesture: RetachGesture,
  pill: MagnetRect | null | undefined,
  frame: MagnetRect | null | undefined,
  cursor: MagnetPoint | null | undefined,
): boolean {
  if (retachesOnDrop(gesture.frameArmed, pill, frame)) return true;
  return gesture.cursorArmed && cursorOnPill(pill, cursor);
}
