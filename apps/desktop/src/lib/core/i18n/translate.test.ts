import { describe, expect, it } from "vitest";
import { en } from "./en";
import { es } from "./es";
import { parseLocale, translate } from "./translate";

describe("translate", () => {
  it("usa español por defecto", () => {
    expect(parseLocale(undefined)).toBe("es");
    expect(parseLocale("es")).toBe("es");
    expect(parseLocale("en")).toBe("en");
  });

  it("system sigue el idioma del navegador", () => {
    const expected = (globalThis.navigator?.language ?? "")
      .toLowerCase()
      .startsWith("en")
      ? "en"
      : "es";
    expect(parseLocale("system")).toBe(expected);
  });

  it("interpola variables", () => {
    expect(translate("es", "about.availableTitle", { version: "1.2" })).toContain("1.2");
    expect(translate("en", "about.availableTitle", { version: "1.2" })).toContain("1.2");
  });

  it("cae a español si falta la clave en inglés", () => {
    expect(translate("en", "tray.show")).toBeTruthy();
    expect(translate("en", "tray.show")).not.toBe("tray.show");
  });

  it("no muestra la clave cruda si existe en español", () => {
    expect(translate("es", "tray.quit")).toBe("Salir");
  });

  it("mantiene las mismas hojas en es y en", () => {
    function leaves(tree: Record<string, unknown>, prefix = ""): string[] {
      const out: string[] = [];
      for (const [key, value] of Object.entries(tree)) {
        const path = prefix ? `${prefix}.${key}` : key;
        if (typeof value === "string") out.push(path);
        else if (value && typeof value === "object") {
          out.push(...leaves(value as Record<string, unknown>, path));
        }
      }
      return out;
    }
    const esKeys = leaves(es as unknown as Record<string, unknown>).sort();
    const enKeys = leaves(en as unknown as Record<string, unknown>).sort();
    expect(enKeys).toEqual(esKeys);
  });
});
