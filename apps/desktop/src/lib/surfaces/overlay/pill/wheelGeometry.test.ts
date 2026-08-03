import { describe, expect, it } from "vitest";
import { nodeAngle, nodePosition, separators, wedgeClip } from "./wheelGeometry";

const SQUARE = { width: 240, height: 240 };

describe("nodeAngle", () => {
  it("el primer nodo apunta arriba", () => {
    expect(nodeAngle(0, 6)).toBeCloseTo(-Math.PI / 2);
  });

  it("reparte la vuelta en partes iguales", () => {
    const step = nodeAngle(1, 6) - nodeAngle(0, 6);
    expect(step).toBeCloseTo((Math.PI * 2) / 6);
  });

  it("da la vuelta completa", () => {
    expect(nodeAngle(6, 6)).toBeCloseTo(nodeAngle(0, 6) + Math.PI * 2);
  });
});

describe("nodePosition", () => {
  it("el primero cae arriba del centro, a un radio", () => {
    const p = nodePosition(0, 6, SQUARE, 80);
    expect(p.x).toBeCloseTo(120);
    expect(p.y).toBeCloseTo(40);
  });

  it("con cuatro nodos, el tercero cae abajo", () => {
    const p = nodePosition(2, 4, SQUARE, 80);
    expect(p.x).toBeCloseTo(120);
    expect(p.y).toBeCloseTo(200);
  });

  it("todos quedan a la misma distancia del centro", () => {
    for (let i = 0; i < 6; i++) {
      const p = nodePosition(i, 6, SQUARE, 80);
      expect(Math.hypot(p.x - 120, p.y - 120)).toBeCloseTo(80);
    }
  });
});

describe("wedgeClip", () => {
  it("arranca en el centro y tiene los cinco puntos del arco", () => {
    const clip = wedgeClip(0, 6, SQUARE);
    expect(clip.startsWith("polygon(120.0px 120.0px,")).toBe(true);
    expect(clip.split(",").length).toBe(6);
  });

  /**
   * El radio es la diagonal completa a propósito: el polígono TIENE que
   * desbordar el rectángulo para que las fronteras de adentro sean los rayos
   * exactos del sector.
   */
  it("desborda el rectángulo", () => {
    const clip = wedgeClip(0, 6, SQUARE);
    const xs = [...clip.matchAll(/(-?\d+\.\d)px (-?\d+\.\d)px/g)].map((m) => [
      Number(m[1]),
      Number(m[2]),
    ]);
    const outside = xs.some(
      ([x, y]) => x < 0 || y < 0 || x > SQUARE.width || y > SQUARE.height,
    );
    expect(outside).toBe(true);
  });
});

describe("separators", () => {
  it("hay uno por gajo y van en el medio entre dos nodos", () => {
    const seps = separators(6, SQUARE, 20);
    expect(seps).toHaveLength(6);
    // A mitad de camino entre el nodo 0 (-90°) y el nodo 1 (-30°).
    expect(seps[0].deg).toBeCloseTo(-60);
  });

  /**
   * El largo depende del ángulo: un rayo hacia una esquina recorre más que uno
   * hacia el medio de un lado. Con un largo fijo, las diagonales quedaban
   * cortas.
   */
  it("cada uno llega hasta el borde según su propio ángulo", () => {
    const seps = separators(4, SQUARE, 0);
    // Con cuatro gajos los separadores caen en las diagonales exactas: el
    // recorrido hasta el borde es la media diagonal, no el medio lado.
    expect(seps[0].len).toBeCloseTo(120 * Math.SQRT2, 1);
  });

  it("descuenta el núcleo y nunca da largo negativo", () => {
    const conNucleo = separators(4, SQUARE, 30);
    expect(conNucleo[0].len).toBeCloseTo(120 * Math.SQRT2 - 30, 1);
    // Un núcleo más grande que el lienzo no puede producir una línea al revés.
    expect(separators(4, SQUARE, 9999)[0].len).toBe(0);
  });

  it("no se va al infinito en las cardinales", () => {
    // Con dos gajos los separadores caen justo en horizontal, donde `cos` es 0
    // y la cuenta del otro eje se indefine si no se la protege.
    for (const sep of separators(2, SQUARE, 0)) {
      expect(Number.isFinite(sep.len)).toBe(true);
    }
  });
});
