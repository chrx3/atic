import { describe, expect, it } from "vitest";
import { nextIndex } from "./listNav";

describe("nextIndex", () => {
  it("no navega una colección vacía", () => {
    expect(nextIndex("ArrowDown", -1, 0)).toBeNull();
    expect(nextIndex("Home", 0, 0)).toBeNull();
  });

  it("devuelve null para teclas que no navegan", () => {
    expect(nextIndex("Enter", 0, 5)).toBeNull();
    expect(nextIndex("a", 0, 5)).toBeNull();
    expect(nextIndex("Escape", 0, 5)).toBeNull();
  });

  it("en una lista ignora las flechas horizontales", () => {
    expect(nextIndex("ArrowRight", 0, 5)).toBeNull();
    expect(nextIndex("ArrowLeft", 3, 5)).toBeNull();
  });

  it("sin selección elige un extremo en vez de moverse desde cero", () => {
    expect(nextIndex("ArrowDown", -1, 5)).toBe(0);
    expect(nextIndex("ArrowUp", -1, 5)).toBe(4);
    expect(nextIndex("End", -1, 5)).toBe(4);
  });

  it("se detiene en los bordes en vez de dar la vuelta", () => {
    expect(nextIndex("ArrowUp", 0, 5)).toBe(0);
    expect(nextIndex("ArrowDown", 4, 5)).toBe(4);
  });

  it("en una grilla baja por columnas y se mueve de a uno en horizontal", () => {
    // 7 ítems en 3 columnas:  0 1 2 / 3 4 5 / 6
    expect(nextIndex("ArrowDown", 1, 7, 3)).toBe(4);
    expect(nextIndex("ArrowUp", 4, 7, 3)).toBe(1);
    expect(nextIndex("ArrowRight", 1, 7, 3)).toBe(2);
    expect(nextIndex("ArrowLeft", 1, 7, 3)).toBe(0);
  });

  it("en la última fila incompleta baja al último y no al vacío", () => {
    expect(nextIndex("ArrowDown", 5, 7, 3)).toBe(6);
    expect(nextIndex("ArrowDown", 4, 7, 3)).toBe(6);
  });

  it("Home y End van a los extremos", () => {
    expect(nextIndex("Home", 4, 5)).toBe(0);
    expect(nextIndex("End", 0, 5)).toBe(4);
  });
});
