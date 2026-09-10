import { describe, expect, it } from "vitest";
import { MARGEN, PAGINA_H, PAGINA_W } from "./flipLayout";
import {
  altoListaResumen,
  altoTextoResumen,
  bloquesDeResumen,
  etiquetaResumen,
  RESUMEN_W,
} from "./flipSummary";

function contador() {
  const ids = { n: 0 };
  return () => `id-${++ids.n}`;
}

const RESUMEN = [
  "# Resumen",
  "Hablamos del lanzamiento.",
  "",
  "## Próximos pasos",
  "- [ ] Escribir el mail",
  "- [x] Agendar demo",
  "",
  "## Temas",
  "- Precio",
  "- Fecha",
].join("\n");

describe("flipSummary", () => {
  it("nombra el resumen con día-mes-año corto", () => {
    expect(etiquetaResumen("2026-08-20T12:00:00.000Z")).toBe(
      "Resumen reunión 20-08-26",
    );
  });

  it("las tareas salen como checklist y el relato como texto", () => {
    const bloques = bloquesDeResumen(RESUMEN, "Resumen reunión 20-08-26", contador());
    const textos = bloques.filter((b) => b.kind === "text");
    const listas = bloques.filter((b) => b.kind === "check");
    expect(textos[0]?.kind === "text" && textos[0].body).toBe(
      "Resumen reunión 20-08-26",
    );
    expect(
      textos.some((b) => b.kind === "text" && b.body.includes("lanzamiento")),
    ).toBe(true);
    expect(listas).toHaveLength(1);
    if (listas[0]?.kind !== "check") throw new Error("esperaba checklist");
    expect(listas[0].items.map((i) => [i.text, i.done])).toEqual([
      ["Escribir el mail", false],
      ["Agendar demo", true],
    ]);
    expect(textos.some((b) => b.kind === "text" && b.body.includes("Precio"))).toBe(
      true,
    );
  });

  it("el alto de la lista incluye padding, ítems y el botón de añadir", () => {
    // .objeto.lista padding (18+6) + 28 por fila + .mas (4+28)
    expect(altoListaResumen(1)).toBe(24 + 28 + 32);
    expect(altoListaResumen(6)).toBe(24 + 6 * 28 + 32);
    expect(altoListaResumen(0)).toBe(0);
  });

  it("el alto del texto arranca en el mínimo y crece por línea", () => {
    const corto = altoTextoResumen("Acuerdos");
    expect(corto).toBeGreaterThanOrEqual(48);
    expect(altoTextoResumen("Acuerdos\ncon dos líneas")).toBeGreaterThan(corto);
  });

  it("el texto declara el alto de las líneas que de verdad ocupa", () => {
    // 560 de ancho y padding de 24: ~74 caracteres por línea
    const cuerpo = "x".repeat(74 * 3 + 10);
    expect(altoTextoResumen(cuerpo)).toBeGreaterThanOrEqual(48 + 3 * 13.5 * 1.5 - 0.01);
  });

  it("no parte el resumen a otra celda cuando entra entero", () => {
    // Soltado abajo del todo: tiene que subir al techo, no escaparse de celda.
    const bloques = bloquesDeResumen(RESUMEN, "Resumen reunión 20-08-26", contador(), {
      x: 600,
      y: PAGINA_H - 40,
    });
    const celdas = new Set(bloques.map((b) => Math.floor((b.x ?? 0) / PAGINA_W)));
    expect(celdas).toEqual(new Set([0]));
    expect(bloques[0]?.y).toBe(MARGEN);
  });

  it("se alinea al margen de la celda donde lo soltaste", () => {
    const enPrimera = bloquesDeResumen(RESUMEN, "R", contador(), { x: 600, y: 40 });
    expect(enPrimera[0]?.x).toBe(MARGEN);
    const enSegunda = bloquesDeResumen(RESUMEN, "R", contador(), {
      x: PAGINA_W + 200,
      y: 40,
    });
    expect(enSegunda[0]?.x).toBe(PAGINA_W + MARGEN);
    expect(enSegunda[0]?.y).toBe(40);
  });

  it("baja a la celda de al lado cuando el resumen no entra en una", () => {
    const muchos = Array.from(
      // 14 secciones de dos líneas: entran diez por celda, así que son dos.
      { length: 14 },
      (_, i) => `## Tema ${i}\nTexto de la sección ${i}`,
    ).join("\n\n");
    const bloques = bloquesDeResumen(muchos, "R", contador(), { x: 600, y: 40 });
    const enSegunda = bloques.filter((b) => Math.floor((b.x ?? 0) / PAGINA_W) === 1);
    expect(enSegunda.length).toBeGreaterThan(0);
    // la segunda celda arranca en su margen, no a mitad de camino
    expect(enSegunda[0]?.x).toBe(PAGINA_W + MARGEN);
    expect(enSegunda[0]?.y).toBe(MARGEN);
    // y nunca se va más allá de la celda contigua
    expect(bloques.every((b) => (b.x ?? 0) < PAGINA_W * 2)).toBe(true);
    expect(new Set(bloques.map((b) => Math.floor((b.x ?? 0) / PAGINA_W)))).toEqual(
      new Set([0, 1]),
    );
  });

  it("usa columna de documento, más ancha que una nota suelta", () => {
    const bloques = bloquesDeResumen(RESUMEN, "R", contador());
    expect(bloques.every((b) => (b.w ?? 0) === RESUMEN_W)).toBe(true);
  });
});
