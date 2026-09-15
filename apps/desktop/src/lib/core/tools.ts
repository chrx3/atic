/** Herramientas del shell de Atic (caja de utilidades local-first). */

export type ToolId =
  | "meetings"
  | "dictation"
  | "clipboard"
  | "snippets"
  | "captures"
  | "board"
  | "color"
  | "agents"
  | "launcher";

/**
 * La consola de agentes: chat con Claude Code / Codex / ACP más consolas
 * embebidas, ofrecida en la rueda, ajustes, atajo y launcher.
 *
 * Estuvo cerrada mientras la consola solo aguantaba dos sesiones fijas (una
 * local y una ssh, y abrir otra mataba a la anterior). Se reabre ahora que las
 * consolas son N pestañas independientes.
 */
export const AGENTS_ENABLED = true;

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
  /**
   * Solo vive en su atajo: no gana gajo en la rueda.
   *
   * Va acá y no en una lista paralela para que sumar una tool de este tipo sea
   * marcar su campo, no acordarse de un `Set` en otro archivo. El test repetía
   * la lista a mano y por eso no podía fallar cuando el código cambiaba.
   */
  shortcutOnly?: boolean;
  /**
   * Tiene cuerpo a pantalla completa en la ventana principal.
   *
   * Pizarra y Apps son una acción, no una vista: no tienen pestaña en el
   * workspace. Antes esa lista vivía dentro de `ToolWorkspace`.
   */
  body?: boolean;
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
    body: true,
  },
  {
    id: "dictation",
    label: "Dictado",
    short: "Voz a texto",
    blurb: "Habla y pega texto en cualquier app con un atajo.",
    actionLabel: "Dictar",
    shortcutOnly: true,
    body: true,
  },
  {
    id: "clipboard",
    label: "Clipboard",
    short: "Historial",
    blurb: "Historial local de texto e imágenes; atajo para pegar desde la pill.",
    actionLabel: "Ver historial",
    body: true,
  },
  {
    id: "snippets",
    // "Fragmentos" describía la forma, no el uso, y no decía en qué se
    // diferencia de Clipboard. La distinción real es el origen: el historial se
    // llena solo con lo que copias, esto lo guardas tú a propósito.
    label: "Textos",
    short: "Guardados a mano",
    blurb:
      "Los textos que escribes siempre, listos para pegar. Más un bloc para notas sueltas.",
    actionLabel: "Ver textos",
    body: true,
  },
  {
    id: "agents",
    label: "Agentes",
    short: "Consola con interfaz",
    blurb:
      "Conversa con agentes de consola desde una interfaz, sin perder sus herramientas.",
    actionLabel: "Abrir consola",
    body: true,
  },
  {
    id: "captures",
    label: "Capturas",
    short: "Pantalla",
    blurb: "Recortes rápidos al portapapeles y al shelf flotante.",
    actionLabel: "Tomar captura",
    body: true,
  },
  {
    id: "board",
    label: "Pizarra",
    short: "Marcar la pantalla",
    blurb: "Congela la pantalla y la marcas con flechas y círculos, ahí donde está.",
    actionLabel: "Dibujar",
  },
  {
    id: "color",
    label: "Color",
    short: "Cuentagotas",
    blurb:
      "Elige un color de la pantalla, píxel a píxel, o desde la rosa cromática. Se copia al portapapeles.",
    actionLabel: "Elegir color",
  },
  {
    id: "launcher",
    label: "Apps",
    short: "Programas del sistema",
    blurb:
      "Abre apps y acciones del PC. Mismo launcher que Ctrl+Space (tipo Spotlight).",
    actionLabel: "Buscar apps",
    shortcutOnly: true,
  },
];

export const TOOLS: ToolDef[] = AGENTS_ENABLED
  ? ALL_TOOLS
  : ALL_TOOLS.filter((tool) => tool.id !== "agents");

/**
 * La rueda de la pill: mismas tools menos las que son puro atajo.
 *
 * Un gajo solo se gana el sitio si apuntarle es mejor que la tecla. El
 * launcher vive en Ctrl+Space, y el dictado en su atajo —que además es el
 * único camino que puede hacer push-to-talk de verdad: el clic pasa por el
 * vuelo al slot, así que cuando el mic abre ya soltaste el botón—. Las dos
 * siguen enteras en la ventana principal y en sus atajos, con su `body`.
 */
export const WHEEL_TOOLS: ToolDef[] = TOOLS.filter((tool) => !tool.shortcutOnly);

/**
 * Las que tienen cuerpo propio en la ventana principal (`ToolWorkspace`).
 *
 * Se deriva del registro para que el set que estaba dentro del componente no
 * se quede viejo al sumar una tool con vista.
 */
export const BODIED_TOOLS: ToolDef[] = TOOLS.filter((tool) => tool.body);

export function toolById(id: ToolId): ToolDef {
  return TOOLS.find((tool) => tool.id === id) ?? TOOLS[0];
}
