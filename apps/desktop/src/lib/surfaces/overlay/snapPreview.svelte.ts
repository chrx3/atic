/**
 * Fantasma del snap: lo escribe el float que se arrastra y lo pinta
 * OverlaySurface, debajo del globo, en coords del overlay.
 */

export type SnapRect = { x: number; y: number; w: number; h: number };

export const snapPreview = $state<{ frame: SnapRect | null }>({
  frame: null,
});
