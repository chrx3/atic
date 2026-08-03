/**
 * Lo que ESLint no ve: los `<style>` scoped, que es donde vive la mayor parte
 * del CSS de la app.
 *
 * El objetivo es que **no puedan volver los 85 colores literales** que hay hoy
 * repartidos por los componentes, ni las duraciones y curvas sueltas que
 * esquivan los tokens de motion. La paleta es el único sitio donde un color
 * puede escribirse a mano.
 *
 * Igual que ESLint y Prettier, por ahora solo mira lo nuevo: el árbol viejo se
 * borra con la reescritura y la lista de exclusiones se vacía en la fase 9.
 */

/** @type {import('stylelint').Config} */
export default {
  extends: ["stylelint-config-standard"],
  customSyntax: undefined,
  overrides: [
    {
      files: ["**/*.svelte"],
      customSyntax: "postcss-html",
    },
    {
      // La paleta declara los literales; es su razón de existir.
      //
      // El banco de pruebas también: no es UI de producto, no se publica, y
      // tiene que poder pintar siluetas de colores que no están en ninguna
      // paleta para poder mirarlas.
      files: ["src/styles/palettes/**/*.css", "src/lib/dev/**", "src/routes/dev/**"],
      rules: {
        "color-no-hex": null,
        "declaration-property-value-disallowed-list": null,
      },
    },
  ],
  ignoreFiles: [
    ".svelte-kit/**",
    "build/**",
    "node_modules/**",
    "src/app.css",
    "src/lib/*.svelte",
    "src/routes/**/*.svelte",
    "!src/routes/dev/**",
  ],
  rules: {
    // Las at-rules de Tailwind v4. No son CSS estándar y stylelint no las
    // conoce, pero son justamente donde viven los tokens.
    "at-rule-no-unknown": [
      true,
      {
        ignoreAtRules: [
          "theme",
          "source",
          "custom-variant",
          "variant",
          "utility",
          "apply",
          "plugin",
          "config",
          "reference",
        ],
      },
    ],
    // Tailwind se importa por nombre de paquete, no por URL.
    "import-notation": "string",
    // Los valores de `text-rendering` vienen de SVG y se escriben en camelCase.
    // El autofix los pasaba a minúsculas: funciona, pero deja de leerse.
    "value-keyword-case": [
      "lower",
      { camelCaseSvgKeywords: true, ignoreKeywords: ["optimizeLegibility"] },
    ],
    "color-no-hex": true,
    "declaration-property-value-disallowed-list": {
      "/^(color|background|background-color|fill|stroke|border-color|outline-color)$/":
        ["/^#/", "/^rgb/", "/^hsl/", "/^oklch/"],
      // Fuerza `var(--duration-*)`: los ms sueltos son cómo se desincroniza el
      // motion, y `motion.ts` los lee del CSS para que haya una sola verdad.
      "/^(transition-duration|animation-duration)$/": ["/^[0-9]/"],
      "/^(transition-timing-function|animation-timing-function)$/": ["/^cubic-bezier/"],
    },
    // El proyecto usa kebab-case en las custom properties, no BEM.
    "custom-property-pattern": null,
    // Las paletas agrupan tokens con líneas en blanco —superficies, tinta,
    // estado— y esa separación es la que hace la lista legible.
    "custom-property-empty-line-before": null,
    "selector-class-pattern": null,
    // Svelte marca como `:global` lo que necesita salir del scope.
    "selector-pseudo-class-no-unknown": [true, { ignorePseudoClasses: ["global"] }],
  },
};
