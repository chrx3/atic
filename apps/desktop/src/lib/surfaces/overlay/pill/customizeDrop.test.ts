import { describe, expect, it } from "vitest";

import {
  CUSTOMIZE_CHIP,
  CUSTOMIZE_GAP,
  dropSlot,
  slotIndex,
  type DropRow,
} from "./customizeDrop";

const pitch = CUSTOMIZE_CHIP + CUSTOMIZE_GAP;
/** Grilla de 8 fichas por línea. */
const grid = { x: 100, y: 50, w: 8 * pitch - CUSTOMIZE_GAP, h: CUSTOMIZE_CHIP };
const center = (k: number) => grid.x + k * pitch + CUSTOMIZE_CHIP / 2;

describe("slotIndex", () => {
  it("antes del centro de la primera, entra primera", () => {
    expect(slotIndex(grid, 4, { x: grid.x, y: 60 })).toBe(0);
    expect(slotIndex(grid, 4, { x: center(0) - 1, y: 60 })).toBe(0);
  });

  it("pasado el centro de una ficha, entra después de ella", () => {
    expect(slotIndex(grid, 4, { x: center(0) + 1, y: 60 })).toBe(1);
    expect(slotIndex(grid, 4, { x: center(2) + 1, y: 60 })).toBe(3);
  });

  it("nunca más allá del final de la fila", () => {
    expect(slotIndex(grid, 2, { x: center(6), y: 60 })).toBe(2);
    expect(slotIndex(grid, 0, { x: center(3), y: 60 })).toBe(0);
  });

  it("la segunda línea sigue la cuenta de la primera", () => {
    expect(slotIndex(grid, 12, { x: center(1) + 1, y: grid.y + pitch + 5 })).toBe(10);
  });

  it("no se va a negativo a la izquierda ni arriba", () => {
    expect(slotIndex(grid, 4, { x: 0, y: 0 })).toBe(0);
  });
});

describe("dropSlot", () => {
  const rows: DropRow[] = [
    {
      bucket: "ring",
      zone: { x: 0, y: 0, w: 400, h: 60 },
      grid: { ...grid, y: 20 },
      count: 3,
    },
    {
      bucket: "more",
      zone: { x: 0, y: 70, w: 400, h: 60 },
      grid: { ...grid, y: 90 },
      count: 1,
    },
    {
      bucket: "hidden",
      zone: { x: 0, y: 140, w: 400, h: 60 },
      grid: { ...grid, y: 160 },
      count: 4,
    },
  ];

  it("elige la fila bajo el cursor", () => {
    expect(dropSlot(rows, { x: center(0) + 1, y: 95 })).toEqual({
      bucket: "more",
      index: 1,
    });
  });

  it("entre filas, la más cercana", () => {
    expect(dropSlot(rows, { x: grid.x, y: 66 })).toEqual({ bucket: "more", index: 0 });
    expect(dropSlot(rows, { x: grid.x, y: 400 })?.bucket).toBe("hidden");
  });

  it("sin filas no hay destino", () => {
    expect(dropSlot([], { x: 0, y: 0 })).toBeNull();
  });
});
