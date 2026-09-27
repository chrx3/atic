import { describe, expect, it } from "vitest";
import { unscalePoint } from "./xtermScale";

describe("unscalePoint", () => {
  it("lleva el punto al espacio sin escalar desde la esquina del terminal", () => {
    // Pizarra al 50 %: 100 px en pantalla son 200 px de terminal.
    expect(unscalePoint({ x: 150, y: 90 }, { left: 50, top: 40 }, 0.5)).toEqual({
      x: 250,
      y: 140,
    });
  });

  it("sin zoom deja el punto igual", () => {
    expect(unscalePoint({ x: 12, y: 34 }, { left: 5, top: 6 }, 1)).toEqual({
      x: 12,
      y: 34,
    });
  });
});
