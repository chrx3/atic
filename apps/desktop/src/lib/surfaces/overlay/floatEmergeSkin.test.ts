import { describe, expect, it } from "vitest";
import { rectKey, sameRect } from "./floatEmergeSkinMath";

describe("floatEmergeSkin idle keys", () => {
  it("sameRect tolerates subpixel jitter under epsilon", () => {
    const a = { x: 100, y: 200, w: 400, h: 48 };
    const b = { x: 100.4, y: 200.3, w: 400.2, h: 48.1 };
    expect(sameRect(a, b)).toBe(true);
    expect(sameRect(a, { ...a, x: 101 })).toBe(false);
  });

  it("rectKey quantizes to half-pixels", () => {
    expect(rectKey({ x: 10.2, y: 20.7, w: 100.1, h: 48.4 })).toBe(
      rectKey({ x: 10, y: 20.5, w: 100, h: 48.5 }),
    );
    expect(rectKey({ x: 10, y: 20, w: 100, h: 48 })).not.toBe(
      rectKey({ x: 11, y: 20, w: 100, h: 48 }),
    );
  });
});
