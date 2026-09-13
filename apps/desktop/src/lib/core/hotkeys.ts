/**
 * Atajos: de tecla apretada a string de config, y de ahí a cómo se muestra.
 *
 * Dos cosas que en Mac no son obvias y que viven acá para no repetirlas en cada
 * componente de captura:
 *
 * - Con Option apretada, `KeyboardEvent.key` trae el carácter del layout (`π`
 *   con Option+P, `@` con Option+2) y el parser de Rust no lo reconoce. La
 *   tecla sale de `event.code`, que es física y del layout no depende.
 * - `CmdOrCtrl` es el modificador primario de cada plataforma (⌘ en Mac, Ctrl
 *   en Windows/Linux), pero en Mac Control y Command son teclas distintas:
 *   apretar ⌃ no puede guardarse como `CmdOrCtrl`.
 *
 * El SO lo inyecta el layout con `setShortcutOs`: acá no se toca `navigator`
 * para que el módulo siga siendo TS puro y se pueda testear.
 */

export type ShortcutOs = "macos" | "windows" | "other";

/** Lo mínimo de `KeyboardEvent` que hace falta; estructural, como `KeyMods`. */
export interface ShortcutKeyEvent {
  key: string;
  code: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
}

let os: ShortcutOs = "other";

/** Una vez por ventana, al arrancar el layout. */
export function setShortcutOs(next: ShortcutOs): void {
  os = next;
}

export function shortcutOs(): ShortcutOs {
  return os;
}

export function detectShortcutOs(userAgent: string): ShortcutOs {
  if (/mac/i.test(userAgent)) return "macos";
  if (/win/i.test(userAgent)) return "windows";
  return "other";
}

/** Un modificador solo no es un atajo: se espera a la tecla que lo acompaña. */
export function isModifierOnlyKey(key: string): boolean {
  return ["Control", "Shift", "Alt", "Meta", "OS"].includes(key);
}

/**
 * Tecla FÍSICA a partir de `event.code`: `KeyP` → `P`, `Digit2` → `2`.
 * El resto de los códigos (`Space`, `ArrowUp`, `F5`) el parser ya los entiende.
 */
export function keyFromCode(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  return code;
}

/**
 * String de config para un `keydown`, o `null` si todavía no hay atajo.
 *
 * En Windows, Win la reserva el SO: el componente que captura la rechaza con
 * su aviso antes de llegar acá.
 */
export function shortcutFromEvent(
  e: ShortcutKeyEvent,
  platform: ShortcutOs = os,
): string | null {
  if (isModifierOnlyKey(e.key)) return null;

  const out: string[] = [];
  if (platform === "macos") {
    // Control y Command son teclas distintas: apretar ⌃ no es apretar ⌘.
    if (e.metaKey) out.push("Command");
    if (e.ctrlKey) out.push("Control");
  } else {
    if (e.ctrlKey) out.push("CmdOrCtrl");
    // Linux: `CmdOrCtrl` resolvería a Control y guardaría mal la tecla Super.
    if (e.metaKey) out.push("Super");
  }
  if (e.altKey) out.push("Alt");
  if (e.shiftKey) out.push("Shift");

  let key = e.code ? keyFromCode(e.code) : e.key;
  if (key === " ") key = "Space";
  else if (key.length === 1) key = key.toUpperCase();
  else if (/^F\d{1,2}$/i.test(key)) key = key.toUpperCase();

  // Sin modificador no hay atajo global posible, salvo las F.
  if (out.length === 0 && !/^F\d{1,2}$/i.test(key)) return null;
  out.push(key);
  return out.join("+");
}

/** Cada token como se muestra: símbolos en Mac, nombres en el resto. */
function tokenLabel(token: string, platform: ShortcutOs): string {
  switch (token.toLowerCase()) {
    case "cmdorctrl":
    case "cmdorcontrol":
    case "commandorctrl":
    case "commandorcontrol":
      return platform === "macos" ? "⌘" : "Ctrl";
    case "command":
    case "cmd":
    case "super":
      // `Super` es Command en Mac y la tecla Win en Windows.
      return platform === "macos" ? "⌘" : "Win";
    case "control":
    case "ctrl":
      return platform === "macos" ? "⌃" : "Ctrl";
    case "alt":
    case "option":
      return platform === "macos" ? "⌥" : "Alt";
    case "shift":
      return platform === "macos" ? "⇧" : "Shift";
    default:
      return token;
  }
}

/** Partes de un atajo, para dibujar tecla por tecla. */
export function shortcutParts(raw: string, platform: ShortcutOs = os): string[] {
  return raw
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => tokenLabel(part, platform));
}

/** Atajo legible para texto corrido: `CmdOrCtrl+Shift+P` → `⌘ + ⇧ + P`. */
export function formatShortcutText(raw: string, platform: ShortcutOs = os): string {
  return shortcutParts(raw, platform).join(" + ");
}
