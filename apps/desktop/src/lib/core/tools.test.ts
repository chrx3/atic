import { describe, expect, it } from "vitest";
import { TOOLS, WHEEL_TOOLS } from "./tools";

describe("WHEEL_TOOLS", () => {
  it("no incluye el launcher: Spotlight vive fuera de la rueda", () => {
    expect(WHEEL_TOOLS.some((tool) => tool.id === "launcher")).toBe(false);
    expect(TOOLS.some((tool) => tool.id === "launcher")).toBe(true);
  });

  it("conserva el resto de las tools visibles", () => {
    expect(WHEEL_TOOLS.map((tool) => tool.id)).toEqual(
      TOOLS.filter((tool) => tool.id !== "launcher").map((tool) => tool.id),
    );
  });
});
