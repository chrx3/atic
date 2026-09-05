import { describe, expect, it } from "vitest";
import { createDockExpand, reuseDockedFrame, shouldResizeLauncher, rememberedSetupWidth } from "./dockExpand";

describe("createDockExpand", () => {
  it("call dispara el bind vigente", () => {
    const dock = createDockExpand();
    let called = 0;
    const unbind = dock.bind(() => {
      called += 1;
    });
    dock.call();
    expect(called).toBe(1);
    unbind();
  });

  it("unbind deja de llamar", () => {
    const dock = createDockExpand();
    let called = 0;
    const unbind = dock.bind(() => {
      called += 1;
    });
    unbind();
    dock.call();
    expect(called).toBe(0);
  });

  it("sin bind, call no tira", () => {
    expect(() => createDockExpand().call()).not.toThrow();
  });
});

describe("reuseDockedFrame", () => {
  it("solo si está achicado y el globo sigue vivo", () => {
    expect(
      reuseDockedFrame({ minimized: true, alive: true, hasAnchor: true }),
    ).toBe(true);
    expect(
      reuseDockedFrame({ minimized: true, alive: false, hasAnchor: true }),
    ).toBe(false);
    expect(
      reuseDockedFrame({ minimized: false, alive: true, hasAnchor: true }),
    ).toBe(false);
  });
});

describe("shouldResizeLauncher", () => {
  it("anima al pasar de selector a consola", () => {
    expect(
      shouldResizeLauncher({
        current: "setup",
        next: "console",
        height: 184,
        minConsoleHeight: 340,
      }),
    ).toBe(true);
  });

  it("agranda si la consola quedó con el marco del selector", () => {
    expect(
      shouldResizeLauncher({
        current: "console",
        next: "console",
        height: 184,
        minConsoleHeight: 340,
      }),
    ).toBe(true);
  });

  it("no anima si la consola ya tiene tamaño", () => {
    expect(
      shouldResizeLauncher({
        current: "console",
        next: "console",
        height: 520,
        minConsoleHeight: 340,
      }),
    ).toBe(false);
  });
});

describe("rememberedSetupWidth", () => {
  it("descarta la semilla de nacimiento", () => {
    expect(rememberedSetupWidth(40, 400)).toBe(400);
  });

  it("conserva un selector ya usable", () => {
    expect(rememberedSetupWidth(480, 400)).toBe(480);
  });

  it("ignora valores rotos", () => {
    expect(rememberedSetupWidth(Number.NaN, 400)).toBe(400);
  });
});
