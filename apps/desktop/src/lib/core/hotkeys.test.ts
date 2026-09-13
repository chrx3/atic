import { describe, expect, it } from "vitest";
import {
  detectShortcutOs,
  formatShortcutText,
  keyFromCode,
  setShortcutOs,
  shortcutFromEvent,
  shortcutParts,
  type ShortcutKeyEvent,
} from "./hotkeys";

function event(
  partial: Partial<ShortcutKeyEvent> & { code: string },
): ShortcutKeyEvent {
  return {
    key: "",
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    ...partial,
  };
}

describe("keyFromCode", () => {
  it("saca letras y dígitos del código físico", () => {
    expect(keyFromCode("KeyQ")).toBe("Q");
    expect(keyFromCode("Digit2")).toBe("2");
    expect(keyFromCode("Space")).toBe("Space");
    expect(keyFromCode("ArrowUp")).toBe("ArrowUp");
  });
});

describe("shortcutFromEvent", () => {
  it("con Option usa la tecla física y no el carácter del layout", () => {
    // En Mac, Option+Q manda `key: "œ"`, que el parser no reconoce.
    const optionQ = event({ key: "œ", code: "KeyQ", altKey: true });
    expect(shortcutFromEvent(optionQ, "macos")).toBe("Alt+Q");
  });

  it("en Mac distingue Control de Command", () => {
    expect(
      shortcutFromEvent(event({ key: "q", code: "KeyQ", metaKey: true }), "macos"),
    ).toBe("Command+Q");
    expect(
      shortcutFromEvent(event({ key: "q", code: "KeyQ", ctrlKey: true }), "macos"),
    ).toBe("Control+Q");
  });

  it("fuera de Mac usa el primario CmdOrCtrl", () => {
    expect(
      shortcutFromEvent(event({ key: "q", code: "KeyQ", ctrlKey: true }), "windows"),
    ).toBe("CmdOrCtrl+Q");
  });

  it("en Linux Super no se confunde con Control", () => {
    const superQ = event({ key: "q", code: "KeyQ", metaKey: true });
    expect(shortcutFromEvent(superQ, "other")).toBe("Super+Q");
  });

  it("solo un modificador no arma atajo", () => {
    const alt = event({ key: "Alt", code: "AltLeft", altKey: true });
    expect(shortcutFromEvent(alt, "macos")).toBeNull();
  });

  it("una letra sin modificador tampoco", () => {
    expect(shortcutFromEvent(event({ key: "q", code: "KeyQ" }), "macos")).toBeNull();
  });

  it("las F solas sí", () => {
    expect(shortcutFromEvent(event({ key: "F5", code: "F5" }), "windows")).toBe("F5");
  });

  it("mantiene modificadores y tecla con Shift", () => {
    const optionShiftP = event({
      key: "∏",
      code: "KeyP",
      altKey: true,
      shiftKey: true,
    });
    expect(shortcutFromEvent(optionShiftP, "macos")).toBe("Alt+Shift+P");
  });
});

describe("display", () => {
  it("en Mac muestra símbolos y separa Command de Control", () => {
    expect(shortcutParts("CmdOrCtrl+Shift+D", "macos")).toEqual(["⌘", "⇧", "D"]);
    expect(shortcutParts("Control+Q", "macos")).toEqual(["⌃", "Q"]);
    expect(shortcutParts("Alt+Z", "macos")).toEqual(["⌥", "Z"]);
    expect(shortcutParts("Command+Q", "macos")).toEqual(["⌘", "Q"]);
  });

  it("fuera de Mac muestra nombres", () => {
    expect(shortcutParts("CmdOrCtrl+Shift+D", "windows")).toEqual([
      "Ctrl",
      "Shift",
      "D",
    ]);
    expect(shortcutParts("Alt+Z", "other")).toEqual(["Alt", "Z"]);
    expect(shortcutParts("Super+Q", "windows")).toEqual(["Win", "Q"]);
  });

  it("en texto corrido", () => {
    expect(formatShortcutText("CmdOrCtrl+Space", "macos")).toBe("⌘ + Space");
  });
});

describe("detectShortcutOs", () => {
  it("reconoce el SO del user agent", () => {
    expect(detectShortcutOs("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe(
      "macos",
    );
    expect(detectShortcutOs("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe(
      "windows",
    );
    expect(detectShortcutOs("Mozilla/5.0 (X11; Linux x86_64)")).toBe("other");
  });
});

describe("estado del SO", () => {
  it("los helpers lo usan por defecto", () => {
    setShortcutOs("macos");
    expect(shortcutParts("CmdOrCtrl+P")).toEqual(["⌘", "P"]);
    expect(shortcutFromEvent(event({ key: "q", code: "KeyQ", ctrlKey: true }))).toBe(
      "Control+Q",
    );
    setShortcutOs("other");
  });
});
