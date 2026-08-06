/**
 * Acción primaria de cada herramienta en el picker de la ventana principal.
 *
 * El CTA de la card no “abre” la tool: la ejecuta (grabar, dictar, capturar…).
 * Clipboard y Textos no tienen un one-shot claro: su acción abre el detalle.
 */

import { capture } from "$domain/capture.svelte";
import { dictation } from "$domain/dictation.svelte";
import { showAgentsWindow } from "$ipc/agents";
import { startCaptureSession } from "$ipc/captures";
import type { ToolId } from "./tools";

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
    case "clipboard":
      return { kind: "openDetail", label: "Ver historial" };
    case "snippets":
      return { kind: "openDetail", label: "Ver textos" };
  }
}

export async function runToolAction(id: ToolId): Promise<"openedDetail" | void> {
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
      await showAgentsWindow();
      return;
    case "clipboard":
    case "snippets":
      return "openedDetail";
  }
}
