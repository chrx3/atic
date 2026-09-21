/**
 * Acción primaria de cada herramienta en el picker de la ventana principal.
 *
 * El CTA de la card no “abre” la tool: la ejecuta (grabar, dictar, capturar…).
 * Clipboard / textos / agentes: la isla de la pill (vía slot). Apps: float.
 * Tools con slot espacial delegan en el overlay: flyTo → ejecutar.
 *
 * Vive en `surfaces/` y no en `core/`: orquesta dominio, IPC y slots, y la
 * consumen las DOS superficies que ofrecen la acción (el rail de `main` y la
 * rueda de la pill). Bajarla a `core` era mentir sobre sus dependencias;
 * bajarla a `features` la haría importar hacia arriba (`toolSlots`).
 */

import { capture } from "$domain/capture.svelte";
import { dictation } from "$domain/dictation.svelte";
import { startBoard } from "$ipc/annotate";
import { startCaptureSession, startColorPicker } from "$ipc/captures";
import { showClipboardWindow } from "$ipc/clipboard";
import { showSystemWindow } from "$ipc/system";
import { showLauncher } from "$ipc/search";
import { emit } from "@tauri-apps/api/event";
import { hasToolSlot } from "$surfaces/overlay/toolSlots";
import { t } from "$domain/i18n.svelte";
import { type ToolId } from "$core/tools";

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
        label: dictation.active
          ? t("tools.dictation.stop")
          : t("tools.dictation.start"),
        danger: dictation.active,
      };
    case "captures":
      return { kind: "run", label: t("tools.captures.actionLabel") };
    case "board":
      return { kind: "run", label: t("tools.board.actionLabel") };
    case "color":
      return { kind: "run", label: t("tools.color.actionLabel") };
    case "agents":
      return { kind: "run", label: t("tools.agents.actionLabel") };
    case "launcher":
      return { kind: "run", label: t("tools.launcher.actionLabel") };
    case "clipboard":
      return { kind: "run", label: t("tools.clipboard.actionLabel") };
    case "snippets":
      return { kind: "run", label: t("tools.snippets.actionLabel") };
    case "system":
      return { kind: "run", label: t("tools.system.actionLabel") };
  }
}

/** Pedir al overlay: volar al slot y ejecutar la tool. */
export const requestActivateAtSlot = (tool: ToolId) => emit("activate-tool-slot", tool);

/**
 * Ejecuta la acción sin pasar por el vuelo al slot.
 * Lo usa el overlay después de `flyTo`, o tools sin slot.
 * Textos y agentes viven en la isla; no reabrir el float.
 */
export async function executeToolAction(id: ToolId): Promise<"openedDetail" | void> {
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
    case "color":
      await startColorPicker();
      return;
    case "agents":
      // La isla de la pill es la superficie. Un handler viejo del overlay
      // no debe volver a abrir el float.
      return;
    case "launcher":
      await showLauncher();
      return;
    case "clipboard":
      await showClipboardWindow();
      return;
    case "system":
      await showSystemWindow();
      return;
    case "snippets":
      return;
  }
}

/**
 * Acción primaria desde catálogo / pestañas.
 * Si la tool tiene slot, el overlay vuela y ejecuta; si no, corre acá.
 */
export async function runToolAction(id: ToolId): Promise<"openedDetail" | void> {
  if (hasToolSlot(id)) {
    await requestActivateAtSlot(id);
    return;
  }
  return executeToolAction(id);
}
