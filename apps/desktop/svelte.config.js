// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
    // Un alias por capa de la arquitectura.
    //
    // No son cosmética: con ellos, la regla de "quién puede importar a quién"
    // es una lista de prefijos en `eslint.config.js` en vez de un plugin que
    // tenga que interpretar rutas relativas. Las carpetas van apareciendo a
    // medida que avanza la reescritura; un alias sin carpeta no molesta.
    alias: {
      $core: "src/lib/core",
      $ipc: "src/lib/ipc",
      $tokens: "src/lib/tokens",
      $ui: "src/lib/ui",
      $liquid: "src/lib/liquid",
      $patterns: "src/lib/patterns",
      $domain: "src/lib/domain",
      $features: "src/lib/features",
      $surfaces: "src/lib/surfaces",
    },
  },
};

export default config;
