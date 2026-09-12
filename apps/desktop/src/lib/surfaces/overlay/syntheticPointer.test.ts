import { describe, expect, it } from "vitest";

import { nativePointerAlreadyHandled } from "./syntheticPointer";

describe("nativePointerAlreadyHandled", () => {
  it("no omite si nunca llegó un clic nativo", () => {
    expect(nativePointerAlreadyHandled(0, 50)).toBe(false);
  });

  it("omite el sintético si el DOM ya recibió el flanco", () => {
    expect(nativePointerAlreadyHandled(100, 140)).toBe(true);
    expect(nativePointerAlreadyHandled(100, 179)).toBe(true);
  });

  it("vuelve a sintetizar cuando el nativo ya es viejo", () => {
    expect(nativePointerAlreadyHandled(100, 181)).toBe(false);
  });
});
