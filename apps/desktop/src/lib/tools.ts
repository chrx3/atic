/** Herramientas del shell de Atic (caja de utilidades local-first). */

export type ToolId = "meetings" | "dictation" | "clipboard" | "snippets" | "captures";

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
    label: "Fragmentos",
    short: "Texto rápido",
    blurb: "Plantillas reutilizables y bloc de notas; atajo para pegar desde la pill.",
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
