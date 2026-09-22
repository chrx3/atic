/**
 * Dónde se guarda el hogar elegido de la pill (ver `PillHome` en `edgeDock`).
 *
 * En el almacenamiento del overlay y no en la config: depende de los monitores
 * de ESTA máquina, y `pill_position` de la config ya lo usan otros flujos de
 * Rust (el clipboard en el cursor lo guarda y lo restaura). Si el
 * almacenamiento falla o trae basura, la pill usa el hogar por defecto.
 */
import { isPillHome, type PillHome } from "$surfaces/overlay/edgeDock";

const KEY = "atic.pill.home";

export function readPillHome(): PillHome | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    return isPillHome(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

export function writePillHome(home: PillHome): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(home));
  } catch {
    // Sin almacenamiento el hogar dura lo que dure la sesión.
  }
}

export function clearPillHome(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // Nada que borrar si no hay almacenamiento.
  }
}
