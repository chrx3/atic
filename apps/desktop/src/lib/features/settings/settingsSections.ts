/** Ids de sección del SettingsPanel (ventana principal). */
const SETTINGS_SECTION_IDS = [
  "general",
  "meetings",
  "dictation",
  "captures",
  "shortcuts",
  "pill",
  "launcher",
  "audio",
  "summary",
  "agents",
  "about",
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTION_IDS)[number];

/** Valida un id que llega de fuera (evento `open-settings`, deep-links). */
export function isSettingsSection(value: string): value is SettingsSectionId {
  return (SETTINGS_SECTION_IDS as readonly string[]).includes(value);
}
