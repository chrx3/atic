/**
 * Avisos de estado (modelo, esfuerzo, permisos…) que no son chat:
 * se filtran del hilo y se muestran como toast.
 */
import { effortShortLabel, modelLabelFor } from "$lib/agentModels";
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
      t.startsWith("Compactando el contexto") ||
      t.startsWith("Contexto compactado")
    );
  }
  if (item.kind === "message" && item.role === "assistant" && !item.streaming) {
    const t = item.text.trim();
    return /^Set model to /i.test(t) || /^Set effort level to /i.test(t);
  }
  return false;
}

/**
 * Texto corto para toast, o null si el ítem se oculta sin notificar
 * (ecos en inglés del CLI: ya hubo notice en español).
 */
export function statusToastMessage(
  item: AgentItem,
  models: { id: string; name?: string; label?: string }[] = [],
): string | null {
  if (item.kind !== "notice") return null;

  if (item.text.startsWith("Modelo:")) {
    const rest = item.text.slice("Modelo:".length).trim();
    if (!rest || /elegí/i.test(rest)) return item.text;
    return `Modelo: ${modelLabelFor(rest, models)}`;
  }
  if (item.text.startsWith("Esfuerzo:")) {
    const rest = item.text.slice("Esfuerzo:".length).trim();
    if (!rest || /elegí/i.test(rest)) return item.text;
    return `Esfuerzo: ${effortShortLabel(rest)}`;
  }
  if (
    item.text.startsWith("Plan mode") ||
    item.text.startsWith("Permisos:") ||
    item.text.startsWith("Limpiando la conversación") ||
    item.text.startsWith("Consultando uso") ||
    item.text.startsWith("Consultando costo") ||
    item.text.startsWith("Compactando el contexto") ||
    item.text.startsWith("Contexto compactado")
  ) {
    // Primera línea: el cuerpo largo no cabe en un toast.
    return item.text.split("\n")[0]?.trim() || item.text;
  }
  return null;
}
