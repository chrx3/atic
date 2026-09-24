/**
 * La paleta del xterm, clara u oscura según el tema de la app.
 *
 * Aparte del componente porque la usan dos: el panel de consolas y la vista
 * de terminal de la ventana de agentes. Colores con contraste mínimo 4.5 con
 * su fondo; el acento (cursor, selección) es el coral de la marca.
 */
import { themeBase } from "$lib/theme";

export function terminalTheme(): Record<string, string> {
  const light = themeBase(document.documentElement.dataset.theme) === "light";
  return light
    ? {
        background: "#fbfbf8",
        foreground: "#24241f",
        cursor: "#d35f45",
        cursorAccent: "#fbfbf8",
        selectionBackground: "rgba(218, 119, 86, 0.3)",
        black: "#31312c",
        red: "#b43d3d",
        green: "#2f774d",
        yellow: "#806000",
        blue: "#3569a3",
        magenta: "#7d50a1",
        cyan: "#267580",
        white: "#d8d8d0",
        brightBlack: "#74746b",
        brightRed: "#d5544f",
        brightGreen: "#3b9360",
        brightYellow: "#a57a00",
        brightBlue: "#4b83c4",
        brightMagenta: "#9a68bf",
        brightCyan: "#3693a0",
        brightWhite: "#ffffff",
      }
    : {
        background: "#151715",
        foreground: "#e8e8e1",
        cursor: "#e36f52",
        cursorAccent: "#151715",
        selectionBackground: "rgba(218, 119, 86, 0.35)",
        black: "#22241f",
        red: "#e0675f",
        green: "#73b98d",
        yellow: "#d4ad58",
        blue: "#78a9d4",
        magenta: "#b18bd0",
        cyan: "#69b5bd",
        white: "#d9d9d2",
        brightBlack: "#777970",
        brightRed: "#f17b71",
        brightGreen: "#8ed0a4",
        brightYellow: "#e8c572",
        brightBlue: "#94c0e5",
        brightMagenta: "#c9a4e3",
        brightCyan: "#83cbd2",
        brightWhite: "#ffffff",
      };
}
