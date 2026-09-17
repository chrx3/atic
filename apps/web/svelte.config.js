import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/**
 * El sitio es estático: se compila a HTML/CSS/JS y se sube a cualquier host.
 * El demo vive entero en el cliente (nada que renderizar en servidor), pero
 * el resto de las páginas se prerenderiza para que carguen al instante.
 *
 * @type {import('@sveltejs/kit').Config}
 */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
    alias: {
      // Las piezas de la app que el sitio reutiliza (tokens, catálogo de
      // herramientas): se importan del árbol real, no copiadas.
      $atic: "../desktop/src",
    },
  },
};

export default config;
