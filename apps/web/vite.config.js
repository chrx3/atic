import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath } from "node:url";

/**
 * El sitio comparte el sistema de diseño con la app: los tokens, paletas y
 * `core/` se importan directo de `apps/desktop`. Eso explica dos cosas:
 *
 *   - `server.fs.allow`: el dev server tiene que poder leer fuera de este
 *     paquete.
 *   - el alias `$atic`: para que el sitio diga de dónde viene cada pieza que
 *     no es suya, en vez de un `../../desktop/...` suelto por ahí.
 */
const desktop = fileURLToPath(new URL("../desktop", import.meta.url));

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  server: {
    port: 5180,
    strictPort: true,
    fs: {
      allow: [".", desktop],
    },
  },
  preview: {
    port: 4180,
    strictPort: true,
  },
});
