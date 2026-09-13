import { describe, expect, it } from "vitest";
import type { MacPermissions } from "$ipc/permissions";
import { hasMissingPermissions, microphoneAction, permissionRows } from "./model";

const concedido: MacPermissions = {
  microphone: "authorized",
  screen_recording: true,
  accessibility: true,
};

describe("hasMissingPermissions", () => {
  it("todo concedido no falta nada", () => {
    expect(hasMissingPermissions(concedido)).toBe(false);
  });

  it("basta que falte uno", () => {
    expect(hasMissingPermissions({ ...concedido, accessibility: false })).toBe(true);
    expect(hasMissingPermissions({ ...concedido, screen_recording: false })).toBe(true);
    expect(hasMissingPermissions({ ...concedido, microphone: "denied" })).toBe(true);
  });
});

describe("microphoneAction", () => {
  it("sin decidir se puede pedir", () => {
    expect(microphoneAction("not_determined")).toBe("allow");
  });

  it("denegado o restringido manda a Ajustes", () => {
    expect(microphoneAction("denied")).toBe("settings");
    expect(microphoneAction("restricted")).toBe("settings");
  });

  it("concedido no muestra acción", () => {
    expect(microphoneAction("authorized")).toBe("none");
  });
});

describe("permissionRows", () => {
  it("ordena micrófono, pantalla y accesibilidad", () => {
    expect(permissionRows(concedido).map((row) => row.kind)).toEqual([
      "microphone",
      "screen_recording",
      "accessibility",
    ]);
  });

  it("solo ofrece botón en lo pendiente", () => {
    const rows = permissionRows({ ...concedido, screen_recording: false });
    expect(rows[0].action).toBe("none");
    expect(rows[1]).toMatchObject({ granted: false, action: "allow" });
    expect(rows[2].action).toBe("none");
  });

  it("accesibilidad siempre se resuelve en Ajustes", () => {
    const rows = permissionRows({ ...concedido, accessibility: false });
    expect(rows[2]).toMatchObject({ granted: false, action: "settings" });
  });
});
