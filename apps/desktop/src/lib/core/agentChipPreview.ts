/**
 * Recorte visual del chip de la pill: primera línea, 28 caracteres.
 *
 * Una sola definición: la usan el chat de Atic (`readyLabel`) y el pager de TUI.
 */
export const CHIP_PREVIEW_MAX = 28;

export function clipChipPreview(text: string | null | undefined): string {
  const trimmed = text?.trim();
  if (!trimmed) return "Listo";
  const line = trimmed.split("\n")[0]?.trim() || "Listo";
  return line.length > CHIP_PREVIEW_MAX
    ? `${line.slice(0, CHIP_PREVIEW_MAX - 1)}…`
    : line;
}
