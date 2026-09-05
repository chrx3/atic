/**
 * Navegación del picker de carpetas: filtro, salto por letra y favoritos.
 *
 * El listado lo da Rust; acá solo se ordena lo ya cargado y se recuerda
 * qué rutas el usuario quiere fijas.
 */

export type FolderFav = { name: string; path: string };

export const FOLDER_FAVS_KEY = "atic.agents.folderFavorites";
export const TYPEAHEAD_MS = 700;
const MAX_FAVS = 24;

/** Quita tildes para que "d" encuentre "Documentos" y "á" cuente como "a". */
export function foldName(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

export function normalizePath(path: string): string {
  return path
    .replace(/^\\\\\?\\/, "")
    .replace(/\\/g, "/")
    .replace(/\/+$/, "")
    .toLowerCase();
}

export function pathsEqual(a: string, b: string): boolean {
  return normalizePath(a) === normalizePath(b);
}

export function leafName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

export function filterEntries<T extends { name: string }>(
  entries: readonly T[],
  query: string,
): T[] {
  const q = foldName(query.trim());
  if (!q) return [...entries];
  return entries.filter((entry) => foldName(entry.name).includes(q));
}

/**
 * Salto tipo Explorer: una letra cicla las que empiezan con ella;
 * varias letras seguidas buscan el prefijo.
 */
export function jumpIndex(
  names: readonly string[],
  buffer: string,
  activeIndex: number,
): number {
  const q = foldName(buffer);
  if (!q || names.length === 0) return -1;
  const cycling = [...q].every((ch) => ch === q[0]);
  const needle = cycling ? q[0] : q;
  const from =
    activeIndex < 0 ? 0 : cycling ? (activeIndex + 1) % names.length : activeIndex;

  for (let i = 0; i < names.length; i++) {
    const idx = (from + i) % names.length;
    if (foldName(names[idx]).startsWith(needle)) return idx;
  }
  return -1;
}

export function isJumpKey(event: KeyboardEvent): boolean {
  if (event.ctrlKey || event.metaKey || event.altKey) return false;
  if (event.key.length !== 1) return false;
  return /\p{L}|\p{N}/u.test(event.key);
}

export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return target.isContentEditable;
}

export function isFav(favs: readonly FolderFav[], path: string): boolean {
  return favs.some((fav) => pathsEqual(fav.path, path));
}

export function toggleFav(
  favs: readonly FolderFav[],
  entry: FolderFav,
): FolderFav[] {
  if (isFav(favs, entry.path)) {
    return favs.filter((fav) => !pathsEqual(fav.path, entry.path));
  }
  return [{ name: entry.name, path: entry.path }, ...favs].slice(0, MAX_FAVS);
}

export function readFavs(): FolderFav[] {
  try {
    const raw = localStorage.getItem(FOLDER_FAVS_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const out: FolderFav[] = [];
    for (const item of parsed) {
      if (!item || typeof item !== "object") continue;
      const rec = item as { name?: unknown; path?: unknown };
      if (typeof rec.path !== "string" || !rec.path.trim()) continue;
      const path = rec.path.trim();
      const name =
        typeof rec.name === "string" && rec.name.trim()
          ? rec.name.trim()
          : leafName(path);
      if (out.some((fav) => pathsEqual(fav.path, path))) continue;
      out.push({ name, path });
      if (out.length >= MAX_FAVS) break;
    }
    return out;
  } catch {
    return [];
  }
}

export function writeFavs(favs: readonly FolderFav[]): void {
  try {
    localStorage.setItem(FOLDER_FAVS_KEY, JSON.stringify(favs));
  } catch {
    /* El picker sigue sin favoritos persistidos. */
  }
}
