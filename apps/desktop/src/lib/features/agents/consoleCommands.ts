/**
 * Los comandos guardados de la consola, fuera de `ConsolePanel`.
 *
 * Vivían dentro del menú «+» del rail. Cuando ese menú se mudó a la hoja
 * «Nueva consola» (que vive en el lanzador), la lista quedó en dos superficies:
 * el formato y las reglas —cuántos, sin repetir— tienen que ser uno solo.
 */
export const SAVED_CMDS_KEY = "atic.agents.savedCommands";
export const SAVED_CMDS_MAX = 8;

export function loadSavedCommands(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(SAVED_CMDS_KEY) ?? "[]") as unknown;
    return Array.isArray(raw)
      ? raw.filter((cmd): cmd is string => typeof cmd === "string")
      : [];
  } catch {
    return [];
  }
}

export function saveSavedCommands(list: string[]): void {
  try {
    localStorage.setItem(SAVED_CMDS_KEY, JSON.stringify(list));
  } catch {
    /* la lista sigue en memoria aunque el storage esté bloqueado */
  }
}

/** Recordar un comando: al frente, sin repetir y con tope. */
export function rememberCommand(list: string[], cmd: string): string[] {
  const trimmed = cmd.trim();
  if (!trimmed) return list;
  return [trimmed, ...list.filter((c) => c !== trimmed)].slice(0, SAVED_CMDS_MAX);
}
