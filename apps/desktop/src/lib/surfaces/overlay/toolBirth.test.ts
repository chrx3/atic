import { describe, expect, it } from "vitest";
import {
  birthAtCursor,
  captureToolBirth,
  createDetachHandoffGate,
  DETACH_HANDOFF_PLACE_FRAMES,
  nextDetachHandoff,
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

describe("nextDetachHandoff", () => {
  const rest = { x: 100, y: 200, w: 300, h: 150 };
  const oldFrame = { x: 900, y: 700, w: 680, h: 520 };

  it("sin marco pide reintentar (el traspaso saldría al vacío)", () => {
    expect(nextDetachHandoff(createDetachHandoffGate(), null, rest)).toBe(false);
  });

  it("con reposo pendiente y marco viejo pide reintentar", () => {
    const gate = createDetachHandoffGate();
    expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(false);
    // Sigue pidiendo mientras el evento de Rust no coloque en la cara.
    expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(false);
  });

  it("acepta cuando el marco ya es el reposo de la cara", () => {
    const gate = createDetachHandoffGate();
    expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(false);
    expect(nextDetachHandoff(gate, { ...rest }, rest)).toBe(true);
  });

  it("sin reposo acepta el marco que haya (despegue sin rect medido)", () => {
    expect(nextDetachHandoff(createDetachHandoffGate(), oldFrame, null)).toBe(
      true,
    );
  });

  it("pasado el tope acepta igual para no colgar el gesto", () => {
    const gate = createDetachHandoffGate();
    for (let i = 0; i < DETACH_HANDOFF_PLACE_FRAMES; i++) {
      expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(false);
    }
    expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(true);
  });

  it("aceptar resetea la espera del gesto siguiente", () => {
    const gate = createDetachHandoffGate();
    expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(false);
    expect(nextDetachHandoff(gate, { ...rest }, rest)).toBe(true);
    expect(nextDetachHandoff(gate, oldFrame, rest)).toBe(false);
  });
});
