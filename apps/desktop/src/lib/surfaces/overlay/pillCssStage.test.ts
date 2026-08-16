import { describe, expect, it } from "vitest";

import { clampTo } from "./pillCssStage";
import type { Area } from "$ipc/overlay";

const DISC = { w: 40, h: 40 };
const VIEW = { w: 1920, h: 1080 };
const SOLO: Area[] = [{ x: 0, y: 0, w: 1920, h: 1080 }];

describe("clampTo", () => {
  it("deja pegar al borde superior del monitor", () => {
    expect(clampTo(SOLO, { x: 100, y: 0 }, DISC, VIEW)).toEqual({ x: 100, y: 0 });
  });

  it("no deja y negativa aunque el monitor empiece por encima del CSS", () => {
    // Origen del cliente más abajo que bounds: overflow:hidden recorta a y=0.
    const shifted: Area[] = [{ x: 0, y: -40, w: 1920, h: 1080 }];
    expect(clampTo(shifted, { x: 100, y: -20 }, DISC, VIEW)).toEqual({
      x: 100,
      y: 0,
    });
  });

  it("no deja salir por debajo del viewport CSS", () => {
    expect(clampTo(SOLO, { x: 100, y: 1070 }, DISC, VIEW)).toEqual({
      x: 100,
      y: 1040,
    });
  });

  it("sin áreas, igual se queda dentro del viewport", () => {
    expect(clampTo([], { x: 10, y: -8 }, DISC, VIEW)).toEqual({ x: 10, y: 0 });
  });
});
