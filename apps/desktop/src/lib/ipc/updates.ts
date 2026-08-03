/** Actualizaciones desde GitHub Releases. */

import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type { Update as AppUpdate, DownloadEvent as UpdateDownloadEvent };

/** Comprueba si hay una actualización en GitHub Releases (`latest.json`). */
export const checkAppUpdate = (): Promise<Update | null> => check();

/** Descarga, instala y reinicia la app con la actualización dada. */
export async function installAppUpdateAndRelaunch(
  update: Update,
  onEvent?: (event: DownloadEvent) => void,
): Promise<void> {
  await update.downloadAndInstall(onEvent);
  await relaunch();
}
