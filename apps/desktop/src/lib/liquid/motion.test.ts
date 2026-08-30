import { describe, expect, it } from "vitest";
import { boxShape, pillShape } from "./geometry";
import { clusterParts, rigidShift, unionAabb } from "./motion";
import type { Shape } from "./sdf";

const box = (x: number, y: number, w = 40, h = 40): Shape =>
  pillShape({ x, y, w, h });

describe("rigidShift", () => {
  it("devuelve el delta común cuando todo se traslada igual", () => {
    const a = [box(10, 20), box(80, 20, 100, 40)];
    const b = [box(40, 50), box(110, 50, 100, 40)];
    expect(rigidShift(a, b)).toEqual({ dx: 30, dy: 30 });
  });

  it("es null si las formas se mueven distinto (rueda del picker)", () => {
    const a = [box(10, 20), box(10, 80), box(200, 50, 100, 40)];
    const b = [box(12, 10), box(12, 90), box(200, 50, 100, 40)];
    expect(rigidShift(a, b)).toBeNull();
  });

  it("acepta cápsulas que se mueven juntas", () => {
    const a: Shape[] = [
      { kind: "capsule", ax: 0, ay: 0, bx: 10, by: 0, r: 4 },
    ];
    const b: Shape[] = [
      { kind: "capsule", ax: 5, ay: 8, bx: 15, by: 8, r: 4 },
    ];
    expect(rigidShift(a, b)).toEqual({ dx: 5, dy: 8 });
  });
});

describe("clusterParts", () => {
  it("deja juntas las superficies dentro del alcance", () => {
    const pill = [box(0, 0)];
    const auth = [box(45, 0)];
    const islands = clusterParts({ pill, "agent-auth": auth }, 10);
    expect(islands).toHaveLength(1);
    expect(islands[0]?.id).toBe("agent-auth+pill");
    expect(islands[0]?.shapes).toHaveLength(2);
  });

  it("separa las que están más lejos que el alcance", () => {
    const pill = [box(0, 0)];
    const launcher = [box(400, 0, 200, 40)];
    const islands = clusterParts({ pill, launcher }, 10);
    expect(islands.map((i) => i.id).sort()).toEqual(["launcher", "pill"]);
  });

  it("ignora partes vacías", () => {
    expect(clusterParts({ pill: [box(0, 0)], agents: [] }, 10)).toEqual([
      { id: "pill", shapes: [box(0, 0)] },
    ]);
  });

  it("no funde floats de distinto grupo aunque se solapen", () => {
    const clipboard = [box(0, 0, 200, 300)];
    const agents = [box(20, 20, 200, 300)];
    const islands = clusterParts(
      { clipboard, agents },
      10,
      { clipboard: "clipboard", agents: "agents" },
    );
    expect(islands.map((i) => i.id).sort()).toEqual(["agents", "clipboard"]);
  });

  it("sí funde un float con la pill si ambos van al hub", () => {
    const pill = [box(0, 0)];
    const clipboard = [box(20, 0)];
    const islands = clusterParts(
      { pill, clipboard },
      10,
      { pill: "hub", clipboard: "hub" },
    );
    expect(islands).toHaveLength(1);
    expect(islands[0]?.id).toBe("clipboard+pill");
  });
});

describe("unionAabb", () => {
  it("envuelve las formas", () => {
    const b = unionAabb([
      boxShape({ x: 10, y: 20, w: 40, h: 40 }, 8),
      boxShape({ x: 80, y: 20, w: 20, h: 10 }, 4),
    ]);
    expect(b).toEqual({ minX: 10, minY: 20, maxX: 100, maxY: 60 });
  });
});
