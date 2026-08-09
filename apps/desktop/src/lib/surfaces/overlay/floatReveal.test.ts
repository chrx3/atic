import { describe, expect, it } from "vitest";
import { isCloseRevealPhase, separateAxisProp } from "./floatReveal";

describe("separateAxisProp", () => {
  it("left/right animan left", () => {
    expect(separateAxisProp("left")).toBe("left");
    expect(separateAxisProp("right")).toBe("left");
  });

  it("top/bottom animan top", () => {
    expect(separateAxisProp("top")).toBe("top");
    expect(separateAxisProp("bottom")).toBe("top");
  });
});

describe("isCloseRevealPhase", () => {
  it("reconoce fases de cierre", () => {
    expect(isCloseRevealPhase("tuck")).toBe(true);
    expect(isCloseRevealPhase("approach")).toBe(true);
    expect(isCloseRevealPhase("shrink")).toBe(true);
    expect(isCloseRevealPhase("ready")).toBe(false);
    expect(isCloseRevealPhase("expand")).toBe(false);
  });
});
