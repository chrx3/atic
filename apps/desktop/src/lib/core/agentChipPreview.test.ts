import { describe, expect, it } from "vitest";
import { clipChipPreview } from "./agentChipPreview";

describe("clipChipPreview", () => {
  it("recorta igual que el readyLabel de la pill", () => {
    expect(clipChipPreview("hola")).toBe("hola");
    expect(clipChipPreview("  hola\nmundo")).toBe("hola");
    expect(clipChipPreview("")).toBe("Listo");
    expect(clipChipPreview(null)).toBe("Listo");
    expect(clipChipPreview(undefined)).toBe("Listo");
    expect(clipChipPreview("a".repeat(28))).toBe("a".repeat(28));
    expect(clipChipPreview("a".repeat(29))).toBe(`${"a".repeat(27)}…`);
  });
});
