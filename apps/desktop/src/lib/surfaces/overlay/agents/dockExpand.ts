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
