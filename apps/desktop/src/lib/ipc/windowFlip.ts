/** Prototipo: reverso de notas sobre una ventana ajena. */

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { InkStroke, NoteBlock, WindowFlipView } from "$core/types";
import { on } from "./events";

export type { InkStroke, NoteBlock, WindowFlipView };

export const windowFlipState = () => invoke<WindowFlipView | null>("window_flip_state");

export const saveWindowFlipBlocks = (blocks: NoteBlock[]) =>
  invoke<void>("window_flip_save_blocks", { blocks });

/** Los bytes no viajan: Rust lee la imagen del portapapeles del sistema. */
export const pasteWindowFlipImage = () =>
  invoke<{ asset: string; width: number; height: number }>("window_flip_paste_image");

/** Trae al bloc una imagen que ya está en disco (el historial del portapapeles). */
export const importWindowFlipImage = (path: string) =>
  invoke<{ asset: string; width: number; height: number }>("window_flip_import_image", {
    path,
  });

/** `false` si el foco se fue a otra ventana de Atic: ahí no hay que cerrar. */
export const windowFlipFocusIsForeign = () =>
  invoke<boolean>("window_flip_focus_is_foreign");

export const windowFlipAssetSrc = (assetsDir: string, asset: string) =>
  convertFileSrc(`${assetsDir}/${asset}`);

/** Data URL mismo-origen para rasterizar el tablero sin contaminar el canvas. */
export const windowFlipAssetData = (asset: string) =>
  invoke<string>("window_flip_asset_data", { asset });

export const closeWindowFlip = () => invoke<void>("window_flip_close");

export const presentWindowFlip = () => invoke<void>("window_flip_present");

export const concealWindowFlip = () => invoke<void>("window_flip_conceal");

export const refreshWindowFlipPreview = () =>
  invoke<WindowFlipView>("window_flip_refresh_preview");

export const windowFlipPreviewSrc = (path: string) =>
  path ? convertFileSrc(path) : "";

export const exportWindowFlip = (
  format: string,
  path: string,
  pages: {
    previewBase64: string;
    previewMime: string;
    fotos: {
      imageBase64: string;
      mime: string;
      x: number;
      y: number;
      w: number;
      h: number;
    }[];
    textos: { body: string; x: number; y: number; w: number; h: number }[];
    checks: {
      items: { text: string; done: boolean }[];
      x: number;
      y: number;
      w: number;
      h: number;
    }[];
    inkBase64: string;
  }[],
  /** Tamaño de la celda del tablero: de acá sale la página del PDF y del PPT. */
  pageW: number,
  pageH: number,
) => invoke<string>("window_flip_export", { format, path, pages, pageW, pageH });

export const onWindowFlipOpen = (
  cb: (view: WindowFlipView) => void,
): Promise<UnlistenFn> => on("window-flip-open", cb);

export const onWindowFlipRequestClose = (cb: () => void): Promise<UnlistenFn> =>
  on("window-flip-request-close", cb);
