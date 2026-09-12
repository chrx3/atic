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

  it("permite cruzar de un monitor al vecino", () => {
    const dual: Area[] = [
      { x: 0, y: 0, w: 1000, h: 800 },
      { x: 1000, y: 0, w: 1000, h: 800 },
    ];
    const view = { w: 2000, h: 800 };
    // Centro todavía en el primero: el clamp al monitor actual la dejaba
    // atascada en x=960 y nunca pasaba. Con la unión sí cruza.
    expect(clampTo(dual, { x: 980, y: 100 }, DISC, view)).toEqual({
      x: 980,
      y: 100,
    });
    expect(clampTo(dual, { x: 1200, y: 100 }, DISC, view)).toEqual({
      x: 1200,
      y: 100,
    });
  });
});
