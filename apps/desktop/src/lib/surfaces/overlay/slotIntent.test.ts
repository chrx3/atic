import { describe, expect, it } from "vitest";
import {
  enqueueActivate,
  isCursorAnchored,
  isScreenCentered,
  pillToCursorMovePx,
  shouldCommitShow,
  shouldReturnHomeAfterClose,
  slotIntent,
  spatialDismissTargets,
} from "./slotIntent";

describe("slotIntent", () => {
  it("abre si no hay nada, o si el pedido es otra tool", () => {
    expect(slotIntent("clipboard", false)).toBe("show");
    expect(slotIntent("clipboard", false)).toBe("show");
    expect(slotIntent("snippets", false)).toBe("show");
  });

  it("cierra si esa misma tool espacial ya está abierta y el cursor no se movió", () => {
    expect(slotIntent("clipboard", true)).toBe("close");
    expect(slotIntent("clipboard", true, 12)).toBe("close");
    expect(slotIntent("snippets", true, 0)).toBe("close");
    expect(slotIntent("launcher", true, 200)).toBe("close");
  });

  it("reubica clipboard / textos si ya están abiertos y el cursor se fue", () => {
    expect(slotIntent("clipboard", true, 48)).toBe("relocate");
    expect(slotIntent("snippets", true, 120)).toBe("relocate");
  });

  it("dictado / grabar / captura no se cierran por estar “abiertos”", () => {
    expect(slotIntent("dictation", true)).toBe("show");
    expect(slotIntent("meetings", true)).toBe("show");
    expect(slotIntent("captures", true)).toBe("show");
  });

  it("force (rueda): apuntar un gajo abre, nunca cierra ni reubica", () => {
    const force = { force: true };
    // Los tres casos que SÍ cambian de intención sin el flag.
    expect(slotIntent("clipboard", true, 0, 48, force)).toBe("show");
    expect(slotIntent("clipboard", true, 200, 48, force)).toBe("show");
    expect(slotIntent("launcher", true, 0, 48, force)).toBe("show");
    expect(slotIntent("snippets", true, 120, 48, force)).toBe("show");
  });

  it("force: false explícito se comporta como el atajo", () => {
    expect(slotIntent("clipboard", true, 0, 48, { force: false })).toBe("close");
    expect(slotIntent("clipboard", true, 90, 48, { force: false })).toBe("relocate");
  });
});

describe("isCursorAnchored / pillToCursorMovePx", () => {
  it("solo clipboard y textos se anclan al cursor", () => {
    expect(isCursorAnchored("clipboard")).toBe(true);
    expect(isCursorAnchored("snippets")).toBe(true);
    expect(isCursorAnchored("launcher")).toBe(false);
    expect(isCursorAnchored("agents")).toBe(false);
    expect(isScreenCentered("launcher")).toBe(true);
    expect(isScreenCentered("agents")).toBe(true);
    expect(isScreenCentered("clipboard")).toBe(false);
    expect(isScreenCentered("snippets")).toBe(false);
  });

  it("mide el vuelo de la pill al cursor, o 0 si no hay puntero", () => {
    expect(
      pillToCursorMovePx({ x: 100, y: 100 }, { w: 40, h: 40 }, { x: 220, y: 120 }),
    ).toBe(100);
    expect(pillToCursorMovePx({ x: 0, y: 0 }, { w: 40, h: 40 }, null)).toBe(0);
  });
});

describe("spatialDismissTargets", () => {
  it("al mostrar clipboard cierra el resto", () => {
    expect(spatialDismissTargets("clipboard", {})).toEqual([
      "snippets",
      "agents",
      "launcher",
    ]);
  });

  it("el pin conserva esa tool; el launcher siempre cede", () => {
    expect(
      spatialDismissTargets("clipboard", {
        snippets: true,
        agents: true,
      }),
    ).toEqual(["launcher"]);
  });

  it("sin keep, cierra todas las espaciales no fijadas", () => {
    expect(spatialDismissTargets(undefined, { clipboard: true })).toEqual([
      "snippets",
      "agents",
      "launcher",
    ]);
  });
});

describe("enqueueActivate", () => {
  it("en idle arranca ya", () => {
    expect(enqueueActivate(false, "clipboard")).toEqual({
      start: true,
      pending: null,
    });
  });

  it("si está ocupado, el último pedido gana", () => {
    expect(enqueueActivate(true, "clipboard")).toEqual({
      start: false,
      pending: "clipboard",
    });
    expect(enqueueActivate(true, "snippets")).toEqual({
      start: false,
      pending: "snippets",
    });
  });

  // El payload es el pedido entero, no el id: un pedido de la rueda encolado
  // detrás de un atajo tiene que conservar su `force` al desencolarse.
  it("la cola guarda el pedido completo, force incluido", () => {
    expect(enqueueActivate(true, { id: "clipboard", force: true })).toEqual({
      start: false,
      pending: { id: "clipboard", force: true },
    });
    expect(enqueueActivate(false, { id: "snippets", force: false })).toEqual({
      start: true,
      pending: null,
    });
  });
});

describe("commit / return-home", () => {
  it("no abre A si ya pidieron B", () => {
    expect(shouldCommitShow(null)).toBe(true);
    expect(shouldCommitShow("snippets")).toBe(false);
  });

  it("no vuelve a casa si hay un switch pendiente", () => {
    expect(shouldReturnHomeAfterClose(null)).toBe(true);
    expect(shouldReturnHomeAfterClose("clipboard")).toBe(false);
  });

  it("también con pedidos completos en la cola", () => {
    expect(shouldCommitShow({ id: "clipboard", force: true })).toBe(false);
    expect(shouldReturnHomeAfterClose({ id: "snippets", force: false })).toBe(false);
  });
});
