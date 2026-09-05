/**
 * El expand de la pestaña: la pill llama, el float agranda.
 *
 * El estado `minimized` vive en `agentsDock.svelte.ts` ($state) para que la
 * barra reaccione. Acá queda el puente, testeable sin runes.
 */

/** Reusar el marco achicado: hay globo vivo. Si no, hay que nacer de la pill. */
export function reuseDockedFrame(state: {
  minimized: boolean;
  alive: boolean;
  hasAnchor: boolean;
}): boolean {
  return state.minimized && state.alive && state.hasAnchor;
}

/**
 * Hay que animar el marco: cambió la vista, o la consola quedó con el
 * tamaño del selector (esconder desde Agentes y volver a abrir).
 */
export function shouldResizeLauncher(state: {
  current: "setup" | "console";
  next: "setup" | "console";
  height?: number;
  minConsoleHeight: number;
}): boolean {
  if (state.current !== state.next) return true;
  return (
    state.next === "console" &&
    state.height != null &&
    state.height < state.minConsoleHeight
  );
}

/**
 * El morph de nacimiento es un disco de ~40 px. Si se guarda como ancho del
 * selector, el próximo open nace como una tira y no hay forma de leerlo.
 */
export function rememberedSetupWidth(width: number, min: number): number {
  return Number.isFinite(width) && width >= min ? width : min;
}

export type DockExpand = {
  bind(expand: () => void): () => void;
  call(): void;
};

export function createDockExpand(): DockExpand {
  let expand: (() => void) | null = null;
  return {
    bind(fn) {
      expand = fn;
      return () => {
        if (expand === fn) expand = null;
      };
    },
    call() {
      expand?.();
    },
  };
}
