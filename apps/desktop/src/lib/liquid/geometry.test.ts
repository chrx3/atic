import { describe, expect, it } from "vitest";
import { boxShape, gapBetween, pillShape, stemBetween } from "./geometry";
import { BLEND, BULGE, CELL, INFLUENCE, REACH } from "./constants";
import { smin } from "./sdf";
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

describe("stemBetween", () => {
  const pill = { x: 100, y: 0, w: 34, h: 34 };
  const below = { x: 40, y: 50, w: 160, h: 200 };

  it("cuelga del centro del ancla hacia el panel de abajo", () => {
    const stem = stemBetween(pill, below, "top", 4);
    expect(stem).toEqual({
      kind: "capsule",
      ax: 117,
      ay: 28,
      bx: 117,
      by: 56,
      r: 4,
    });
  });

  it("no dibuja hilo si las dos formas ya se tocan", () => {
    expect(stemBetween(pill, { x: 40, y: 24, w: 160, h: 20 }, "top", 4)).toBeNull();
  });

  it("conecta en horizontal cuando el panel queda a la derecha", () => {
    const stem = stemBetween(pill, { x: 150, y: 0, w: 100, h: 80 }, "left", 4);
    expect(stem?.kind).toBe("capsule");
    if (stem?.kind !== "capsule") return;
    expect(stem.ay).toBe(stem.by);
    expect(stem.ay).toBe(17);
    expect(stem.bx).toBeGreaterThan(stem.ax);
  });

  it("colgado de una esquina, el hilo cae sobre el panel y no al lado", () => {
    // El panel nace del canto derecho de la isla: se solapan 20 px.
    const corner = { x: 114, y: 50, w: 200, h: 160 };
    const stem = stemBetween(pill, corner, "top", 4);
    expect(stem?.kind).toBe("capsule");
    if (stem?.kind !== "capsule") return;
    expect(stem.ax).toBe(stem.bx);
    expect(stem.ax).toBeGreaterThanOrEqual(corner.x);
    expect(stem.ax).toBeLessThanOrEqual(pill.x + pill.w);
  });

  it("sin solape no hay hilo posible: mejor ninguno que uno en el aire", () => {
    expect(stemBetween(pill, { x: 300, y: 50, w: 200, h: 160 }, "top", 4)).toBeNull();
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
  it("REACH = blend/2 (hueco de 12 px de la app)", () => {
    expect(BLEND).toBe(24);
    expect(REACH).toBe(12);
  });

  /**
   * Los dos umbrales miden cosas distintas y el doble no es casual: entre
   * INFLUENCE y REACH las siluetas ya se deforman una hacia la otra. Agrupar
   * islas por REACH se saltea ese tramo y la junta aparece de golpe.
   */
  it("INFLUENCE = blend, el doble de REACH", () => {
    expect(INFLUENCE).toBe(24);
    expect(INFLUENCE).toBe(REACH * 2);
  });

  it("el smin ya mueve la superficie antes de que cierre el cuello", () => {
    // Cara de una forma (d=0) frente a otra a distancia g.
    const face = (g: number) => smin(0, g, BLEND);
    // En INFLUENCE todavía no pasa nada; entrando, se estira progresivamente.
    expect(face(INFLUENCE)).toBeCloseTo(0, 6);
    expect(face(18)).toBeLessThan(0);
    expect(face(REACH)).toBeLessThan(face(18));
    // Ese salto de 1.5 px es lo que antes entraba en un solo frame.
    expect(face(REACH)).toBeCloseTo(-1.5, 6);
  });

  /**
   * La razón de ser de todo esto: el SDF forma el cuello solo (g ≤ k/2),
   * sin las cinco constantes que el filtro SVG necesitaba para un hueco de 10.
   */
  it("la pill y la burbuja separadas 10 px quedan en un solo trazo", () => {
    const pill = pillShape({ x: 700, y: 800, w: 176, h: 40 });
    const bubble = boxShape({ x: 410, y: 270, w: 580, h: 520 }, 26);
    const field = new Field([pill, bubble], BLEND);
    // El punto medio del hueco: entre el borde de arriba de la pill (800) y el
    // de abajo de la burbuja (790).
    expect(field.eval(788, 795)).toBeLessThanOrEqual(0);
  });

  it("cell y bulge quedan en los defaults del launcher", () => {
    expect(CELL).toBe(6);
    expect(BULGE).toBe(6);
  });
});
