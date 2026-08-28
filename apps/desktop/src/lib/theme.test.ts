import { describe, expect, it } from "vitest";
import commandsRs from "../../src-tauri/src/commands.rs?raw";
import configRs from "../../../../crates/core/src/config.rs?raw";
import overlayRs from "../../src-tauri/src/overlay.rs?raw";
import {
  cycleTheme,
  normalizeTheme,
  resolveTheme,
  themeBase,
  themeLabel,
  UI_THEMES,
} from "./theme";

describe("normalizeTheme", () => {
  it("acepta todos los temas persistibles", () => {
    for (const theme of UI_THEMES) expect(normalizeTheme(theme)).toBe(theme);
  });

  it("cae a sistema si el valor no es un tema", () => {
    expect(normalizeTheme(null)).toBe("system");
    expect(normalizeTheme(undefined)).toBe("system");
    expect(normalizeTheme("")).toBe("system");
    expect(normalizeTheme("claro")).toBe("system");
  });
});

describe("resolveTheme", () => {
  it("respeta el tema pedido sin mirar el SO", () => {
    expect(resolveTheme("light")).toBe("light");
    expect(resolveTheme("dark")).toBe("dark");
    expect(resolveTheme("sepia")).toBe("sepia");
    expect(resolveTheme("midnight")).toBe("midnight");
  });

  it("con sistema, sin window, asume claro", () => {
    expect(resolveTheme("system")).toBe("light");
  });
});

describe("themeBase", () => {
  it("clasifica cada tema de un lado o del otro", () => {
    expect(themeBase("light")).toBe("light");
    expect(themeBase("sepia")).toBe("light");
    expect(themeBase("mist")).toBe("light");
    expect(themeBase("graphite")).toBe("dark");
    expect(themeBase("midnight")).toBe("dark");
    expect(themeBase("dark")).toBe("dark");
    expect(themeBase("claude")).toBe("light");
    expect(themeBase("claude-dark")).toBe("dark");
  });

  it("el personalizado sigue a sus perillas, no a una tabla", () => {
    // Sin aplicar nada todavía, vale el lado del base por defecto.
    expect(themeBase("custom")).toBe("dark");
  });

  it("un valor desconocido cae en sistema", () => {
    expect(themeBase("naranja")).toBe("light");
  });
});

describe("cycleTheme", () => {
  it("recorre claro → oscuro → sistema", () => {
    expect(cycleTheme("light")).toBe("dark");
    expect(cycleTheme("dark")).toBe("system");
    expect(cycleTheme("system")).toBe("light");
  });

  it("desde un tema intermedio vuelve al atajo", () => {
    expect(cycleTheme("sepia")).toBe("light");
    expect(cycleTheme("graphite")).toBe("light");
  });
});

describe("themeLabel", () => {
  it("nombra cada modo", () => {
    expect(themeLabel("light")).toBe("Claro");
    expect(themeLabel("dark")).toBe("Oscuro");
    expect(themeLabel("system")).toBe("Sistema");
    expect(themeLabel("sepia")).toBe("Sepia");
    expect(themeLabel("mist")).toBe("Niebla");
    expect(themeLabel("graphite")).toBe("Grafito");
    expect(themeLabel("midnight")).toBe("Nocturno");
    expect(themeLabel("claude")).toBe("Claude");
    expect(themeLabel("claude-dark")).toBe("Claude oscuro");
    expect(themeLabel("custom")).toBe("Personalizado");
  });
});

describe("tema entre webviews", () => {
  it("el overlay aísla el perfil de WebView2", () => {
    expect(overlayRs).toMatch(/data_directory\(dir\.join\("overlay-webview"\)\)/);
  });

  it("persistir config avisa el tema a todos los webviews", () => {
    expect(commandsRs).toMatch(/emit\("ui-theme"/);
  });

  /**
   * Rust valida `ui_theme` contra su propia lista: si se desincronizan, el tema
   * nuevo se elige, se aplica, y vuelve a `system` al guardar.
   */
  it("Rust conoce los mismos temas", () => {
    const list = configRs.match(/const UI_THEMES: \[&str; \d+\] = \[(.+?)\];/s)?.[1] ?? "";
    const themes = [...list.matchAll(/"([a-z-]+)"/g)].map((match) => match[1]);
    const expected = UI_THEMES.filter((theme) => theme !== "system");
    expect(themes.sort()).toEqual([...expected].sort());
  });
});
