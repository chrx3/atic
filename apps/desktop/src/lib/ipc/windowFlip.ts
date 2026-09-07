/** Prototipo: reverso de notas sobre una ventana ajena. */

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { on } from "./events";

export interface WindowFlipView {
  key: string;
  title: string;
  exe: string;
  previewPath: string;
  note: string;
  cardLeft: number;
  cardTop: number;
  cardWidth: number;
  cardHeight: number;
}

export const windowFlipState = () => invoke<WindowFlipView | null>("window_flip_state");

export const saveWindowFlipNote = (body: string) =>
  invoke<void>("window_flip_save_note", { body });

export const closeWindowFlip = () => invoke<void>("window_flip_close");

export const refreshWindowFlipPreview = () =>
  invoke<WindowFlipView>("window_flip_refresh_preview");

export const windowFlipPreviewSrc = (path: string) =>
  path ? convertFileSrc(path) : "";

export const onWindowFlipOpen = (
  cb: (view: WindowFlipView) => void,
): Promise<UnlistenFn> => on("window-flip-open", cb);

export const onWindowFlipRequestClose = (cb: () => void): Promise<UnlistenFn> =>
  on("window-flip-request-close", cb);
