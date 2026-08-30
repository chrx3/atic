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

  it("quita markdown para no mostrar asteriscos crudos", () => {
    expect(clipChipPreview("Soy **Muse Spark 1.2.0**")).toBe("Soy Muse Spark 1.2.0");
    expect(clipChipPreview("ok `0545fca3` listo")).toBe("ok 0545fca3 listo");
  });
});
