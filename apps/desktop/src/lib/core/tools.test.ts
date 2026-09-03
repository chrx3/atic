import { describe, expect, it } from "vitest";
import { TOOLS, WHEEL_TOOLS } from "./tools";

/** Lo que vive solo en su atajo y no se gana un gajo de la rueda. */
const SHORTCUT_ONLY = ["launcher", "dictation"] as const;

describe("WHEEL_TOOLS", () => {
  it("deja fuera lo que es puro atajo, pero sigue existiendo como tool", () => {
    for (const id of SHORTCUT_ONLY) {
      expect(WHEEL_TOOLS.some((tool) => tool.id === id)).toBe(false);
      // Sigue en `TOOLS`: la ventana principal la muestra y su atajo la ejecuta.
      expect(TOOLS.some((tool) => tool.id === id)).toBe(true);
    }
  });

  it("conserva el resto de las tools visibles, en orden", () => {
    expect(WHEEL_TOOLS.map((tool) => tool.id)).toEqual(
      TOOLS.filter(
        (tool) => !SHORTCUT_ONLY.includes(tool.id as (typeof SHORTCUT_ONLY)[number]),
      ).map((tool) => tool.id),
    );
  });
});
