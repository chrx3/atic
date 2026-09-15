import { describe, expect, it } from "vitest";

import { nativePointerAlreadyHandled, resolveSynthEcho } from "./syntheticPointer";

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

describe("resolveSynthEcho", () => {
  const echo = { x: 100, y: 200 };

  it("sin eco pendiente no consume nada", () => {
    expect(
      resolveSynthEcho(null, { type: "click", clientX: 100, clientY: 200 }),
    ).toEqual({
      echo: null,
      consume: false,
    });
  });

  it("consume el click nativo del mismo punto y cierra la espera", () => {
    expect(
      resolveSynthEcho(echo, { type: "click", clientX: 102, clientY: 197 }),
    ).toEqual({
      echo: null,
      consume: true,
    });
  });

  it("consume el up pero deja abierta la espera del click", () => {
    const up = resolveSynthEcho(echo, {
      type: "pointerup",
      clientX: 100,
      clientY: 200,
    });
    expect(up.consume).toBe(true);
    expect(up.echo).toEqual(echo);
    // El click que cierra el mismo gesto todavía se tiene que tragar.
    expect(
      resolveSynthEcho(up.echo, { type: "click", clientX: 100, clientY: 200 }),
    ).toEqual({
      echo: null,
      consume: true,
    });
  });

  it("también consume el mouseup del mismo gesto", () => {
    expect(
      resolveSynthEcho(echo, { type: "mouseup", clientX: 99, clientY: 201 }).consume,
    ).toBe(true);
  });

  it("un down en el mismo punto es un gesto nuevo: pasa y cierra la espera", () => {
    expect(
      resolveSynthEcho(echo, { type: "pointerdown", clientX: 100, clientY: 200 }),
    ).toEqual({ echo: null, consume: false });
    expect(
      resolveSynthEcho(echo, { type: "mousedown", clientX: 100, clientY: 200 }),
    ).toEqual({ echo: null, consume: false });
  });

  it("un clic en otro lado es del usuario: pasa y cierra la espera", () => {
    expect(
      resolveSynthEcho(echo, { type: "click", clientX: 300, clientY: 200 }),
    ).toEqual({
      echo: null,
      consume: false,
    });
  });
});
