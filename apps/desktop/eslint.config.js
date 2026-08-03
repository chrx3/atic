import js from "@eslint/js";
import ts from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import prettier from "eslint-config-prettier";
import svelteConfig from "./svelte.config.js";

/**
 * Reglas de importación por capa.
 *
 * La arquitectura del plan es una sola flecha, siempre hacia abajo:
 *
 *   routes → surfaces → features → patterns → ui | liquid → tokens → ipc → core
 *
 * Sin esto la dirección es una intención escrita en un documento; con esto es
 * un error de build. Los alias de `svelte.config.js` existen justamente para
 * que la regla sea una lista de prefijos y no un plugin de rutas relativas.
 *
 * Se aplican solo a las carpetas nuevas. El árbol viejo desaparece con la
 * reescritura, y ponerle reglas a código que se va a borrar solo produce ruido.
 */
const boundaries = [
  {
    files: ["src/lib/core/**"],
    banned: [
      "$ipc/*",
      "$tokens/*",
      "$ui/*",
      "$liquid/*",
      "$patterns/*",
      "$domain/*",
      "$features/*",
      "$surfaces/*",
      "$lib/*",
      "@tauri-apps/*",
      "svelte",
      "svelte/*",
    ],
    why: "core es TS puro: sin DOM, sin Svelte, sin Tauri y sin nada del proyecto.",
  },
  {
    files: ["src/lib/ipc/**"],
    banned: [
      "$ui/*",
      "$liquid/*",
      "$patterns/*",
      "$domain/*",
      "$features/*",
      "$surfaces/*",
      "svelte",
      "svelte/*",
    ],
    why: "ipc solo habla con Rust y con core.",
  },
  {
    files: ["src/lib/ui/**", "src/lib/liquid/**"],
    banned: ["$ipc/*", "$domain/*", "$features/*", "$surfaces/*", "@tauri-apps/*"],
    why: "las primitivas no saben de dominio ni de Tauri: reciben props y nada más.",
  },
  {
    files: ["src/lib/patterns/**"],
    banned: ["$ipc/*", "$domain/*", "$features/*", "$surfaces/*", "@tauri-apps/*"],
    why: "los patrones componen primitivas; el dominio entra por las features.",
  },
  {
    files: ["src/lib/domain/**"],
    banned: ["$ui/*", "$liquid/*", "$patterns/*", "$features/*", "$surfaces/*"],
    why: "el estado no importa componentes: son los componentes los que lo leen.",
  },
];

export default ts.config(
  {
    ignores: [
      ".svelte-kit/",
      "build/",
      "src-tauri/",
      "static/",
      // El árbol previo a la reescritura. La lista se achica fase a fase y
      // queda vacía en la fase 9; hasta entonces, poner en verde 25.000 líneas
      // que se van a borrar es trabajo tirado.
      "src/lib/*.svelte",
      "src/lib/*.ts",
      "src/routes/**/*.svelte",
      "!src/routes/dev/**",
      "src/app.css",
    ],
  },

  js.configs.recommended,
  ts.configs.recommended,
  svelte.configs.recommended,
  prettier,
  svelte.configs.prettier,

  {
    rules: {
      // TypeScript ya resuelve los identificadores; dejarla prendida solo
      // obliga a mantener una lista de globals del navegador en paralelo.
      "no-undef": "off",
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_", varsIgnorePattern: "^_" },
      ],
    },
  },

  // Reglas que necesitan tipos. Solo bajo `src/`: los archivos de config de la
  // raíz no están en el `tsconfig` y el servicio de proyecto los rechaza. Y
  // solo en `.ts`: en `.svelte` el servicio todavía es frágil, y `svelte-check`
  // ya cubre lo importante.
  {
    files: ["src/**/*.ts"],
    extends: [ts.configs.recommendedTypeChecked],
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
  },

  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: {
        // Sin esto el parser de Svelte lee el `<script lang="ts">` como JS y se
        // cae en la primera anotación de tipo.
        parser: ts.parser,
        svelteConfig,
      },
    },
    rules: {
      // Las listas de turnos y de herramientas se reordenan: sin clave, Svelte
      // reusa nodos por posición y el estado se mezcla entre elementos.
      "svelte/require-each-key": "error",
      "svelte/no-dom-manipulating": "error",
      "svelte/no-reactive-reassign": "error",
      "svelte/button-has-type": "error",
      "svelte/no-useless-mustaches": "error",
      // Única superficie por la que entra HTML ajeno: la salida del agente.
      // Cada uso tiene que justificarse con un comentario en el sitio.
      "svelte/no-at-html-tags": "error",
    },
  },

  ...boundaries.map(({ files, banned, why }) => ({
    files,
    rules: {
      "no-restricted-imports": [
        "error",
        { patterns: [{ group: banned, message: why }] },
      ],
    },
  })),

  // El banco de pruebas vive fuera de las reglas de capa: es dev, habla con
  // Tauri a propósito y no se publica.
  {
    files: ["src/lib/dev/**", "src/routes/dev/**"],
    rules: { "no-restricted-imports": "off" },
  },

  {
    files: ["**/*.test.ts"],
    rules: { "@typescript-eslint/no-non-null-assertion": "off" },
  },
);
