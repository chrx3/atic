import { describe, expect, it } from "vitest";
import {
  arrowHead,
  beginShape,
  clampToImage,
  commit,
  emptyState,
  extendShape,
  isDegenerate,
  normalizeRect,
  redo,
  strokeWidth,
  toImagePoint,
  toolForKey,
  undo,
  type FreeShape,
  type SpanShape,
} from "./annotateModel";

const RED = "#ff3b30";

describe("strokeWidth", () => {
  it("en la imagen de referencia el nivel 2 mide 4 px", () => {
    expect(strokeWidth(2, 1280)).toBe(4);
  });

  it("escala con el ancho: un 4K no se anota con pelos", () => {
    expect(strokeWidth(2, 3840)).toBe(12);
  });

  it("no encoge por debajo de la referencia", () => {
    // Una captura chica no tiene por qué dar trazos de menos de un píxel.
    expect(strokeWidth(1, 320)).toBe(2);
  });
});

describe("extendShape", () => {
  it("ignora los puntos que no se movieron lo suficiente", () => {
    const pen = beginShape("pen", RED, 4, { x: 0, y: 0 });
    const same = extendShape(pen, { x: 1, y: 0 });
    expect(same).toBe(pen);
  });

  it("acumula los que sí", () => {
    const pen = beginShape("pen", RED, 4, { x: 0, y: 0 });
    const moved = extendShape(pen, { x: 10, y: 0 }) as FreeShape;
    expect(moved.points).toHaveLength(2);
    // Nueva referencia: el componente la tiene en `$state` y necesita ver el
    // cambio para redibujar.
    expect(moved).not.toBe(pen);
  });

  it("en las formas de dos puntas solo mueve el extremo", () => {
    const arrow = beginShape("arrow", RED, 4, { x: 0, y: 0 });
    const moved = extendShape(arrow, { x: 30, y: 40 }) as SpanShape;
    expect(moved.from).toEqual({ x: 0, y: 0 });
    expect(moved.to).toEqual({ x: 30, y: 40 });
  });
});

describe("isDegenerate", () => {
  it("un clic suelto no deja forma", () => {
    expect(isDegenerate(beginShape("rect", RED, 4, { x: 5, y: 5 }))).toBe(true);
    expect(isDegenerate(beginShape("pen", RED, 4, { x: 5, y: 5 }))).toBe(true);
  });

  it("un arrastre de verdad sí", () => {
    const arrow = extendShape(beginShape("arrow", RED, 4, { x: 0, y: 0 }), {
      x: 40,
      y: 0,
    });
    expect(isDegenerate(arrow)).toBe(false);
  });
});

describe("commit / undo / redo", () => {
  const arrow = extendShape(beginShape("arrow", RED, 4, { x: 0, y: 0 }), {
    x: 40,
    y: 0,
  });
  const rect = extendShape(beginShape("rect", RED, 4, { x: 0, y: 0 }), {
    x: 40,
    y: 40,
  });

  it("descarta lo degenerado sin tocar el estado", () => {
    const state = emptyState();
    expect(commit(state, beginShape("rect", RED, 4, { x: 1, y: 1 }))).toBe(state);
  });

  it("deshacer y rehacer devuelven lo mismo", () => {
    const one = commit(emptyState(), arrow);
    const two = commit(one, rect);
    const back = undo(two);
    expect(back.shapes).toHaveLength(1);
    expect(redo(back).shapes).toEqual(two.shapes);
  });

  it("dibujar algo nuevo borra el rehacer", () => {
    const back = undo(commit(emptyState(), arrow));
    expect(back.undone).toHaveLength(1);
    expect(commit(back, rect).undone).toHaveLength(0);
  });

  it("deshacer sin nada que deshacer no rompe", () => {
    const state = emptyState();
    expect(undo(state)).toBe(state);
    expect(redo(state)).toBe(state);
  });
});

describe("toImagePoint", () => {
  const rect = { left: 100, top: 50, width: 640, height: 360 };
  const natural = { width: 1280, height: 720 };

  it("convierte a píxeles de la imagen aunque se muestre a la mitad", () => {
    expect(toImagePoint({ x: 100, y: 50 }, rect, natural)).toEqual({ x: 0, y: 0 });
    expect(toImagePoint({ x: 420, y: 230 }, rect, natural)).toEqual({ x: 640, y: 360 });
  });

  it("sin rectángulo medible cae en la escala natural", () => {
    const zero = { left: 0, top: 0, width: 0, height: 0 };
    expect(toImagePoint({ x: 12, y: 8 }, zero, natural)).toEqual({ x: 12, y: 8 });
  });
});

describe("clampToImage", () => {
  it("no deja dibujar fuera del lienzo", () => {
    const natural = { width: 100, height: 100 };
    expect(clampToImage({ x: -20, y: 130 }, natural)).toEqual({ x: 0, y: 100 });
  });
});

describe("arrowHead", () => {
  it("la punta apunta al destino y el cuello queda detrás", () => {
    const head = arrowHead({ x: 0, y: 0 }, { x: 100, y: 0 }, 4);
    expect(head.tip).toEqual({ x: 100, y: 0 });
    expect(head.neck.x).toBeLessThan(100);
    expect(head.neck.y).toBeCloseTo(0);
    // Las barbas, simétricas respecto del eje de la flecha.
    expect(head.left.y).toBeCloseTo(-head.right.y);
  });

  it("nunca es más larga que la flecha", () => {
    const head = arrowHead({ x: 0, y: 0 }, { x: 6, y: 0 }, 40);
    expect(head.neck.x).toBeGreaterThanOrEqual(0);
  });

  it("una flecha de largo cero no divide por cero", () => {
    const head = arrowHead({ x: 7, y: 7 }, { x: 7, y: 7 }, 4);
    expect(head.tip).toEqual({ x: 7, y: 7 });
    expect(Number.isNaN(head.left.x)).toBe(false);
  });
});

describe("normalizeRect", () => {
  it("da lo mismo arrastrar hacia arriba que hacia abajo", () => {
    const a = normalizeRect({ x: 10, y: 10 }, { x: 40, y: 50 });
    const b = normalizeRect({ x: 40, y: 50 }, { x: 10, y: 10 });
    expect(a).toEqual(b);
    expect(a).toEqual({ x: 10, y: 10, w: 30, h: 40 });
  });
});

describe("toolForKey", () => {
  it("número y letra llevan a la misma herramienta", () => {
    expect(toolForKey("2")).toBe("arrow");
    expect(toolForKey("F")).toBe("arrow");
  });

  it("lo que no es del editor no elige nada", () => {
    expect(toolForKey("z")).toBeNull();
    expect(toolForKey("Escape")).toBeNull();
  });
});
