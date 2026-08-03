/** Historial del portapapeles. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { AgentsComposerInsert, ClipboardItem } from "$core/types";
import { on } from "./events";

export type { AgentsComposerInsert };

export const listClipboardHistory = () =>
  invoke<ClipboardItem[]>("list_clipboard_history");
export const pasteClipboardItem = (id: string) =>
  invoke<void>("paste_clipboard_item", { id });
export const pinClipboardItem = (id: string, pinned: boolean) =>
  invoke<void>("pin_clipboard_item", { id, pinned });
export const deleteClipboardItem = (id: string) =>
  invoke<void>("delete_clipboard_item", { id });

/** Ruta de archivo para `startDrag` (imagen o .atic-drag-*.txt). */
export const clipboardDragPath = (id: string) =>
  invoke<string>("clipboard_drag_path", { id });
/** Contenido de un `.atic-drag-*.txt` del historial. */
export const readClipboardDragText = (path: string) =>
  invoke<string>("read_clipboard_drag_text", { path });

/**
 * `fly`: acercar la pill al cursor (atajo global) o expandir donde está.
 * Devuelve los ms que dura el vuelo (0 si no vuela): hay que esperarlos antes
 * de expandir el panel, o el reencuadre se ancla a mitad del recorrido.
 */
export const prepareClipboardPill = (fly: boolean) =>
  invoke<number>("prepare_clipboard_pill", { fly });

export const onClipboardHistoryChanged = (cb: () => void): Promise<UnlistenFn> =>
  on("clipboard-history-changed", cb);

/** Insertar en el compositor cuando el clipboard pega con agentes abierto. */
export const onAgentsComposerInsert = (
  cb: (payload: AgentsComposerInsert) => void,
): Promise<UnlistenFn> => on("agents-composer-insert", cb);
