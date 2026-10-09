// Empaqueta agent.mjs con el Claude Agent SDK en un solo archivo (dist/agent.mjs),
// el que viaja junto a atic-pill.exe. En desarrollo se usa agent.mjs directo.
import { build } from "esbuild";

await build({
  entryPoints: ["agent.mjs"],
  outfile: "dist/agent.mjs",
  bundle: true,
  platform: "node",
  format: "esm",
  target: "node18",
  logLevel: "warning",
  // Algunas dependencias usan require(): se lo damos en el módulo ESM.
  banner: { js: "import { createRequire as __atic_cr } from 'module'; const require = __atic_cr(import.meta.url);" },
});
console.log("sidecar empaquetado → dist/agent.mjs");
