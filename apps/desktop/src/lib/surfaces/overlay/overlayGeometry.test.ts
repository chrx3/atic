import { describe, expect, it } from "vitest";
import { viewportShifted } from "./overlayGeometry";

describe("viewportShifted", () => {
  it("ignora el temblor de un par de píxeles", () => {
    expect(viewportShifted({ w: 1551, h: 864 }, { w: 1554, h: 864 })).toBe(false);
  });

  it("detecta el salto del recuadro chico al escritorio real", () => {
    expect(viewportShifted({ w: 1551, h: 864 }, { w: 3840, h: 1080 })).toBe(true);
  });

  it("también cuenta un encogimiento", () => {
    expect(viewportShifted({ w: 3840, h: 1080 }, { w: 1920, h: 1080 })).toBe(true);
  });
});
