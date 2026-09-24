/**
 * Avisos de estado (modelo, esfuerzo, permisos…) que no son chat: se
 * filtran del hilo. El estado ya se ve en los selectores del composer.
 */
import type { AgentItem } from "$lib/types";

/** Notice / eco del CLI que no debe ocupar burbujas en la conversación. */
export function isChatStatusNoise(item: AgentItem): boolean {
  if (item.kind === "notice") {
    const t = item.text;
    return (
      t.startsWith("Modelo:") ||
      t.startsWith("Esfuerzo:") ||
      t.startsWith("Plan mode") ||
      t.startsWith("Permisos:") ||
      t.startsWith("Limpiando la conversación") ||
      t.startsWith("Consultando uso") ||
      t.startsWith("Consultando costo") ||
      // Eventos `system` del CLI sin traducir (hooks, etc.): telemetría.
      t.startsWith("sistema: ")
    );
  }
  if (item.kind === "message" && item.role === "assistant" && !item.streaming) {
    const t = item.text.trim();
    return /^Set model to /i.test(t) || /^Set effort level to /i.test(t);
  }
  return false;
}
