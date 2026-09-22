import { describe, expect, it } from "vitest";

import { isPeekTool, peekFor } from "./peekTools";

describe("peekFor", () => {
  it("las herramientas con vistazo lo abren con su texto de respaldo", () => {
    expect(peekFor("system", "Sistema — recursos")).toEqual({
      tool: "system",
      fallback: "Sistema — recursos",
    });
    expect(peekFor("clipboard", "x")?.tool).toBe("clipboard");
    expect(peekFor("agents", "x")?.tool).toBe("agents");
    expect(peekFor("captures", "x")?.tool).toBe("captures");
    expect(peekFor("color", "x")?.tool).toBe("color");
    expect(peekFor("snippets", "x")?.tool).toBe("snippets");
  });

  it("el resto se queda con el tooltip: sin vistazo", () => {
    expect(peekFor("board", "Pizarra")).toBeNull();
    expect(peekFor("more", "Más")).toBeNull();
    expect(isPeekTool("customize")).toBe(false);
    expect(isPeekTool(null)).toBe(false);
  });
});
