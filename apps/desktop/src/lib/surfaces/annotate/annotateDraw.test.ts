import { describe, expect, it } from "vitest";
import { drawShape, type DrawTarget } from "./annotateDraw";
import { HIGHLIGHT_FACTOR, type Shape } from "./annotateModel";

/**
 * Un canvas de mentira que apunta lo que le piden.
 *
 * No se rasteriza nada: lo que hay que fijar son las decisiones —que el
 * resaltador sea translúcido y multiplique, que la flecha cierre su punta, que
 * la línea no llegue hasta el vértice— y todas se ven en la lista de llamadas.
 */
function fakeCtx(): DrawTarget & { ops: string[] } {
  const ops: string[] = [];
  const record =
    (name: string) =>
    (...args: unknown[]) => {
      ops.push(`${name}(${args.map((a) => String(a)).join(",")})`);
    };
  return {
    ops,
    save: record("save"),
    restore: record("restore"),
    beginPath: record("beginPath"),
    closePath: record("closePath"),
    moveTo: record("moveTo"),
    lineTo: record("lineTo"),
    quadraticCurveTo: record("quadraticCurveTo"),
    ellipse: record("ellipse"),
    rect: record("rect"),
    stroke: record("stroke"),
    fill: record("fill"),
    strokeStyle: "",
    fillStyle: "",
    lineWidth: 0,
    lineCap: "butt",
    lineJoin: "miter",
    globalAlpha: 1,
    globalCompositeOperation: "source-over",
  };
}

const pen: Shape = {
  kind: "pen",
  color: "#ff3b30",
  width: 4,
  points: [
    { x: 0, y: 0 },
    { x: 10, y: 10 },
    { x: 20, y: 0 },
  ],
};

describe("drawShape", () => {
  it("siempre deja el contexto como lo encontró", () => {
    const ctx = fakeCtx();
    drawShape(ctx, pen);
    expect(ctx.ops[0]).toBe("save()");
    expect(ctx.ops.at(-1)).toBe("restore()");
  });

  it("el trazo libre se suaviza con curvas, no con palitos", () => {
    const ctx = fakeCtx();
    drawShape(ctx, pen);
    expect(ctx.ops.some((op) => op.startsWith("quadraticCurveTo"))).toBe(true);
  });

  it("un solo punto es un toque, no un trazo vacío", () => {
    const ctx = fakeCtx();
    drawShape(ctx, { ...pen, points: [{ x: 5, y: 5 }] });
    expect(ctx.ops).toContain("moveTo(5,5)");
    expect(ctx.ops).toContain("lineTo(5,5)");
    expect(ctx.ops).toContain("stroke()");
  });

  it("el resaltador multiplica, es translúcido y va más ancho", () => {
    const ctx = fakeCtx();
    drawShape(ctx, { ...pen, kind: "highlight" });
    expect(ctx.globalCompositeOperation).toBe("multiply");
    expect(ctx.globalAlpha).toBeLessThan(1);
    expect(ctx.lineWidth).toBe(pen.width * HIGHLIGHT_FACTOR);
  });

  it("la flecha cierra la punta y la rellena", () => {
    const ctx = fakeCtx();
    drawShape(ctx, {
      kind: "arrow",
      color: "#ff3b30",
      width: 4,
      from: { x: 0, y: 0 },
      to: { x: 100, y: 0 },
    });
    expect(ctx.ops).toContain("closePath()");
    expect(ctx.ops).toContain("fill()");
    // La línea muere en el cuello: si llegara a la punta, el remate redondo
    // asomaría por delante del triángulo.
    expect(ctx.ops).not.toContain("lineTo(100,0)");
  });

  it("la elipse se inscribe en el arrastre", () => {
    const ctx = fakeCtx();
    drawShape(ctx, {
      kind: "ellipse",
      color: "#ff3b30",
      width: 4,
      from: { x: 10, y: 20 },
      to: { x: 110, y: 220 },
    });
    expect(ctx.ops.some((op) => op.startsWith("ellipse(60,120,50,100"))).toBe(true);
  });
});
