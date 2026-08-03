import { describe, expect, it } from "vitest";
import { fieldToPath } from "./contour";
import { Field, type Shape } from "./sdf";

/** La escena real: la barra de la pill con la consola encima. */
function scene(gap = 10): Shape[] {
  const pillW = 176;
  const pillH = 40;
  const bubW = 580;
  const bubH = 520;
  const pillCy = 800;
  return [
    { kind: "box", cx: 700, cy: pillCy, hw: pillW / 2, hh: pillH / 2, r: pillH / 2 },
    {
      kind: "box",
      cx: 700,
      cy: pillCy - pillH / 2 - gap - bubH / 2,
      hw: bubW / 2,
      hh: bubH / 2,
      r: 26,
    },
  ];
}

describe("descarte por bloques", () => {
  /**
   * La prueba que sostiene la optimización.
   *
   * El descarte no es una aproximación: como el gradiente del campo nunca pasa
   * de 1, `|d| > lado·1.71` en el centro de un bloque garantiza que no hay
   * cruce ni ahí ni en las celdas que lo tocan. Si el contorno saliera aunque
   * sea un poco distinto, la garantía sería falsa.
   */
  it("da exactamente el mismo contorno que muestrear todo", () => {
    for (const k of [26, 59, 75]) {
      const field = new Field(scene(), k);
      const completo = fieldToPath(field, { cell: 4, narrowBand: false });
      const banda = fieldToPath(field, { cell: 4 });
      expect(banda.d).toBe(completo.d);
    }
  });

  it("evalúa una fracción de la grilla", () => {
    const banda = fieldToPath(new Field(scene(), 59), { cell: 4 });
    expect(banda.evals).toBeLessThan(banda.samples / 2);
  });
});

describe("fieldToPath", () => {
  it("une en un solo trazo lo que está dentro del alcance", () => {
    // Hueco 10, alcance k/2 = 29.5: tiene que salir un contorno solo.
    const path = fieldToPath(new Field(scene(10), 59), { cell: 4 });
    expect(path.d.match(/M /g)).toHaveLength(1);
  });

  it("deja dos trazos cuando el hueco pasa el alcance", () => {
    // Hueco 60 contra un alcance de 13.
    const path = fieldToPath(new Field(scene(60), 26), { cell: 4 });
    expect(path.d.match(/M /g)).toHaveLength(2);
  });

  it("el contorno pasa por la geometría pedida, sin engordar", () => {
    // Una caja sola: sin otra forma cerca, el `smin` no la toca.
    const field = new Field(
      [{ kind: "box", cx: 0, cy: 0, hw: 100, hh: 60, r: 12 }],
      40,
    );
    const path = fieldToPath(field, { cell: 1, smooth: 0 });
    const xs = [...path.d.matchAll(/[ML] (-?[\d.]+) (-?[\d.]+)/g)].map((m) =>
      Number(m[1]),
    );
    // Tolerancia de una celda: es la resolución con la que se trazó.
    expect(Math.min(...xs)).toBeGreaterThan(-101);
    expect(Math.max(...xs)).toBeLessThan(101);
  });

  it("agranda la celda en vez de devolver nada cuando la escena es enorme", () => {
    // 4000×4000 a celda 1 pide 16 millones de muestras, muy por encima del tope.
    const field = new Field(
      [{ kind: "box", cx: 0, cy: 0, hw: 2000, hh: 2000, r: 40 }],
      10,
    );
    const path = fieldToPath(field, { cell: 1 });
    expect(path.cell).toBeGreaterThan(1);
    expect(path.d.length).toBeGreaterThan(0);
  });

  it("sin formas no explota", () => {
    const path = fieldToPath(new Field([], 20), { cell: 4 });
    expect(path.d).toBe("");
  });
});
