/**
 * Recorte visual del chip de la pill: primera línea, 28 caracteres.
 *
 * Una sola definición: la usan el chat de Atic (`readyLabel`) y el pager de TUI.
 * El chip no renderiza markdown: quita marcas sueltas para que no se vean `**`.
 */
export const CHIP_PREVIEW_MAX = 28;

function stripChipMarkup(text: string): string {
  return text
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1")
    .replace(/^#+\s+/gm, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function clipChipPreview(text: string | null | undefined): string {
  const trimmed = text?.trim();
  if (!trimmed) return "Listo";
  const line = stripChipMarkup(trimmed.split("\n")[0] ?? "") || "Listo";
  return line.length > CHIP_PREVIEW_MAX
    ? `${line.slice(0, CHIP_PREVIEW_MAX - 1)}…`
    : line;
}
