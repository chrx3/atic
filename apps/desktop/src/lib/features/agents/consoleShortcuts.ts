/**
 * Atajos de teclado de la consola de agentes, editables.
 *
 * Las cuatro acciones de estructura (dividir, dividir abajo, nueva, cerrar)
 * se leen de config en vez de vivir hardcodeadas en el matcher. El zoom no
 * entra: sigue la convención del navegador (Ctrl + / − / 0) y moverla
 * rompería el hábito más de lo que aporta.
 *
 * El formato del atajo es el de los atajos globales (`core/hotkeys`): tokens
 * "+"-separados, modificador normalizado por plataforma (`CmdOrCtrl`,
 * `Control`, `Command`, `Super`, `Alt`, `Shift`) y la tecla final tal como
 * `keyFromCode` la escribe (`KeyD` → `D`).
 */

import { keyFromCode } from "$core/hotkeys";

export type ConsoleAction =
  "split-right" | "split-down" | "new-console" | "close-console";

export const CONSOLE_ACTION_ORDER: ConsoleAction[] = [
  "split-right",
  "split-down",
  "new-console",
  "close-console",
];

export const CONSOLE_SHORTCUT_DEFAULTS: Record<ConsoleAction, string> = {
  "split-right": "CmdOrCtrl+D",
  "split-down": "CmdOrCtrl+Shift+D",
  "new-console": "CmdOrCtrl+N",
  "close-console": "CmdOrCtrl+W",
};

const STORAGE_KEY = "atic.agents.consoleShortcuts";

/** La huella de un atajo: qué modificador exige y qué tecla cierra el acorde. */
export type ConsoleChord = {
  /** `either` = CmdOrCtrl (Ctrl o ⌘); `ctrl`/`meta` = esa tecla y solo esa. */
  mod: "ctrl" | "meta" | "either";
  alt: boolean;
  shift: boolean;
  /** Tecla final en mayúscula: "D", "2", "Space", "NumpadAdd". */
  key: string;
};

const MODIFIER_TOKENS = new Set([
  "cmdorctrl",
  "commandorctrl",
  "cmdorcontrol",
  "commandorcontrol",
  "control",
  "ctrl",
  "command",
  "cmd",
  "super",
  "alt",
  "option",
  "opt",
  "shift",
]);

export function parseChord(raw: string): ConsoleChord | null {
  const tokens = raw
    .split("+")
    .map((token) => token.trim())
    .filter(Boolean);
  if (tokens.length < 2) return null;

  const chord: ConsoleChord = { mod: "either", alt: false, shift: false, key: "" };
  let ctrlSeen = false;
  let altSeen = false;
  let shiftSeen = false;

  for (const token of tokens) {
    const lower = token.toLowerCase();
    if (MODIFIER_TOKENS.has(lower)) {
      if (lower.includes("commandor") || lower === "cmdorctrl") {
        if (ctrlSeen) return null; // dos modificadores del mismo eje
        ctrlSeen = true;
        chord.mod = "either";
      } else if (lower === "control" || lower === "ctrl") {
        if (ctrlSeen) return null;
        ctrlSeen = true;
        chord.mod = "ctrl";
      } else if (lower === "command" || lower === "cmd" || lower === "super") {
        if (ctrlSeen) return null;
        ctrlSeen = true;
        chord.mod = "meta";
      } else if (lower === "shift") {
        if (shiftSeen) return null;
        shiftSeen = true;
        chord.shift = true;
      } else {
        if (altSeen) return null;
        altSeen = true;
        chord.alt = true;
      }
      continue;
    }
    if (chord.key) return null; // dos teclas no forman un acorde
    chord.key = token.toUpperCase();
  }

  if (!chord.key || MODIFIER_TOKENS.has(chord.key.toLowerCase())) return null;
  // La consola vive dentro de un terminal: sin Ctrl/⌘, el CLI se come la
  // tecla antes de que Atic la vea. Alt solo no alcanza.
  if (!ctrlSeen) return null;
  return chord;
}

/** ¿El evento ejecuta este acorde? Respeta Ctrl ≠ ⌘ cuando el atajo distingue. */
export function chordMatches(
  chord: ConsoleChord,
  e: {
    ctrlKey: boolean;
    metaKey: boolean;
    altKey: boolean;
    shiftKey: boolean;
    key: string;
    code: string;
  },
): boolean {
  const mod =
    chord.mod === "either"
      ? e.ctrlKey || e.metaKey
      : chord.mod === "ctrl"
        ? e.ctrlKey
        : e.metaKey;
  if (!mod) return false;
  if (chord.alt !== e.altKey) return false;
  if (chord.shift !== e.shiftKey) return false;

  const key = chord.key.toUpperCase();
  // Por `key` y por `code`: WebView2 a veces transforma `event.key` en el
  // carácter de control antes de que el acorde llegue, y el `code` sobrevive.
  return e.key.toUpperCase() === key || keyFromCode(e.code).toUpperCase() === key;
}

/** Byte de control para la vía `onData`: solo Ctrl + letra seca lo tiene. */
export function controlByte(raw: string): string | null {
  const chord = parseChord(raw);
  if (!chord || chord.mod === "meta" || chord.alt || chord.shift) return null;
  if (!/^[A-Z]$/.test(chord.key)) return null;
  return String.fromCharCode(chord.key.charCodeAt(0) - 64);
}

export type ShortcutCheck =
  { ok: true } | { ok: false; reason: "invalid" | "in-use" | "zoom" };

/** Las teclas que el zoom (fijo, convención de navegador) ya se reservó. */
const ZOOM_KEYS = new Set([
  "+",
  "=",
  "-",
  "0",
  "NUMPADADD",
  "NUMPADSUBTRACT",
  "NUMPAD0",
]);

function normalize(raw: string): string {
  return raw
    .split("+")
    .map((token) => token.trim().toLowerCase())
    .filter(Boolean)
    .sort()
    .join("+");
}

export function validateConsoleShortcut(
  raw: string,
  current: Record<ConsoleAction, string>,
  action: ConsoleAction,
): ShortcutCheck {
  const chord = parseChord(raw);
  if (!chord) return { ok: false, reason: "invalid" };
  if (ZOOM_KEYS.has(chord.key) || ZOOM_KEYS.has(chord.key.toUpperCase())) {
    return { ok: false, reason: "zoom" };
  }
  const fingerprint = normalize(raw);
  for (const other of CONSOLE_ACTION_ORDER) {
    if (other === action) continue;
    if (normalize(current[other]) === fingerprint)
      return { ok: false, reason: "in-use" };
  }
  return { ok: true };
}

export function loadConsoleShortcuts(): Record<ConsoleAction, string> {
  const out = { ...CONSOLE_SHORTCUT_DEFAULTS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return out;
    const saved = JSON.parse(raw) as Partial<Record<ConsoleAction, string>>;
    for (const action of CONSOLE_ACTION_ORDER) {
      const value = saved[action];
      // Solo entran acordes que el matcher pueda ejecutar hoy: con modificador
      // Ctrl/⌘ y una tecla. Lo demás vuelve al default en silencio.
      if (typeof value === "string" && parseChord(value)) out[action] = value;
    }
  } catch {
    /* config corrupta o storage bloqueado: quedan los default */
  }
  return out;
}

export function saveConsoleShortcuts(shortcuts: Record<ConsoleAction, string>): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(shortcuts));
  } catch {
    /* sin storage: los atajos viven hasta recargar, igual que el resto */
  }
}
