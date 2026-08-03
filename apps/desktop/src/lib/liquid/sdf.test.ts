import { describe, expect, it } from "vitest";
import {
  Field,
  sdCapsule,
  sdRoundBox,
  smin,
  sminBulge,
  sminReach,
  type Shape,
} from "./sdf";

describe("sdRoundBox", () => {
  it("vale 0 en el borde y el radio menor en el centro", () => {
    const box = [100, 100, 50, 20, 8] as const;
    expect(sdRoundBox(150, 100, ...box)).toBeCloseTo(0);
    expect(sdRoundBox(100, 120, ...box)).toBeCloseTo(0);
    expect(sdRoundBox(100, 100, ...box)).toBeCloseTo(-20);
  });

  it("crece 1 por cada píxel que uno se aleja", () => {
    expect(sdRoundBox(160, 100, 100, 100, 50, 20, 8)).toBeCloseTo(10);
  });

  it("no se da vuelta cuando el radio se pasa de la caja", () => {
    // Radio 999 sobre una caja de 20×20: se recorta al semieje.
    expect(sdRoundBox(100, 100, 100, 100, 10, 10, 999)).toBeCloseTo(-10);
  });
});

describe("sdCapsule", () => {
  it("vale 0 a distancia r del eje", () => {
    expect(sdCapsule(50, 13, 20, 10, 80, 10, 3)).toBeCloseTo(0);
  });

  it("mide contra el extremo cuando el punto se pasa del segmento", () => {
    expect(sdCapsule(90, 10, 20, 10, 80, 10, 3)).toBeCloseTo(7);
  });
});

describe("smin", () => {
  it("con k = 0 es el mínimo duro", () => {
    expect(smin(3, 7, 0)).toBe(3);
    expect(smin(-2, 5, 0)).toBe(-2);
  });

  it("baja el campo k/4 donde las dos distancias empatan", () => {
    for (const k of [4, 12, 26, 59, 75]) {
      expect(smin(10, 10, k)).toBeCloseTo(10 - k / 4);
    }
  });

  it("no altera nada lejos de la otra forma", () => {
    // Separadas por mucho más que k, el mínimo suave es el mínimo.
    expect(smin(2, 500, 26)).toBeCloseTo(2);
  });
});

describe("alcance y bulto", () => {
  it("son k/2 y k/4", () => {
    expect(sminReach(59)).toBe(29.5);
    expect(sminBulge(59)).toBe(14.75);
  });

  /**
   * El número que reemplaza al `1.72·σ` del filtro. En el punto medio del hueco
   * la distancia a cada forma es g/2, así que el campo vale `g/2 − k/4` y se
   * llena justo cuando `g = k/2`.
   */
  it("el campo se anula en el medio de un hueco de exactamente k/2", () => {
    const k = 40;
    const gap = sminReach(k); // 20
    // Cara derecha de la primera en x = 50; la segunda se centra para que su
    // cara izquierda caiga en x = 50 + gap.
    const shapes: Shape[] = [
      { kind: "box", cx: 0, cy: 0, hw: 50, hh: 50, r: 0 },
      { kind: "box", cx: 100 + gap, cy: 0, hw: 50, hh: 50, r: 0 },
    ];
    const field = new Field(shapes, k);
    expect(field.eval(50 + gap / 2, 0)).toBeCloseTo(0);
  });

  it("un hueco más chico que k/2 queda unido, uno más grande no", () => {
    const k = 40;
    const at = (gap: number) =>
      new Field(
        [
          { kind: "box", cx: 0, cy: 0, hw: 50, hh: 50, r: 0 },
          { kind: "box", cx: 100 + gap, cy: 0, hw: 50, hh: 50, r: 0 },
        ],
        k,
      ).eval(50 + gap / 2, 0);

    expect(at(sminReach(k) - 4)).toBeLessThan(0);
    expect(at(sminReach(k) + 4)).toBeGreaterThan(0);
  });

  /**
   * Con el hueco de 10 px que usa la app hoy, hasta el k por defecto alcanza —
   * y por eso las cinco constantes del cuello dibujado de `AgentsSurface`
   * (grosor 26→10, piso 6, corte 140, penetración 9/7) dejan de hacer falta.
   */
  it("k = 26 ya cruza el hueco de 10 px de la app", () => {
    expect(sminReach(26)).toBeGreaterThan(10);
  });
});

describe("Field.bounds", () => {
  it("deja de margen el bulto y nada más", () => {
    const k = 40;
    const f = new Field([{ kind: "box", cx: 0, cy: 0, hw: 10, hh: 20, r: 4 }], k);
    const b = f.bounds();
    expect(b.minX).toBeCloseTo(-10 - sminBulge(k));
    expect(b.maxY).toBeCloseTo(20 + sminBulge(k));
  });

  it("envuelve todas las formas", () => {
    const f = new Field(
      [
        { kind: "box", cx: 0, cy: 0, hw: 10, hh: 10, r: 0 },
        { kind: "capsule", ax: 100, ay: 0, bx: 140, by: 0, r: 5 },
      ],
      0,
    );
    const b = f.bounds();
    expect(b.minX).toBeCloseTo(-10);
    expect(b.maxX).toBeCloseTo(145);
  });

  it("sin formas devuelve una caja vacía en vez de infinitos", () => {
    expect(new Field([], 10).bounds()).toEqual({
      minX: 0,
      minY: 0,
      maxX: 0,
      maxY: 0,
    });
  });
});
