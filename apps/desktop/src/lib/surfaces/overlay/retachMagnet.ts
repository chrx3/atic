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
 * Las tres superficies despegables (historial, textos, agentes) comparten
 * estas reglas: el imán no puede leerse distinto según el panel.
 */
import { gapBetween } from "$liquid/geometry";

export type MagnetRect = { x: number; y: number; w: number; h: number };

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
