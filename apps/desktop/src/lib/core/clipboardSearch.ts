/** Normaliza para búsqueda: minúsculas y sin acentos. */
export function normalizeSearchText(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/**
 * Coincidencia ligera: substring o tokens (todas las palabras presentes).
 *
 * SIN “caracteres en orden”: en textos largos casi cualquier palabra es una
 * subsecuencia salteada de un párrafo —buscar “mantenemos” devolvía ítems que
 * ni la contenían—. Buscar es substring (normalizado) o todas las palabras, y
 * nada más. Query vacía = match.
 */
export function fuzzyMatch(haystack: string, query: string): boolean {
  const q = normalizeSearchText(query).trim();
  if (!q) return true;

  const text = normalizeSearchText(haystack);
  if (!text) return false;
  if (text.includes(q)) return true;

  const tokens = q.split(/\s+/).filter(Boolean);
  return tokens.length > 1 && tokens.every((token) => text.includes(token));
}

export function clipboardItemMatches(
  item: { preview?: string | null; text?: string | null },
  query: string,
): boolean {
  const haystack = [item.preview, item.text].filter(Boolean).join("\n");
  return fuzzyMatch(haystack, query);
}
