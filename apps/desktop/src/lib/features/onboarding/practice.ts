/** Los tres gestos que hay que hacer con las manos en el primer uso. */

export const PRACTICE_SKIP_AFTER_MS = 12_000;

export type PracticeStepId = "wheel" | "dictation" | "clipboard";

export interface PracticeStep {
  id: PracticeStepId;
  /** Clave de config del atajo que hay que apretar. */
  shortcutKey: "pill_radial_shortcut" | "dictation_shortcut" | "clipboard_shortcut";
}

export const PRACTICE_STEPS: PracticeStep[] = [
  { id: "wheel", shortcutKey: "pill_radial_shortcut" },
  { id: "dictation", shortcutKey: "dictation_shortcut" },
  { id: "clipboard", shortcutKey: "clipboard_shortcut" },
];

export const SETUP_SHORTCUTS = [
  {
    id: "wheel" as const,
    key: "pill_radial_shortcut",
    fallback: "Alt+Z",
    conflict: "rueda de herramientas",
  },
  {
    id: "dictation" as const,
    key: "dictation_shortcut",
    fallback: "CmdOrCtrl+Shift+D",
    conflict: "dictado",
  },
  {
    id: "clipboard" as const,
    key: "clipboard_shortcut",
    fallback: "CmdOrCtrl+Shift+V",
    conflict: "clipboard",
  },
] as const;
