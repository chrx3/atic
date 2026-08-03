import { describe, expect, it } from "vitest";
import { boxShape, gapBetween, pillShape } from "./geometry";
import { BLEND, BULGE, CELL, REACH } from "./constants";
import { Field, shapeSD } from "./sdf";

describe("boxShape", () => {
  it("pasa de esquina a centro sin perder medio píxel", () => {
    const s = boxShape({ x: 10, y: 20, w: 100, h: 40 }, 8);
    expect(s).toEqual({ kind: "box", cx: 60, cy: 40, hw: 50, hh: 20, r: 8 });
  });

  it("la forma vale 0 justo en el borde del rectángulo pedido", () => {
    const rect = { x: 10, y: 20, w: 100, h: 40 };
    const s = boxShape(rect, 8);
    expect(shapeSD(s, rect.x + rect.w, rect.y + rect.h / 2)).toBeCloseTo(0);
    expect(shapeSD(s, rect.x, rect.y + rect.h / 2)).toBeCloseTo(0);
  });
});

describe("pillShape", () => {
  it("redondea hasta la mitad del lado corto", () => {
    expect(pillShape({ x: 0, y: 0, w: 176, h: 40 }).r).toBe(20);
    expect(pillShape({ x: 0, y: 0, w: 40, h: 176 }).r).toBe(20);
  });
});

describe("gapBetween", () => {
  it("mide por el eje que de verdad separa", () => {
    const a = { x: 0, y: 0, w: 100, h: 100 };
    expect(gapBetween(a, { x: 130, y: 0, w: 100, h: 100 })).toBe(30);
    expect(gapBetween(a, { x: 0, y: 130, w: 100, h: 100 })).toBe(30);
  });

  it("es negativa cuando se solapan", () => {
    const a = { x: 0, y: 0, w: 100, h: 100 };
    expect(gapBetween(a, { x: 80, y: 0, w: 100, h: 100 })).toBeLessThan(0);
  });

  it("da lo mismo en cualquier orden", () => {
    const a = { x: 0, y: 0, w: 50, h: 50 };
    const b = { x: 200, y: 0, w: 50, h: 50 };
    expect(gapBetween(a, b)).toBe(gapBetween(b, a));
  });
});

describe("los valores elegidos", () => {
  it("alcanzan de sobra para el hueco de 10 px de la app", () => {
    expect(REACH).toBeGreaterThan(10);
  });

  /**
   * La razón de ser de todo esto: con el filtro, el alcance era 8.6 px contra
   * un hueco de 10, y por eso el cuello había que dibujarlo con cinco
   * constantes. Acá se forma solo.
   */
  it("la pill y la burbuja separadas 10 px quedan en un solo trazo", () => {
    const pill = pillShape({ x: 700, y: 800, w: 176, h: 40 });
    const bubble = boxShape({ x: 410, y: 270, w: 580, h: 520 }, 26);
    const field = new Field([pill, bubble], BLEND);
    // El punto medio del hueco: entre el borde de arriba de la pill (800) y el
    // de abajo de la burbuja (790).
    expect(field.eval(788, 795)).toBeLessThan(0);
  });

  it("la celda ve el detalle más fino que se dibuja", () => {
    // El cuello más angosto que llega a formarse ronda el bulto; con una celda
    // mayor que eso, aparecería y desaparecería entre cuadros.
    expect(CELL).toBeLessThan(BULGE);
  });
});
