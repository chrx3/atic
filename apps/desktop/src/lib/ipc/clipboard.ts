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

/** Ruta de archivo para `startDrag` (imagen; texto usa `startClipboardTextDrag`). */
export const clipboardDragPath = (id: string) =>
  invoke<string>("clipboard_drag_path", { id });
/** OLE de texto plano (`CF_UNICODETEXT`) — no inserta rutas de archivo. */
export const startClipboardTextDrag = (id: string) =>
  invoke<void>("start_clipboard_text_drag", { id });
/** Contenido de un `.atic-drag-*.txt` del historial. */
export const readClipboardDragText = (path: string) =>
  invoke<string>("read_clipboard_drag_text", { path });

/**
 * `fly`: acercar la pill al cursor (atajo global) o expandir donde está.
 * Legacy: el historial ya no crece la pill; usa `showClipboardWindow`.
 */
export const prepareClipboardPill = (fly: boolean) =>
  invoke<number>("prepare_clipboard_pill", { fly });

/** Abre el float de clipboard (idempotente; el overlay decide el cierre). */
export const showClipboardWindow = () => invoke<void>("show_clipboard_window");
export const hideClipboardWindow = () => invoke<void>("hide_clipboard_window");

/** ¿El float de clipboard queda fijado arriba de otras apps? */
export const clipboardAlwaysOnTop = () =>
  invoke<boolean>("clipboard_always_on_top");

/** Fija o desfija el float (always-on-top del overlay mientras está abierto). */
export const setClipboardAlwaysOnTop = (on: boolean) =>
  invoke<void>("set_clipboard_always_on_top", { on });

export const onClipboardBubbleAnchor = (
  cb: (a: import("$core/types").BubbleOpen) => void,
): Promise<UnlistenFn> => on("clipboard-bubble-anchor", cb);

export const onClipboardBubbleDismiss = (cb: () => void): Promise<UnlistenFn> =>
  on("clipboard-bubble-dismiss", cb);

export const onClipboardHistoryChanged = (cb: () => void): Promise<UnlistenFn> =>
  on("clipboard-history-changed", cb);

/** Insertar en el compositor cuando el clipboard pega con agentes abierto. */
export const onAgentsComposerInsert = (
  cb: (payload: AgentsComposerInsert) => void,
): Promise<UnlistenFn> => on("agents-composer-insert", cb);

/** Inserta un ítem del historial en el composer (agentes abierto; no cierra clipboard). */
export const insertClipboardIntoAgents = (id: string) =>
  invoke<void>("insert_clipboard_into_agents", { id });

/** Tras OLE: si el cursor quedó sobre agentes, inserta en el composer. */
export const tryClipboardDropOnAgents = (id: string) =>
  invoke<boolean>("try_clipboard_drop_on_agents", { id });
