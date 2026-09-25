import { describe, expect, it, vi } from "vitest";

import { clampTo, clampToMonitor, createCssStage } from "./pillCssStage";
import type { Area } from "$ipc/overlay";

/** Dos pantallas lado a lado; la de la derecha con barra de tareas abajo. */
const DUAL_TASKBAR: Area[] = [
  { x: 0, y: 0, w: 1000, h: 800 },
  { x: 1000, y: 0, w: 1000, h: 800, work: { x: 1000, y: 0, w: 1000, h: 760 } },
];

vi.mock("$ipc/overlay", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$ipc/overlay")>()),
  overlayWorkAreas: () => Promise.resolve(DUAL_TASKBAR),
  overlayCursor: () => Promise.resolve(null),
}));

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

describe("clampToMonitor", () => {
  const view = { w: 2000, h: 800 };

  it("una cara que crece junto al borde entre pantallas no se parte en dos", () => {
    // Notch arriba, a 60 px del canto interior; la cara mide 300 de ancho.
    const anchor = { x: 940, y: 20 };
    expect(
      clampToMonitor(DUAL_TASKBAR, anchor, { x: 790, y: 0 }, { w: 300, h: 250 }, view),
    ).toEqual({ x: 700, y: 0 });
  });

  it("no se mete bajo la barra de tareas de su monitor", () => {
    const anchor = { x: 1980, y: 700 };
    const p = clampToMonitor(
      DUAL_TASKBAR,
      anchor,
      { x: 1700, y: 600 },
      { w: 300, h: 430 },
      view,
    );
    expect(p.y + 430).toBeLessThanOrEqual(760);
  });

  it("sin monitor bajo el ancla se comporta como clampTo", () => {
    const p = { x: 10, y: 10 };
    expect(clampToMonitor([], { x: -50, y: -50 }, p, DISC, view)).toEqual(
      clampTo([], p, DISC, view),
    );
  });
});

describe("createCssStage acoplada", () => {
  it("abrir y cerrar una cara cerca de una esquina no la corre por el canto", async () => {
    const stage = createCssStage();
    await stage.loadAreas();
    const tab = { w: 40, h: 124 };
    // Pestaña a la izquierda, casi arriba del todo.
    await stage.resize(tab);
    stage.moveTo({ x: 0, y: 20 });
    const rest = stage.at();
    for (let i = 0; i < 3; i++) {
      await stage.resize({ w: 300, h: 430 }, "dockLeft");
      await stage.resize(tab, "dockLeft");
    }
    expect(stage.at()).toEqual(rest);
  });
});
