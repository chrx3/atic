/** Herramientas del shell de Atic (caja de utilidades local-first). */

export type ToolId =
  | "meetings"
  | "dictation"
  | "clipboard"
  | "snippets"
  | "captures"
  | "board"
  | "agents"
  | "launcher";

/**
 * La consola de agentes no se ofrece en la UI (rueda, ajustes, atajo, launcher).
 * El código sigue en el repo; volver a `true` cuando se reabra la feature.
 */
export const AGENTS_ENABLED = false;

/**
 * Semáforo de la pill para agentes que corren en su TUI (Claude Code, etc.).
 * Independiente de `AGENTS_ENABLED`: el pager no abre el chat de Atic.
 */
export const AGENT_PAGER_ENABLED = true;

export type ToolDef = {
  id: ToolId;
  label: string;
  short: string;
  /** Una línea: para qué sirve esta herramienta. */
  blurb: string;
  /**
   * Verbo de la acción primaria en el picker (fallback estático).
   * Los labels dinámicos (Grabar/Parar) viven en `toolActions`.
   */
  actionLabel: string;
  /** false = UI lista; true = aún en construcción. */
  comingSoon?: boolean;
};

const ALL_TOOLS: ToolDef[] = [
  {
    id: "meetings",
    label: "Reuniones",
    short: "Grabar y resumir",
    blurb: "Audio del PC, transcripción local y resúmenes editables.",
    actionLabel: "Grabar",
  },
  {
    id: "dictation",
    label: "Dictado",
    short: "Voz a texto",
    blurb: "Habla y pega texto en cualquier app con un atajo.",
    actionLabel: "Dictar",
  },
  {
    id: "clipboard",
    label: "Clipboard",
    short: "Historial",
    blurb: "Historial local de texto e imágenes; atajo para pegar desde la pill.",
    actionLabel: "Ver historial",
  },
  {
    id: "snippets",
    // "Fragmentos" describía la forma, no el uso, y no decía en qué se
    // diferencia de Clipboard. La distinción real es el origen: el historial se
    // llena solo con lo que copiás, esto lo guardás vos a propósito.
    label: "Textos",
    short: "Guardados a mano",
    blurb:
      "Los textos que escribís siempre, listos para pegar. Más un bloc para notas sueltas.",
    actionLabel: "Ver textos",
  },
  {
    id: "agents",
    label: "Agentes",
    short: "Consola con interfaz",
    blurb:
      "Conversá con agentes de consola desde una interfaz, sin perder sus herramientas.",
    actionLabel: "Abrir consola",
  },
  {
    id: "captures",
    label: "Capturas",
    short: "Pantalla",
    blurb: "Recortes rápidos al portapapeles y al shelf flotante.",
    actionLabel: "Tomar captura",
  },
  {
    id: "board",
    label: "Pizarra",
    short: "Marcar la pantalla",
    blurb: "Congela la pantalla y la marcás con flechas y círculos, ahí donde está.",
    actionLabel: "Dibujar",
  },
  {
    id: "launcher",
    label: "Apps",
    short: "Programas del sistema",
    blurb:
      "Abrí apps y acciones del PC. Mismo launcher que Ctrl+Space (tipo Spotlight).",
    actionLabel: "Buscar apps",
  },
];

export const TOOLS: ToolDef[] = AGENTS_ENABLED
  ? ALL_TOOLS
  : ALL_TOOLS.filter((tool) => tool.id !== "agents");

/**
 * La rueda de la pill: mismas tools menos el launcher.
 * Spotlight vive en Ctrl+Space y en la ventana principal, no en el anillo.
 */
export const WHEEL_TOOLS: ToolDef[] = TOOLS.filter((tool) => tool.id !== "launcher");

export function toolById(id: ToolId): ToolDef {
  return TOOLS.find((tool) => tool.id === id) ?? TOOLS[0];
}
