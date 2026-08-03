/** Modelos de transcripción: catálogo y descarga. */

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { DownloadProgress, ModelStatus } from "$core/types";
import { on } from "./events";

export const listModels = () => invoke<ModelStatus[]>("list_models");
export const currentModelReady = () => invoke<boolean>("current_model_ready");
export const downloadModel = (id: string) => invoke<void>("download_model", { id });

export const onModelDownloadProgress = (
  cb: (p: DownloadProgress) => void,
): Promise<UnlistenFn> => on("model-download-progress", cb);

export const onModelDownloadDone = (cb: (id: string) => void): Promise<UnlistenFn> =>
  on("model-download-done", (p) => cb(p.id));

export const onModelDownloadError = (
  cb: (id: string, message: string) => void,
): Promise<UnlistenFn> => on("model-download-error", (p) => cb(p.id, p.message));

/** Lanza la descarga y resuelve cuando el backend emite done/error para ese id. */
export async function downloadModelAndWait(id: string): Promise<void> {
  let unDone: UnlistenFn | undefined;
  let unErr: UnlistenFn | undefined;
  const cleanup = () => {
    unDone?.();
    unErr?.();
    unDone = undefined;
    unErr = undefined;
  };

  await new Promise<void>((resolve, reject) => {
    void (async () => {
      unDone = await onModelDownloadDone((doneId) => {
        if (doneId === id) {
          cleanup();
          resolve();
        }
      });
      unErr = await onModelDownloadError((errId, message) => {
        if (errId === id) {
          cleanup();
          reject(new Error(message));
        }
      });
      try {
        await downloadModel(id);
      } catch (e) {
        cleanup();
        reject(e instanceof Error ? e : new Error(String(e)));
      }
    })();
  });
}
