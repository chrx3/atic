/** Los tres gestos que hay que hacer con las manos en el primer uso. */

export const PRACTICE_SKIP_AFTER_MS = 12_000;

export type PracticeStepId = "wheel" | "dictation" | "clipboard";

export interface PracticeStep {
  id: PracticeStepId;
  title: string;
  /** Clave de config del atajo que hay que apretar. */
  shortcutKey: "pill_radial_shortcut" | "dictation_shortcut" | "clipboard_shortcut";
  body: string;
}

export const PRACTICE_STEPS: PracticeStep[] = [
  {
    id: "wheel",
    title: "Abrí la rueda",
    shortcutKey: "pill_radial_shortcut",
    body: "Mantené el atajo. La rueda aparece en el cursor. Soltá cuando la hayas visto.",
  },
  {
    id: "dictation",
    title: "Dictá algo",
    shortcutKey: "dictation_shortcut",
    body: "Apretá el atajo, hablá una frase y soltalo. El texto tiene que aparecer acá.",
  },
  {
    id: "clipboard",
    title: "Abrí el historial",
    shortcutKey: "clipboard_shortcut",
    body: "Copiá cualquier texto y apretá el atajo. Tiene que abrirse junto a la pill.",
  },
];

export const SETUP_SHORTCUTS = [
  {
    key: "pill_radial_shortcut",
    label: "Rueda de herramientas",
    hint: "Mantenelo y soltá sobre la herramienta.",
    fallback: "Alt+Z",
    conflict: "rueda de herramientas",
  },
  {
    key: "dictation_shortcut",
    label: "Dictar",
    hint: "Hablás y el texto se pega donde estabas.",
    fallback: "CmdOrCtrl+Shift+D",
    conflict: "dictado",
  },
  {
    key: "clipboard_shortcut",
    label: "Historial del portapapeles",
    hint: "Lo que copiaste, para volver a pegarlo.",
    fallback: "CmdOrCtrl+Shift+V",
    conflict: "clipboard",
  },
] as const;
