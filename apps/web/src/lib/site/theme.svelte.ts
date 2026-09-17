/**
 * El tema del sitio.
 *
 * Son las mismas paletas de la app (`styles/palettes/atic/`), aplicadas con
 * `data-theme` en `<html>` — el mecanismo que ya usa el escritorio. El sitio
 * no inventa temas: elige entre los que la app tiene.
 */
import { loadJSON, saveJSON } from "$lib/demo/state.svelte";

export type ThemeId =
  | "dark"
  | "light"
  | "graphite"
  | "midnight"
  | "sepia"
  | "mist"
  | "claude"
  | "claude-dark";

export const THEMES: { id: ThemeId; label: string; base: "dark" | "light" }[] = [
  { id: "dark", label: "Papel y tinta", base: "dark" },
  { id: "light", label: "Papel y tinta claro", base: "light" },
  { id: "graphite", label: "Grafito", base: "dark" },
  { id: "midnight", label: "Nocturno", base: "dark" },
  { id: "sepia", label: "Sepia", base: "light" },
  { id: "mist", label: "Niebla", base: "light" },
  { id: "claude", label: "Claude", base: "light" },
  { id: "claude-dark", label: "Claude oscuro", base: "dark" },
];

const KEY = "atic-web-theme";

class ThemeStore {
  current = $state<ThemeId>("dark");

  init() {
    const stored = loadJSON<ThemeId>(KEY, "dark");
    this.current = THEMES.some((theme) => theme.id === stored) ? stored : "dark";
    this.apply();
  }

  set(id: ThemeId) {
    this.current = id;
    saveJSON(KEY, id);
    this.apply();
  }

  private apply() {
    if (typeof document === "undefined") return;
    const theme = THEMES.find((item) => item.id === this.current) ?? THEMES[0];
    document.documentElement.dataset.theme = theme.id;
    document.documentElement.dataset.themeBase = theme.base;
  }
}

export const theme = new ThemeStore();
