import { describe, expect, it } from "vitest";
import { TOOL_ICONS } from "../icons";
import { en } from "./i18n/en";
import { es } from "./i18n/es";
import { TOOLS } from "./tools";

/**
 * El contrato del registro de herramientas.
 *
 * Una herramienta se define en varios lugares a la vez —registro, iconos,
 * copy en dos idiomas— y hasta ahora nada verificaba que coincidieran: faltaba
 * una clave en `en.ts` y `pnpm check` pasaba igual. Estos tests recorren el
 * registro y fallan por el nombre de la tool que quedó a medias, no por un
 * índice.
 */

/** Aplana un subárbol de diccionario a claves con puntos (`a.b.c`). */
function claves(arbol: unknown, prefijo = ""): string[] {
  if (typeof arbol !== "object" || arbol === null) return [prefijo];
  return Object.entries(arbol).flatMap(([clave, valor]) =>
    claves(valor, prefijo ? `${prefijo}.${clave}` : clave),
  );
}

function subtool(dict: { tools: unknown }, id: string): Record<string, unknown> {
  return (dict.tools as Record<string, Record<string, unknown>>)[id];
}

describe("registro de herramientas", () => {
  it("no repite ids", () => {
    const ids = TOOLS.map((tool) => tool.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("cada tool tiene icono", () => {
    for (const tool of TOOLS) {
      expect(TOOL_ICONS[tool.id], `falta el icono de «${tool.id}»`).toBeTruthy();
    }
  });

  it("cada tool tiene copy completo en español y en inglés", () => {
    for (const tool of TOOLS) {
      for (const [idioma, dict] of [
        ["es", es],
        ["en", en],
      ] as const) {
        for (const clave of ["label", "short", "blurb", "actionLabel"]) {
          const valor = subtool(dict, tool.id)?.[clave];
          expect(
            typeof valor === "string" && valor.trim().length > 0,
            `falta tools.${tool.id}.${clave} en ${idioma}`,
          ).toBe(true);
        }
      }
    }
  });

  it("los dos idiomas tienen exactamente las mismas claves de tools", () => {
    const soloEs = claves(es.tools).filter(
      (clave) => !claves(en.tools).includes(clave),
    );
    const soloEn = claves(en.tools).filter(
      (clave) => !claves(es.tools).includes(clave),
    );
    expect({ soloEs, soloEn }).toEqual({ soloEs: [], soloEn: [] });
  });
});
