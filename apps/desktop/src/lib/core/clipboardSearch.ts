/** Normaliza para búsqueda: minúsculas y sin acentos. */
export function normalizeSearchText(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

/**
 * Coincidencia ligera: substring, tokens (todos presentes) o caracteres en orden.
 * Query vacía = match.
 */
export function fuzzyMatch(haystack: string, query: string): boolean {
  const q = normalizeSearchText(query).trim();
  if (!q) return true;

  const text = normalizeSearchText(haystack);
  if (!text) return false;
  if (text.includes(q)) return true;

  const tokens = q.split(/\s+/).filter(Boolean);
  if (tokens.length > 1 && tokens.every((token) => text.includes(token))) {
    return true;
  }

  let qi = 0;
  for (let i = 0; i < text.length && qi < q.length; i++) {
    if (text[i] === q[qi]) qi++;
  }
  return qi === q.length;
}

export function clipboardItemMatches(
  item: { preview?: string | null; text?: string | null },
  query: string,
): boolean {
  const haystack = [item.preview, item.text].filter(Boolean).join("\n");
  return fuzzyMatch(haystack, query);
}
