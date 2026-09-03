import { describe, expect, it } from "vitest";
import {
  outerSides,
  snapFrame,
  snapKindAt,
  snapTarget,
  workUnderCursor,
} from "./floatSnap";

const work = { x: 0, y: 0, w: 1000, h: 800 };
const frame = (x: number, y: number, w = 200, h = 160) => ({ x, y, w, h });

describe("snapKindAt", () => {
  it("el marco contra el techo maximiza, no un cuarto", () => {
    expect(snapKindAt(frame(400, 8), work)).toBe("max");
  });

  it("el marco contra un canto lateral parte a la mitad", () => {
    expect(snapKindAt(frame(4, 300), work)).toBe("left");
    expect(snapKindAt(frame(800, 300), work)).toBe("right");
  });

  it("abajo parte a la mitad inferior", () => {
    expect(snapKindAt(frame(400, 650), work)).toBe("bottom");
  });

  it("las esquinas ganan al canto", () => {
    expect(snapKindAt(frame(4, 4), work)).toBe("tl");
    expect(snapKindAt(frame(800, 4), work)).toBe("tr");
    expect(snapKindAt(frame(4, 650), work)).toBe("bl");
    expect(snapKindAt(frame(800, 650), work)).toBe("br");
  });

  it("lejos de los cantos no engancha, aunque el cursor esté al borde", () => {
    expect(snapKindAt(frame(400, 300), work)).toBeNull();
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

  it("a caballo de dos pantallas, lejos de la junta, no engancha", () => {
    expect(snapTarget(frame(850, 200, 300, 200), [left, right])).toBeNull();
  });

  it("el marco contra la junta desde la izquierda parte a la derecha", () => {
    expect(snapTarget(frame(800, 200, 200, 200), [left, right])).toEqual({
      kind: "right",
      work: left,
    });
  });

  it("el marco contra la junta desde la derecha parte a la izquierda", () => {
    expect(snapTarget(frame(1000, 200, 200, 200), [left, right])).toEqual({
      kind: "left",
      work: right,
    });
  });

  it("colgando del canto izquierdo del monitor izquierdo parte a la izquierda", () => {
    expect(snapTarget(frame(-12, 200, 300, 200), [left, right])).toEqual({
      kind: "left",
      work: left,
    });
  });

  it("colgando del canto derecho del monitor derecho parte a la derecha", () => {
    expect(snapTarget(frame(2050, 200, 300, 200), [left, right])).toEqual({
      kind: "right",
      work: right,
    });
  });
});
