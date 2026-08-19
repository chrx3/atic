/** Actualizaciones desde GitHub Releases. */

import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type { Update as AppUpdate, DownloadEvent as UpdateDownloadEvent };

export const GITHUB_REPO_URL = "https://github.com/chrx3/atic";
export const GITHUB_RELEASES_URL = "https://github.com/chrx3/atic/releases";

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

export function friendlyUpdateError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  if (/404|not found|failed to fetch|error sending request|Could not fetch/i.test(raw)) {
    return "No hay un paquete de actualización en GitHub. Subí latest.json junto al .exe del release.";
  }
  return raw;
}
