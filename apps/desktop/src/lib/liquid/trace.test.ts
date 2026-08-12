import { describe, expect, it } from "vitest";
import { pillShape } from "./geometry";
import { PathTracer } from "./trace";

const opts = { blend: 20, cell: 8, smooth: 0 };

describe("PathTracer", () => {
  it("no remuestrea cuando las formas solo se trasladan", () => {
    const tracer = new PathTracer();
    const a = [pillShape({ x: 100, y: 200, w: 40, h: 40 })];
    const first = tracer.next(a, opts);
    expect(first.path.d).not.toBe("");
    expect(first.ms).toBeGreaterThanOrEqual(0);

    const b = [pillShape({ x: 140, y: 230, w: 40, h: 40 })];
    const moved = tracer.next(b, opts);
    expect(moved.path.d).toBe(first.path.d);
    expect(moved.tx).toBe(40);
    expect(moved.ty).toBe(30);
    expect(moved.ms).toBe(0);
    expect(moved.path.evals).toBe(first.path.evals);
  });

  it("remuestrea si cambia el tamaño", () => {
    const tracer = new PathTracer();
    const first = tracer.next(
      [pillShape({ x: 0, y: 0, w: 40, h: 40 })],
      opts,
    );
    const grown = tracer.next(
      [pillShape({ x: 0, y: 0, w: 80, h: 40 })],
      opts,
    );
    expect(grown.tx).toBe(0);
    expect(grown.ty).toBe(0);
    expect(grown.path.d).not.toBe(first.path.d);
  });

  it("conserva el path fino al cambiar la celda si hay traslado", () => {
    const tracer = new PathTracer();
    const first = tracer.next(
      [pillShape({ x: 0, y: 0, w: 40, h: 40 })],
      opts,
    );
    const moved = tracer.next(
      [pillShape({ x: 12, y: 0, w: 40, h: 40 })],
      { ...opts, cell: 12, smooth: 0 },
    );
    expect(moved.path.d).toBe(first.path.d);
    expect(moved.tx).toBe(12);
  });
});
