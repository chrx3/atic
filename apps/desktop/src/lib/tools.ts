/** Herramientas del shell de Atic (caja de utilidades local-first). */

export type ToolId =
  | "meetings"
  | "dictation"
  | "clipboard"
  | "snippets"
  | "captures"
  | "agents";

/** Secciones estables de la ventana principal. Las utilidades de texto
 * (dictado, portapapeles y textos guardados) comparten una sola biblioteca. */
export type AppSectionId =
  | "home"
  | "meetings"
  | "texts"
  | "captures"
  | "agents"
  | "settings";

export type AppSectionDef = {
  id: AppSectionId;
  label: string;
  short: string;
  blurb: string;
};

export const APP_SECTIONS: AppSectionDef[] = [
  {
    id: "home",
    label: "Inicio",
    short: "Centro de control",
    blurb: "Estado, acciones rápidas y acceso a todo Atic.",
  },
  {
    id: "meetings",
    label: "Reuniones",
    short: "Grabar y resumir",
    blurb: "Audio del PC, transcripción local y resúmenes editables.",
  },
  {
    id: "texts",
    label: "Textos",
    short: "Escribir y recuperar",
    blurb: "Textos guardados, notas, portapapeles y dictado en un solo lugar.",
  },
  {
    id: "captures",
    label: "Capturas",
    short: "Pantalla",
    blurb: "Recortes rápidos, historial y extracción de texto.",
  },
  {
    id: "agents",
    label: "Agentes",
    short: "Conversaciones",
    blurb: "Sesiones de agentes con contexto, permisos y archivos.",
  },
  {
    id: "settings",
    label: "Ajustes",
    short: "Preferencias",
    blurb: "Configura la interfaz, audio, modelos, atajos y privacidad.",
  },
];

export function sectionById(id: AppSectionId): AppSectionDef {
  return APP_SECTIONS.find((section) => section.id === id) ?? APP_SECTIONS[0];
}

export type ToolDef = {
  id: ToolId;
  label: string;
  short: string;
  /** Una línea: para qué sirve esta herramienta. */
  blurb: string;
  /** false = UI lista; true = aún en construcción. */
  comingSoon?: boolean;
};

export const TOOLS: ToolDef[] = [
  {
    id: "meetings",
    label: "Reuniones",
    short: "Grabar y resumir",
    blurb: "Audio del PC, transcripción local y resúmenes editables.",
  },
  {
    id: "dictation",
    label: "Dictado",
    short: "Voz a texto",
    blurb: "Habla y pega texto en cualquier app con un atajo.",
  },
  {
    id: "clipboard",
    label: "Clipboard",
    short: "Historial",
    blurb: "Historial local de texto e imágenes; atajo para pegar desde la pill.",
  },
  {
    id: "snippets",
    // "Fragmentos" describía la forma, no el uso, y no decía en qué se
    // diferencia de Clipboard. La distinción real es el origen: el historial se
    // llena solo con lo que copiás, esto lo guardás vos a propósito.
    label: "Textos",
    short: "Guardados a mano",
    blurb: "Los textos que escribís siempre, listos para pegar. Más un bloc para notas sueltas.",
  },
  {
    id: "agents",
    label: "Agentes",
    short: "Consola con interfaz",
    blurb: "Conversá con agentes de consola desde una interfaz, sin perder sus herramientas.",
  },
  {
    id: "captures",
    label: "Capturas",
    short: "Pantalla",
    blurb: "Recortes rápidos al portapapeles y al shelf flotante.",
  },
];

export function toolById(id: ToolId): ToolDef {
  return TOOLS.find((tool) => tool.id === id) ?? TOOLS[0];
}
