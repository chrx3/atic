import { describe, expect, it } from "vitest";
import { createDockExpand, reuseDockedFrame } from "./dockExpand";

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
