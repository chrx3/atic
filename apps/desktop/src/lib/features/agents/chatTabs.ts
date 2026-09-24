/**
 * Qué fichas de chat tenía abiertas cada lanzador, para retomarlas.
 *
 * La sesión de un chat vive en Rust y sobrevive a recargar el overlay; la
 * ficha no, y sin ficha la sesión queda viva y sin nadie que la muestre. Cada
 * lanzador (isla, float, ventana) guarda las suyas bajo su propia clave: así
 * dos vistas no se roban las fichas al volver.
 */

export type ChatTabRecord = {
  session: string;
  label: string;
  command: string | null;
};

const PREFIX = "atic.agents.chatTabs.";

export function chatTabsKey(host: string): string {
  return `${PREFIX}${host}`;
}

/** Lo guardado, o nada si falta o está roto: retomar es un extra, no un requisito. */
export function parseChatTabs(raw: string | null): ChatTabRecord[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) return [];
    return value.flatMap((item): ChatTabRecord[] => {
      if (!item || typeof item !== "object") return [];
      const { session, label, command } = item as Record<string, unknown>;
      if (typeof session !== "string" || !session) return [];
      return [
        {
          session,
          label: typeof label === "string" ? label : "",
          command: typeof command === "string" ? command : null,
        },
      ];
    });
  } catch {
    return [];
  }
}

export function loadChatTabs(key: string): ChatTabRecord[] {
  try {
    return parseChatTabs(localStorage.getItem(key));
  } catch {
    return [];
  }
}

export function saveChatTabs(key: string, tabs: ChatTabRecord[]): void {
  try {
    if (tabs.length === 0) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(tabs));
  } catch {
    /* sin storage no se retoma, pero el chat sigue funcionando */
  }
}
