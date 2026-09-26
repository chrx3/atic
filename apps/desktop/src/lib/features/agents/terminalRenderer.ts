import { WebglAddon } from "@xterm/addon-webgl";
import type { Terminal } from "@xterm/xterm";

/**
 * Pinta el xterm con WebGL. Va después de `term.open()`.
 *
 * xterm 6 ya no trae el renderer de canvas: sin esto usa el DOM, y un TUI de
 * agente que redibuja la pantalla entera varias veces por segundo es su peor
 * caso. Si el contexto se pierde (GPU reiniciada, demasiados terminales
 * abiertos) el addon se suelta y xterm vuelve solo al DOM.
 */
export function useWebglRenderer(term: Terminal): void {
  try {
    const webgl = new WebglAddon();
    webgl.onContextLoss(() => webgl.dispose());
    term.loadAddon(webgl);
  } catch {
    /* sin WebGL2: queda el renderer DOM */
  }
}
