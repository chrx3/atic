import { describe, expect, it } from "vitest";
import {
  chordMatches,
  controlByte,
  parseChord,
  validateConsoleShortcut,
  CONSOLE_SHORTCUT_DEFAULTS,
} from "./consoleShortcuts";

function event(
  overrides: Partial<{
    ctrlKey: boolean;
    metaKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
    key: string;
    code: string;
  }> = {},
) {
  return {
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    key: "",
    code: "",
    ...overrides,
  };
}

describe("parseChord", () => {
  it("lee el formato de los atajos globales", () => {
    expect(parseChord("CmdOrCtrl+D")).toEqual({
      mod: "either",
      alt: false,
      shift: false,
      key: "D",
    });
    expect(parseChord("CmdOrCtrl+Shift+D")?.shift).toBe(true);
    expect(parseChord("Control+Alt+K")).toEqual({
      mod: "ctrl",
      alt: true,
      shift: false,
      key: "K",
    });
    expect(parseChord("Command+K")?.mod).toBe("meta");
  });

  it("rechaza lo que la consola no puede ejecutar", () => {
    // Sin modificador: el CLI se come la tecla.
    expect(parseChord("D")).toBeNull();
    // Tecla duplicada / solo modificadores / dos modificadores de pila.
    expect(parseChord("CmdOrCtrl+D+D")).toBeNull();
    expect(parseChord("CmdOrCtrl+Shift")).toBeNull();
    expect(parseChord("Control+Command+K")).toBeNull();
    expect(parseChord("")).toBeNull();
  });
});

describe("chordMatches", () => {
  const chord = parseChord("CmdOrCtrl+D");

  it("acepta Ctrl y ⌘ (semántica CmdOrCtrl)", () => {
    expect(
      chord && chordMatches(chord, event({ ctrlKey: true, key: "d", code: "KeyD" })),
    ).toBe(true);
    expect(
      chord && chordMatches(chord, event({ metaKey: true, key: "d", code: "KeyD" })),
    ).toBe(true);
  });

  it("mira el `code` cuando el keydown llegó transformado", () => {
    expect(
      chord && chordMatches(chord, event({ ctrlKey: true, key: "\x04", code: "KeyD" })),
    ).toBe(true);
  });

  it("rechaza flags de más", () => {
    expect(
      chord &&
        chordMatches(
          chord,
          event({ ctrlKey: true, altKey: true, key: "d", code: "KeyD" }),
        ),
    ).toBe(false);
    expect(
      chord &&
        chordMatches(
          chord,
          event({ ctrlKey: true, shiftKey: true, key: "D", code: "KeyD" }),
        ),
    ).toBe(false);
  });

  it("no confunde ⌘ con Ctrl cuando el atajo pide Control", () => {
    const control = parseChord("Control+D");
    expect(
      control &&
        chordMatches(control, event({ metaKey: true, key: "d", code: "KeyD" })),
    ).toBe(false);
  });
});

describe("controlByte", () => {
  it("da el byte de control de un Ctrl+letra seco", () => {
    expect(controlByte("CmdOrCtrl+D")).toBe("\x04");
    expect(controlByte("CmdOrCtrl+N")).toBe("\x0e");
    expect(controlByte("CmdOrCtrl+W")).toBe("\x17");
  });

  it("se abstiene en acordes que no son Ctrl+letra", () => {
    expect(controlByte("CmdOrCtrl+Shift+D")).toBeNull();
    expect(controlByte("CmdOrCtrl+0")).toBeNull();
    expect(controlByte("Command+K")).toBeNull();
  });

  it("los defaults dan los bytes que hoy consume la PTY", () => {
    expect(controlByte(CONSOLE_SHORTCUT_DEFAULTS["new-console"])).toBe("\x0e");
    expect(controlByte(CONSOLE_SHORTCUT_DEFAULTS["close-console"])).toBe("\x17");
  });
});

describe("validateConsoleShortcut", () => {
  const current = { ...CONSOLE_SHORTCUT_DEFAULTS };

  it("acepta un reemplazo válido", () => {
    expect(validateConsoleShortcut("CmdOrCtrl+E", current, "new-console")).toEqual({
      ok: true,
    });
  });

  it("rechaza duplicado y zoom", () => {
    expect(validateConsoleShortcut("CmdOrCtrl+D", current, "new-console")).toEqual({
      ok: false,
      reason: "in-use",
    });
    expect(validateConsoleShortcut("CmdOrCtrl+0", current, "close-console")).toEqual({
      ok: false,
      reason: "zoom",
    });
    expect(validateConsoleShortcut("CmdOrCtrl+-", current, "new-console")).toEqual({
      ok: false,
      reason: "zoom",
    });
  });

  it("rechaza lo que no es acorde con modificador", () => {
    expect(validateConsoleShortcut("D", current, "split-right")).toEqual({
      ok: false,
      reason: "invalid",
    });
    expect(validateConsoleShortcut("Alt+D", current, "split-right")).toEqual({
      ok: false,
      reason: "invalid",
    });
  });
});
