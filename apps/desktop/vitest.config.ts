import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";
import svelteConfig from "./svelte.config.js";

/**
 * Los mismos alias que usa la app, leídos de `svelte.config.js`.
 *
 * Duplicar la lista acá sería garantizar que algún día se separen y que los
 * tests importen una carpeta distinta de la que importa la app.
 */
const alias = Object.fromEntries(
  Object.entries(svelteConfig.kit?.alias ?? {}).map(([name, path]) => [
    name,
    fileURLToPath(new URL(path, import.meta.url)),
  ]),
);

/**
 * Config propia, sin el plugin de SvelteKit.
 *
 * Lo que se testea es la lógica pura —geometría, campos de distancia, parseo,
 * formato, el arranque de sesión—, que es exactamente lo que la reescritura
 * saca de adentro de los componentes. No necesita DOM ni compilar `.svelte`, y
 * sin el plugin la suite arranca en una fracción del tiempo.
 *
 * Cuando haga falta probar un store con runes se agrega un proyecto aparte con
 * `environment: "jsdom"` y el plugin puesto.
 */
export default defineConfig({
  resolve: { alias },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
