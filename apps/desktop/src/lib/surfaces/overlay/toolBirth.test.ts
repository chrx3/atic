import { describe, expect, it } from "vitest";
import {
  birthAtCursor,
  captureToolBirth,
  notifyToolResting,
  toolBirth,
  waitToolResting,
} from "./toolBirth";

describe("toolBirth", () => {
  it("centra el disco de nacimiento en el cursor", () => {
    expect(birthAtCursor({ x: 400, y: 300 }, { w: 40, h: 40 })).toEqual({
      x: 380,
      y: 280,
      w: 40,
      h: 40,
    });
  });

  it("guarda una copia del rect, no la referencia", () => {
    const rect = { x: 10, y: 20, w: 40, h: 40 };
    captureToolBirth(rect);
    rect.x = 99;
    expect(toolBirth()).toEqual({ x: 10, y: 20, w: 40, h: 40 });
  });

  it("waitToolResting resuelve al notificar, o al timeout", async () => {
    captureToolBirth({ x: 0, y: 0, w: 1, h: 1 });
    const pending = waitToolResting(200);
    notifyToolResting();
    await pending;
    await expect(waitToolResting(10)).resolves.toBeUndefined();
  });
});
