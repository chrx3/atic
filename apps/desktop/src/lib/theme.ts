/** Tema de interfaz: claro, oscuro o según el sistema. */

export type UiTheme = "light" | "dark" | "system";

/** Clave compartida entre ventanas: el evento `storage` sincroniza las flotantes. */
export const THEME_STORAGE_KEY = "atic-theme";
const STORAGE_KEY = THEME_STORAGE_KEY;

export function normalizeTheme(value: string | null | undefined): UiTheme {
  if (value === "light" || value === "dark" || value === "system") return value;
  return "system";
}

export function systemPrefersDark(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(theme: UiTheme): "light" | "dark" {
  if (theme === "system") return systemPrefersDark() ? "dark" : "light";
  return theme;
}

export function applyTheme(theme: UiTheme): "light" | "dark" {
  const resolved = resolveTheme(theme);
  const root = document.documentElement;
  root.dataset.theme = resolved;
  root.style.colorScheme = resolved;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // ignore
  }
  return resolved;
}

export function readCachedTheme(): UiTheme {
  try {
    return normalizeTheme(localStorage.getItem(STORAGE_KEY));
  } catch {
    return "system";
  }
}

export function cycleTheme(theme: UiTheme): UiTheme {
  if (theme === "light") return "dark";
  if (theme === "dark") return "system";
  return "light";
}

export function themeLabel(theme: UiTheme): string {
  switch (theme) {
    case "light":
      return "Claro";
    case "dark":
      return "Oscuro";
    default:
      return "Sistema";
  }
}
