import { describe, expect, it } from "vitest";
import { treeToKeys, treeToSessions } from "./consoleTransfer";

describe("treeToSessions", () => {
  it("traduce hojas por clave a hojas por sesión", () => {
    const tree = {
      kind: "split" as const,
      direction: "right" as const,
      ratio: 0.6,
      first: { kind: "leaf" as const, key: "t1" },
      second: { kind: "leaf" as const, key: "t2" },
    };
    expect(
      treeToSessions(tree, (key) => (key === "t1" ? "s1" : key === "t2" ? "s2" : null)),
    ).toEqual({
      kind: "split",
      direction: "right",
      ratio: 0.6,
      first: { kind: "leaf", session: "s1" },
      second: { kind: "leaf", session: "s2" },
    });
  });

  it("poda la hoja a medio abrir y colapsa el split", () => {
    const tree = {
      kind: "split" as const,
      direction: "down" as const,
      first: { kind: "leaf" as const, key: "t1" },
      second: { kind: "leaf" as const, key: "t9" },
    };
    expect(treeToSessions(tree, (key) => (key === "t1" ? "s1" : null))).toEqual({
      kind: "leaf",
      session: "s1",
    });
  });

  it("sin hojas vivas no hay árbol", () => {
    expect(treeToSessions({ kind: "leaf", key: "t9" }, () => null)).toBeNull();
    expect(treeToSessions(null, () => "s1")).toBeNull();
  });
});

describe("treeToKeys", () => {
  it("traduce hojas por sesión a claves nuevas y poda lo no adoptado", () => {
    const tree = {
      kind: "split" as const,
      direction: "right" as const,
      first: { kind: "leaf" as const, session: "s1" },
      second: {
        kind: "split" as const,
        direction: "down" as const,
        first: { kind: "leaf" as const, session: "s2" },
        second: { kind: "leaf" as const, session: "s3" },
      },
    };
    // s3 no cupo (tope de pestañas): su rama se poda, el resto conserva forma.
    expect(
      treeToKeys(tree, (s) => (s === "s1" ? "t1" : s === "s2" ? "t2" : null)),
    ).toEqual({
      kind: "split",
      direction: "right",
      first: { kind: "leaf", key: "t1" },
      second: { kind: "leaf", key: "t2" },
    });
  });
});
