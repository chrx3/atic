/**
 * Acción primaria de cada herramienta en el picker de la ventana principal.
 *
 * El CTA de la card no “abre” la tool: la ejecuta (grabar, dictar, capturar…).
 * Clipboard / textos / agentes / Apps: abren float (vía slot si hay).
 * Tools con slot espacial delegan en el overlay: flyTo → ejecutar.
 */

import { capture } from "$domain/capture.svelte";
import { dictation } from "$domain/dictation.svelte";
import { showAgentsWindow } from "$ipc/agents";
import { startCaptureSession } from "$ipc/captures";
import { showClipboardWindow } from "$ipc/clipboard";
import { showLauncher } from "$ipc/search";
import { showSnippetsWindow } from "$ipc/snippets";
import { emit } from "@tauri-apps/api/event";
import { hasToolSlot } from "$surfaces/overlay/toolSlots";
import { AGENTS_ENABLED, type ToolId } from "./tools";

export type ToolActionKind = "run" | "openDetail";

export type ToolAction = {
  kind: ToolActionKind;
  /** Etiqueta del botón (puede cambiar con el estado, p.ej. Grabar/Parar). */
  label: string;
  /** Variante visual cuando la acción está “en curso”. */
  danger?: boolean;
  busy?: boolean;
};

export function toolAction(id: ToolId): ToolAction {
  switch (id) {
    case "meetings":
      return {
        kind: "run",
        label: capture.active ? "Parar" : "Grabar",
        danger: capture.active,
        busy: capture.busy,
      };
    case "dictation":
      return {
        kind: "run",
        label: dictation.active ? "Terminar" : "Dictar",
        danger: dictation.active,
      };
    case "captures":
      return { kind: "run", label: "Tomar captura" };
    case "agents":
      return { kind: "run", label: "Abrir consola" };
    case "launcher":
      return { kind: "run", label: "Buscar apps" };
    case "clipboard":
      return { kind: "run", label: "Ver historial" };
    case "snippets":
      return { kind: "run", label: "Ver textos" };
  }
}

/** Pedir al overlay: volar al slot y ejecutar la tool. */
export const requestActivateAtSlot = (tool: ToolId) =>
  emit("activate-tool-slot", tool);

/**
 * Ejecuta la acción sin pasar por el vuelo al slot.
 * Lo usa el overlay después de `flyTo`, o tools sin slot.
 * Clipboard / textos / Apps: abrir (idempotente), no toggle.
 */
export async function executeToolAction(
  id: ToolId,
): Promise<"openedDetail" | void> {
  switch (id) {
    case "meetings":
      await capture.toggle();
      return;
    case "dictation":
      await dictation.toggle();
      return;
    case "captures":
      await startCaptureSession();
      return;
    case "agents":
      if (AGENTS_ENABLED) await showAgentsWindow();
      return;
    case "launcher":
      await showLauncher();
      return;
    case "clipboard":
      await showClipboardWindow();
      return;
    case "snippets":
      await showSnippetsWindow();
      return;
  }
}

/**
 * Acción primaria desde catálogo / ToolRail.
 * Si la tool tiene slot, el overlay vuela y ejecuta; si no, corre acá.
 */
export async function runToolAction(id: ToolId): Promise<"openedDetail" | void> {
  if (hasToolSlot(id)) {
    await requestActivateAtSlot(id);
    return;
  }
  return executeToolAction(id);
}
