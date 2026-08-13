import { describe, expect, it } from "vitest";
import { isBrowserChromeShortcut, type KeyMods } from "./desktopChrome";

function key(
  key: string,
  mods: { ctrl?: boolean; shift?: boolean; alt?: boolean; meta?: boolean } = {},
): KeyMods {
  return {
    key,
    ctrlKey: mods.ctrl ?? false,
    shiftKey: mods.shift ?? false,
    altKey: mods.alt ?? false,
    metaKey: mods.meta ?? false,
  };
}

describe("isBrowserChromeShortcut", () => {
  it("bloquea imprimir y buscar del navegador", () => {
    expect(isBrowserChromeShortcut(key("p", { ctrl: true }))).toBe(true);
    expect(isBrowserChromeShortcut(key("P", { ctrl: true, shift: true }))).toBe(
      true,
    );
    expect(isBrowserChromeShortcut(key("f", { ctrl: true }))).toBe(true);
    expect(isBrowserChromeShortcut(key("u", { ctrl: true }))).toBe(true);
    expect(isBrowserChromeShortcut(key("s", { ctrl: true }))).toBe(true);
  });

  it("bloquea zoom del WebView", () => {
    expect(isBrowserChromeShortcut(key("+", { ctrl: true }))).toBe(true);
    expect(isBrowserChromeShortcut(key("-", { ctrl: true }))).toBe(true);
    expect(isBrowserChromeShortcut(key("0", { ctrl: true }))).toBe(true);
  });

  it("deja pasar edición y la búsqueda in-app", () => {
    expect(isBrowserChromeShortcut(key("c", { ctrl: true }))).toBe(false);
    expect(isBrowserChromeShortcut(key("v", { ctrl: true }))).toBe(false);
    expect(isBrowserChromeShortcut(key("x", { ctrl: true }))).toBe(false);
    expect(isBrowserChromeShortcut(key("a", { ctrl: true }))).toBe(false);
    expect(isBrowserChromeShortcut(key("z", { ctrl: true }))).toBe(false);
    expect(isBrowserChromeShortcut(key("k", { ctrl: true }))).toBe(false);
  });

  it("deja pasar Ctrl+Alt de los labs", () => {
    expect(isBrowserChromeShortcut(key("p", { ctrl: true, alt: true }))).toBe(
      false,
    );
  });
});
