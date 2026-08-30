import { describe, expect, it } from "vitest";
import { pillLayout, pillStripPage } from "./pillTools";
import { WHEEL_TOOLS } from "./tools";

const ids = (tools: { id: string }[]) => tools.map((tool) => tool.id);

describe("pillLayout", () => {
  it("sin configurar, muestra todas en el anillo", () => {
    const layout = pillLayout();
    expect(ids(layout.ring)).toEqual(ids([...WHEEL_TOOLS]));
    expect(layout.more).toEqual([]);
    expect(layout.hidden).toEqual([]);
  });

  it("respeta el orden elegido, no el del catálogo", () => {
    const layout = pillLayout(["captures", "meetings"]);
    expect(ids(layout.ring)).toEqual(["captures", "meetings"]);
  });

  it("lo que no está en ninguna lista queda oculto", () => {
    const layout = pillLayout(["meetings"], ["board"]);
    expect(ids(layout.ring)).toEqual(["meetings"]);
    expect(ids(layout.more)).toEqual(["board"]);
    expect(ids(layout.hidden)).not.toContain("meetings");
    expect(ids(layout.hidden)).not.toContain("board");
    expect(ids(layout.hidden)).toContain("clipboard");
  });

  it("descarta ids que no son herramientas de la pill", () => {
    const layout = pillLayout(["meetings", "launcher", "inventada"], ["board"]);
    expect(ids(layout.ring)).toEqual(["meetings"]);
  });

  it("no repite una herramienta ni dentro de una lista ni entre las dos", () => {
    const layout = pillLayout(
      ["meetings", "meetings", "board"],
      ["board", "clipboard"],
    );
    expect(ids(layout.ring)).toEqual(["meetings", "board"]);
    expect(ids(layout.more)).toEqual(["clipboard"]);
  });

  it("con el anillo vacío, el submenú sube: nadie queda a dos pasos de todo", () => {
    const layout = pillLayout([], ["board", "clipboard"]);
    expect(ids(layout.ring)).toEqual(["board", "clipboard"]);
    expect(layout.more).toEqual([]);
  });

  it("una config sin ningún id válido vuelve al catálogo entero", () => {
    const layout = pillLayout(["inventada"], ["tampoco"]);
    expect(ids(layout.ring)).toEqual(ids([...WHEEL_TOOLS]));
  });
});

describe("pillStripPage", () => {
  it("la tira repite el escalón de la rueda: las fijas y la puerta a «Más»", () => {
    const layout = pillLayout(["meetings", "captures"], ["board"]);
    expect(pillStripPage(layout)).toEqual(["meetings", "captures", "more"]);
  });

  it("sin herramientas en «Más» la puerta sigue: Ventana vive detrás", () => {
    const layout = pillLayout(["meetings", "captures"], []);
    expect(pillStripPage(layout)).toEqual(["meetings", "captures", "more"]);
  });

  it("en el canto Ventana va en el primer paso, y sin «Más» si no hay submenú", () => {
    const layout = pillLayout(["meetings", "captures"], []);
    expect(pillStripPage(layout, "ring", { windowOnFirst: true })).toEqual([
      "meetings",
      "captures",
      "window",
    ]);
  });

  it("en el canto «Más» queda si hay submenú; Ventana no se duplica detrás", () => {
    const layout = pillLayout(["meetings"], ["board"]);
    expect(pillStripPage(layout, "ring", { windowOnFirst: true })).toEqual([
      "meetings",
      "more",
      "window",
    ]);
    expect(pillStripPage(layout, "more", { windowOnFirst: true })).toEqual([
      "back",
      "board",
    ]);
  });

  it("el segundo paso arranca con la vuelta, porque la tira no tiene núcleo", () => {
    const layout = pillLayout(["meetings"], ["board", "clipboard"]);
    expect(pillStripPage(layout, "more")).toEqual([
      "back",
      "board",
      "clipboard",
      "window",
    ]);
  });

  it("Ventana es el último del segundo paso aunque «Más» esté vacío", () => {
    const layout = pillLayout(["meetings"], []);
    expect(pillStripPage(layout, "more")).toEqual(["back", "window"]);
  });

  it("lo oculto tampoco aparece en la tira", () => {
    const layout = pillLayout(["meetings"], []);
    expect(pillStripPage(layout)).not.toContain("clipboard");
  });
});
