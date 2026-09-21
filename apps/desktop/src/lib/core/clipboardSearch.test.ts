import { describe, expect, it } from "vitest";

import { clipboardItemMatches, fuzzyMatch } from "./clipboardSearch";

describe("fuzzyMatch", () => {
  it("matchea substring normalizado (sin acentos ni mayúsculas)", () => {
    expect(fuzzyMatch("La función se rompió", "funcion")).toBe(true);
    expect(fuzzyMatch("Así mantenemos un seguimiento", "MANTENEMOS")).toBe(true);
  });

  it("NO matchea subsecuencias en textos largos (regresión: 'mantenemos')", () => {
    // El párrafo no contiene "mantenemos", pero sus letras aparecen salteadas
    // en orden — el fallback viejo lo daba por bueno y llenaba la lista de
    // resultados ajenos a la búsqueda.
    const parrafo =
      "Debemos idealmente mostrar la respuesta del stream de los agentes o " +
      "mostrar algún icono de animación diciendo trabajando o contestando, " +
      "pero que también nazca de la piel.";
    expect(fuzzyMatch(parrafo, "mantenemos")).toBe(false);
  });

  it("multi-token: todas las palabras presentes, en cualquier orden", () => {
    expect(fuzzyMatch("historial del clipboard en la isla", "isla clipboard")).toBe(
      true,
    );
    expect(fuzzyMatch("historial del clipboard en la isla", "isla terminal")).toBe(
      false,
    );
  });

  it("query vacía matchea; texto vacío con query, no", () => {
    expect(fuzzyMatch("lo que sea", "   ")).toBe(true);
    expect(fuzzyMatch("", "algo")).toBe(false);
  });
});

describe("clipboardItemMatches", () => {
  it("busca en preview y en text completo", () => {
    expect(clipboardItemMatches({ preview: "hola", text: null }, "hola")).toBe(true);
    expect(
      clipboardItemMatches({ preview: "hola", text: "contenido completo" }, "completo"),
    ).toBe(true);
  });

  it("no inventa coincidencias que solo existen como subsecuencia", () => {
    expect(
      clipboardItemMatches(
        { preview: "hola", text: "otra cosa distinta" },
        "mantenemos",
      ),
    ).toBe(false);
  });
});
