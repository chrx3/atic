// Genera assets/i18n/atic.{es,en}.json desde los diccionarios de la app de
// Tauri (apps/desktop/src/lib/core/i18n), aplanados a claves con puntos.
// Las claves propias de la pill van a mano en assets/i18n/pill.{es,en}.json.
//
// Uso (Node 22.6+): node prototypes/pill-gpui/scripts/i18n-export.mjs

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, "../../../apps/desktop/src/lib/core/i18n");
const target = join(here, "../assets/i18n");

const { es } = await import(new URL(`file:///${join(source, "es.ts")}`).href);
const { en } = await import(new URL(`file:///${join(source, "en.ts")}`).href);

function flatten(tree, prefix = "", out = {}) {
  for (const [key, value] of Object.entries(tree)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === "string") out[path] = value;
    else flatten(value, path, out);
  }
  return out;
}

function write(name, dict) {
  const flat = flatten(dict);
  const sorted = Object.fromEntries(Object.keys(flat).sort().map((key) => [key, flat[key]]));
  writeFileSync(join(target, name), JSON.stringify(sorted, null, 1) + "\n");
  return Object.keys(sorted).length;
}

mkdirSync(target, { recursive: true });
console.log(`es: ${write("atic.es.json", es)} claves, en: ${write("atic.en.json", en)} claves`);
