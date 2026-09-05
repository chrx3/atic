import { describe, expect, it } from "vitest";
import {
  outerSides,
  snapFrame,
  snapKindAt,
  snapTarget,
  workUnderCursor,
} from "./floatSnap";

const work = { x: 0, y: 0, w: 1000, h: 800 };
const at = (x: number, y: number) => ({ x, y });

describe("snapKindAt", () => {
  it("el cursor contra el techo maximiza, no un cuarto", () => {
    expect(snapKindAt(at(400, 8), work)).toBe("max");
  });

  it("el cursor contra un canto lateral parte a la mitad", () => {
    expect(snapKindAt(at(4, 300), work)).toBe("left");
    expect(snapKindAt(at(996, 300), work)).toBe("right");
  });

  it("abajo parte a la mitad inferior", () => {
    expect(snapKindAt(at(400, 792), work)).toBe("bottom");
  });

  it("las esquinas ganan al canto", () => {
    expect(snapKindAt(at(4, 4), work)).toBe("tl");
    expect(snapKindAt(at(996, 4), work)).toBe("tr");
    expect(snapKindAt(at(4, 792), work)).toBe("bl");
    expect(snapKindAt(at(996, 792), work)).toBe("br");
  });

  it("lejos de los cantos no engancha", () => {
    expect(snapKindAt(at(400, 300), work)).toBeNull();
  });
});

describe("snapFrame", () => {
  it("max llena el área con el margen", () => {
    expect(snapFrame("max", work, 12)).toEqual({
      x: 12,
      y: 12,
      w: 976,
      h: 776,
    });
  });

  it("left y right se parten sin solaparse", () => {
    const left = snapFrame("left", work, 12);
    const right = snapFrame("right", work, 12);
    expect(left.x).toBe(12);
    expect(right.x + right.w).toBe(988);
    expect(left.x + left.w).toBeLessThan(right.x);
    expect(left.h).toBe(right.h);
  });

  it("un cuarto cabe en su esquina", () => {
    const tl = snapFrame("tl", work, 12);
    const br = snapFrame("br", work, 12);
    expect(tl.x).toBe(12);
    expect(tl.y).toBe(12);
    expect(br.x + br.w).toBe(988);
    expect(br.y + br.h).toBe(788);
  });
});

describe("workUnderCursor", () => {
  it("elige el monitor que contiene el cursor", () => {
    const areas = [
      { x: 0, y: 0, w: 1000, h: 800 },
      { x: 1000, y: 0, w: 1200, h: 800, work: { x: 1000, y: 0, w: 1200, h: 760 } },
    ];
    expect(workUnderCursor({ x: 1100, y: 40 }, areas)).toEqual({
      x: 1000,
      y: 0,
      w: 1200,
      h: 760,
    });
  });
});

describe("outerSides / snapTarget", () => {
  const left = { x: 0, y: 0, w: 1000, h: 800 };
  const right = { x: 1000, y: 0, w: 1200, h: 800 };

  it("la junta entre monitores es un canto interior", () => {
    expect(outerSides(left, [left, right])).toEqual({
      left: true,
      right: false,
      top: true,
      bottom: true,
    });
    expect(outerSides(right, [left, right])).toEqual({
      left: false,
      right: true,
      top: true,
      bottom: true,
    });
  });

  it("a caballo de dos pantallas, con el cursor lejos de la junta, no engancha", () => {
    expect(snapTarget(at(920, 200), [left, right])).toBeNull();
  });

  it("el cursor contra la junta desde la izquierda parte a la derecha", () => {
    expect(snapTarget(at(990, 200), [left, right])).toEqual({
      kind: "right",
      work: left,
    });
  });

  it("el cursor contra la junta desde la derecha parte a la izquierda", () => {
    expect(snapTarget(at(1010, 200), [left, right])).toEqual({
      kind: "left",
      work: right,
    });
  });

  it("colgando del canto izquierdo del monitor izquierdo parte a la izquierda", () => {
    expect(snapTarget(at(-4, 200), [left, right])).toEqual({
      kind: "left",
      work: left,
    });
  });

  it("colgando del canto derecho del monitor derecho parte a la derecha", () => {
    expect(snapTarget(at(2204, 200), [left, right])).toEqual({
      kind: "right",
      work: right,
    });
  });

  it("el marco contra un canto no engancha si el cursor está lejos", () => {
    expect(snapTarget(at(500, 400), [left, right])).toBeNull();
  });

  it("el cursor contra el techo maximiza aunque el marco no lo toque", () => {
    expect(snapTarget(at(500, 8), [left, right])).toEqual({
      kind: "max",
      work: left,
    });
  });
});
