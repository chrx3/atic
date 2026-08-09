/** Cache en memoria de data URLs de iconos del launcher. */

import { launcherIcon } from "$ipc/search";

const cache = new Map<string, Promise<string | null>>();

export function loadLauncherAppIcon(id: string): Promise<string | null> {
  if (!id.startsWith("app:")) return Promise.resolve(null);
  let pending = cache.get(id);
  if (!pending) {
    pending = launcherIcon(id).catch(() => null);
    cache.set(id, pending);
  }
  return pending;
}
