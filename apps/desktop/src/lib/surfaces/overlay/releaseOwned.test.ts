import { describe, expect, it } from "vitest";
import { releaseOwned } from "./releaseOwned";

describe("releaseOwned", () => {
  it("no borra si otro dueño tomó el id", () => {
    const map = new Map<string, string>([["agents", "float"]]);
    releaseOwned(map, "agents", "chip");
    expect(map.get("agents")).toBe("float");
  });

  it("borra si sigue siendo el dueño", () => {
    const map = new Map<string, string>([["agents", "chip"]]);
    releaseOwned(map, "agents", "chip");
    expect(map.has("agents")).toBe(false);
  });
});
