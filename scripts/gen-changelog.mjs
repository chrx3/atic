#!/usr/bin/env node
// Genera CHANGELOG.md (español) y CHANGELOG.en.md (inglés) en la raíz desde
// apps/atic/assets/changelog.json, la misma fuente de las «Novedades» de la
// app. Uso, desde la raíz del repo:  node scripts/gen-changelog.mjs
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const { versions } = JSON.parse(readFileSync(join(root, "apps/atic/assets/changelog.json"), "utf8"));

const labels = {
  es: { new: "Nuevo", improved: "Mejorado", fixed: "Corregido" },
  en: { new: "New", improved: "Improved", fixed: "Fixed" },
};
const head = {
  es:
    "# Historial de versiones\n\nEnglish: [CHANGELOG.en.md](CHANGELOG.en.md)\n\n" +
    "Lo que notó quien usa Atic en cada versión, de la más nueva a la más antigua. " +
    "Es el mismo texto que la app muestra en «Novedades» al actualizar " +
    "(`apps/atic/assets/changelog.json`); este archivo se genera desde ahí con " +
    "`node scripts/gen-changelog.mjs`. Las descargas están en " +
    "[Releases](https://github.com/chrx3/atic/releases).\n",
  en:
    "# Version history\n\nEspañol: [CHANGELOG.md](CHANGELOG.md)\n\n" +
    "What people using Atic notice in each version, newest first. It is the same text " +
    "the app shows in \"What's new\" after an update (`apps/atic/assets/changelog.json`); " +
    "this file is generated from it with `node scripts/gen-changelog.mjs`. Downloads are on " +
    "[Releases](https://github.com/chrx3/atic/releases).\n",
};

for (const [lang, file] of [["es", "CHANGELOG.md"], ["en", "CHANGELOG.en.md"]]) {
  const out = [head[lang]];
  for (const v of versions) {
    out.push(`\n## ${v.version} — ${v.date}\n`);
    for (const kind of ["new", "improved", "fixed"]) {
      for (const item of v.items.filter((i) => i.kind === kind)) {
        out.push(`- **${labels[lang][kind]}:** ${item[lang]}`);
      }
    }
  }
  writeFileSync(join(root, file), out.join("\n") + "\n", "utf8");
}
console.log(`${versions.length} versiones → CHANGELOG.md y CHANGELOG.en.md`);
