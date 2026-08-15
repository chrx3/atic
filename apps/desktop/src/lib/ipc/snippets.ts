/** Fragmentos, bloc y notas. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { Note, Scratchpad, Snippet } from "$core/types";
import { on } from "./events";

export const listSnippets = () => invoke<Snippet[]>("list_snippets");
export const upsertSnippet = (snippet: Snippet) =>
  invoke<Snippet>("upsert_snippet", { snippet });
export const deleteSnippet = (id: string) => invoke<void>("delete_snippet", { id });
export const pasteSnippet = (id: string) => invoke<void>("paste_snippet", { id });

/** Legacy: el panel ya no crece la pill. */
export const prepareSnippetsPill = (fly: boolean) =>
  invoke<number>("prepare_snippets_pill", { fly });

/** Abre el float de textos (idempotente; el overlay decide el cierre). */
export const showSnippetsWindow = () => invoke<void>("show_snippets_window");
export const hideSnippetsWindow = () => invoke<void>("hide_snippets_window");

/** ¿El float de textos queda fijado arriba de otras apps? */
export const snippetsAlwaysOnTop = () =>
  invoke<boolean>("snippets_always_on_top");

/** Fija o desfija el float (always-on-top del overlay mientras está abierto). */
export const setSnippetsAlwaysOnTop = (on: boolean) =>
  invoke<void>("set_snippets_always_on_top", { on });

export const onSnippetsBubbleAnchor = (
  cb: (a: import("$core/types").BubbleOpen) => void,
): Promise<UnlistenFn> => on("snippets-bubble-anchor", cb);

export const onSnippetsBubbleDismiss = (cb: () => void): Promise<UnlistenFn> =>
  on("snippets-bubble-dismiss", cb);

export const getScratchpad = () => invoke<Scratchpad>("get_scratchpad");
export const setScratchpad = (body: string) =>
  invoke<Scratchpad>("set_scratchpad", { body });

/** Notas guardadas, de la más reciente a la más vieja. */
export const listNotes = () => invoke<Note[]>("list_notes");
/** Crea o actualiza. Sin `id` crea una nueva. `null` = cuerpo vacío, no guarda. */
export const saveNote = (id: string | null, body: string) =>
  invoke<Note | null>("save_note", { id, body });
export const deleteNote = (id: string) => invoke<void>("delete_note", { id });

export const onSnippetsChanged = (cb: () => void): Promise<UnlistenFn> =>
  on("snippets-changed", cb);
