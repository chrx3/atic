/**
 * Que las constantes gemelas no se separen en silencio.
 *
 * El test lee el Rust de verdad. Es feo —busca una `const` con una expresión
 * regular sobre código fuente— y es a propósito: la alternativa es un comentario
 * pidiendo por favor que se cambien las dos, y eso ya falló antes acá.
 *
 * Se importa con `?raw` en vez de leer el disco: es nativo de Vite, así que no
 * hace falta `@types/node` ni resolver rutas a mano, y si el archivo se mueve
 * falla al compilar en lugar de fallar al correr.
 *
 * Si Rust cambia un número, esto se pone rojo con los dos valores a la vista.
 * Si Rust RENOMBRA la constante, también, porque la búsqueda no encuentra nada:
 * ese caso es igual de peligroso y merece la misma señal.
 */

import { describe, expect, it } from "vitest";
import bridgeRs from "../../../../src-tauri/src/agents/bridge.rs?raw";
import floatingRs from "../../../../src-tauri/src/floating.rs?raw";
import { BUBBLE_MIN_H, BUBBLE_MIN_W, MARGIN } from "./contract";

function rustConst(source: string, name: string): number {
  // `const NOMBRE: i32 = 8;` — el tipo se acepta cualquiera para no atarse a i32.
  const match = new RegExp(`const\\s+${name}\\s*:\\s*\\w+\\s*=\\s*(-?\\d+)`).exec(
    source,
  );
  if (!match) throw new Error(`No se encontró \`const ${name}\`. ¿Lo renombraron?`);
  return Number(match[1]);
}

describe("constantes gemelas con Rust", () => {
  it("el margen contra el borde del monitor es el mismo", () => {
    expect(rustConst(floatingRs, "MARGIN")).toBe(MARGIN);
  });

  it("el tamaño mínimo de la consola es el mismo", () => {
    expect(rustConst(bridgeRs, "BUBBLE_MIN_W")).toBe(BUBBLE_MIN_W);
    expect(rustConst(bridgeRs, "BUBBLE_MIN_H")).toBe(BUBBLE_MIN_H);
  });
});
