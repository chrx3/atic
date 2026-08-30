/**
 * Acción primaria de cada herramienta en el picker de la ventana principal.
 *
 * El CTA de la card no “abre” la tool: la ejecuta (grabar, dictar, capturar…).
 * Clipboard / textos / agentes / Apps: abren float (vía slot si hay).
 * Tools con slot espacial delegan en el overlay: flyTo → ejecutar.
 */

import { capture } from "$domain/capture.svelte";
import { dictation } from "$domain/dictation.svelte";
import { presentAgentsWindow } from "$ipc/agents";
import { startBoard } from "$ipc/annotate";
import { startCaptureSession } from "$ipc/captures";
import { showClipboardWindow } from "$ipc/clipboard";
import { showLauncher } from "$ipc/search";
import { showSnippetsWindow } from "$ipc/snippets";
import { emit } from "@tauri-apps/api/event";
import { hasToolSlot } from "$surfaces/overlay/toolSlots";
import { t } from "$domain/i18n.svelte";
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
        label: capture.active ? t("tools.meetings.stop") : t("tools.meetings.record"),
        danger: capture.active,
        busy: capture.busy,
      };
    case "dictation":
      return {
        kind: "run",
        label: dictation.active ? t("tools.dictation.stop") : t("tools.dictation.start"),
        danger: dictation.active,
      };
    case "captures":
      return { kind: "run", label: t("tools.captures.actionLabel") };
    case "board":
      return { kind: "run", label: t("tools.board.actionLabel") };
    case "agents":
      return { kind: "run", label: t("tools.agents.actionLabel") };
    case "launcher":
      return { kind: "run", label: t("tools.launcher.actionLabel") };
    case "clipboard":
      return { kind: "run", label: t("tools.clipboard.actionLabel") };
    case "snippets":
      return { kind: "run", label: t("tools.snippets.actionLabel") };
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
    case "board":
      await startBoard();
      return;
    case "agents":
      if (AGENTS_ENABLED) await presentAgentsWindow();
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
