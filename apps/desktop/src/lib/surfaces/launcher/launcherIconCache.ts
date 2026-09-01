/** Cache en memoria de data URLs de iconos del launcher. */

import { launcherIcon } from "$ipc/search";

const cache = new Map<string, Promise<string | null>>();

export function loadLauncherAppIcon(id: string): Promise<string | null> {
  // "app:" = .lnk del menú Inicio; "uwp:" = app de Store por AppUserModelID.
  if (!id.startsWith("app:") && !id.startsWith("uwp:")) return Promise.resolve(null);
  let pending = cache.get(id);
  if (!pending) {
    pending = launcherIcon(id).catch(() => null);
    // No cachear fallos: un icono que no cargó (destino de red, placeholder
    // de OneDrive) puede resolver en un intento posterior.
    void pending.then((url) => {
      if (url === null && cache.get(id) === pending) cache.delete(id);
    });
    cache.set(id, pending);
  }
  return pending;
}
