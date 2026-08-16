import { describe, expect, it } from "vitest";
import commandsRs from "../../src-tauri/src/commands.rs?raw";
import overlayRs from "../../src-tauri/src/overlay.rs?raw";
import {
  cycleTheme,
  normalizeTheme,
  resolveTheme,
  themeLabel,
} from "./theme";

describe("normalizeTheme", () => {
  it("acepta los tres valores persistidos", () => {
    expect(normalizeTheme("light")).toBe("light");
    expect(normalizeTheme("dark")).toBe("dark");
    expect(normalizeTheme("system")).toBe("system");
  });

  it("cae a sistema si el valor no es un tema", () => {
    expect(normalizeTheme(null)).toBe("system");
    expect(normalizeTheme(undefined)).toBe("system");
    expect(normalizeTheme("")).toBe("system");
    expect(normalizeTheme("claro")).toBe("system");
  });
});

describe("resolveTheme", () => {
  it("respeta claro y oscuro sin mirar el SO", () => {
    expect(resolveTheme("light")).toBe("light");
    expect(resolveTheme("dark")).toBe("dark");
  });

  it("con sistema, sin window, asume claro", () => {
    expect(resolveTheme("system")).toBe("light");
  });
});

describe("cycleTheme", () => {
  it("recorre claro → oscuro → sistema", () => {
    expect(cycleTheme("light")).toBe("dark");
    expect(cycleTheme("dark")).toBe("system");
    expect(cycleTheme("system")).toBe("light");
  });
});

describe("themeLabel", () => {
  it("nombra cada modo", () => {
    expect(themeLabel("light")).toBe("Claro");
    expect(themeLabel("dark")).toBe("Oscuro");
    expect(themeLabel("system")).toBe("Sistema");
  });
});

describe("tema entre webviews", () => {
  it("el overlay aísla el perfil de WebView2", () => {
    expect(overlayRs).toMatch(/data_directory\(dir\.join\("overlay-webview"\)\)/);
  });

  it("persistir config avisa el tema a todos los webviews", () => {
    expect(commandsRs).toMatch(/emit\("ui-theme"/);
  });
});
