import { describe, expect, it } from "vitest";
import { TOOLS, WHEEL_TOOLS } from "./tools";

/**
 * Lo que vive solo en su atajo y no se gana un gajo de la rueda.
 *
 * La lista no se repite acá: sale del propio registro (`shortcutOnly`). Antes
 * era una constante paralela, así que el test no podía fallar cuando el código
 * sumaba o sacaba una tool de este tipo.
 */
const SOLO_ATAJO = TOOLS.filter((tool) => tool.shortcutOnly);

describe("WHEEL_TOOLS", () => {
  it("deja fuera lo que es puro atajo, pero sigue existiendo como tool", () => {
    expect(SOLO_ATAJO.map((tool) => tool.id).sort()).toEqual(["dictation", "launcher"]);
    for (const tool of SOLO_ATAJO) {
      expect(WHEEL_TOOLS.some((item) => item.id === tool.id)).toBe(false);
      // Sigue en `TOOLS`: la ventana principal la muestra y su atajo la ejecuta.
      expect(TOOLS.some((item) => item.id === tool.id)).toBe(true);
    }
  });

  it("conserva el resto de las tools visibles, en orden", () => {
    expect(WHEEL_TOOLS.map((tool) => tool.id)).toEqual(
      TOOLS.filter((tool) => !tool.shortcutOnly).map((tool) => tool.id),
    );
  });
});
