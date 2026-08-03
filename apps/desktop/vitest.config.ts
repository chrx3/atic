import { defineConfig } from "vitest/config";

/**
 * Config propia, sin el plugin de SvelteKit.
 *
 * Lo que se testea es la lógica pura —geometría, campos de distancia, parseo,
 * formato—, que es exactamente lo que la reescritura saca de adentro de los
 * componentes. No necesita DOM ni compilar `.svelte`, y sin el plugin la suite
 * arranca en una fracción del tiempo.
 *
 * Cuando haga falta probar un store con runes se agrega un proyecto aparte con
 * `environment: "jsdom"` y el plugin puesto.
 */
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
