/**
 * Qué herramientas tienen vistazo (ver `toolPeek.svelte.ts`).
 *
 * Aparte del gesto para poder probarlo sin runas: decide qué botones pierden
 * el tooltip y abren un vistazo, y un error acá deja botones mudos.
 */

/** Herramientas con vistazo propio. El resto se queda con el tooltip. */
export const PEEK_TOOLS = [
  "agents",
  "system",
  "clipboard",
  "captures",
  "color",
  "snippets",
] as const;
export type PeekTool = (typeof PEEK_TOOLS)[number];

export function isPeekTool(id: string | null | undefined): id is PeekTool {
  return PEEK_TOOLS.includes(id as PeekTool);
}

/** Qué vistazo abre un ancla, y qué dice si no hay nada que mostrar. */
export type PeekSpec = { tool: PeekTool; fallback: string };

/** El vistazo de `id`, o null si esa herramienta no tiene. */
export function peekFor(id: string, fallback: string): PeekSpec | null {
  return isPeekTool(id) ? { tool: id, fallback } : null;
}
