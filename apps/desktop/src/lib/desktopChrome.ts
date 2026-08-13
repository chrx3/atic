/**
 * Atajos de Chromium/WebView2 que no existen en una app de escritorio.
 *
 * Ctrl+C/V/X/A/Z (edición) y Ctrl+K (búsqueda in-app) se dejan pasar.
 * Ctrl+Alt+* son labs de desarrollo. En DEV se permite recargar y DevTools.
 */

export type KeyMods = {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
};

export function isBrowserChromeShortcut(event: KeyMods): boolean {
  const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
  const ctrl = event.ctrlKey || event.metaKey;
  const dev = import.meta.env.DEV;

  if (key === "F12" || key === "F3") return !dev;
  if (key === "F5") return !dev;
  if (!ctrl) return false;
  if (event.altKey) return false;

  if (event.shiftKey) {
    if (key === "i" || key === "j" || key === "c") return !dev;
    if (key === "p") return true;
    if (key === "r") return !dev;
    return false;
  }

  switch (key) {
    case "p":
    case "f":
    case "g":
    case "u":
    case "s":
    case "j":
    case "n":
    case "t":
    case "o":
    case "d":
    case "h":
    case "+":
    case "-":
    case "=":
    case "0":
      return true;
    case "r":
      return !dev;
    default:
      return false;
  }
}

export function installDesktopChromeGuards(): () => void {
  const onKey = (event: KeyboardEvent) => {
    if (!isBrowserChromeShortcut(event)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  window.addEventListener("keydown", onKey, true);
  const print = window.print.bind(window);
  window.print = () => {};
  return () => {
    window.removeEventListener("keydown", onKey, true);
    window.print = print;
  };
}
