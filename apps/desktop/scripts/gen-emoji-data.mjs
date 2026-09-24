#!/usr/bin/env node
/**
 * Genera `src/lib/features/emoji/emojiData.json` desde emojibase-data (CLDR).
 *
 * Se corre a mano al subir de versión de Unicode; el JSON queda versionado y la
 * app no depende de red ni de paquetes para buscar emojis:
 *
 *   node scripts/gen-emoji-data.mjs
 *
 * Formato compacto (una fila por emoji, ordenado por `order` de CLDR):
 *   [emoji, grupo, nombre_es, nombre_en, palabras_clave, versión, tonos?]
 * `tonos` son las 5 variantes de tono uniforme (claro → oscuro), si existen.
 */
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const VERSION = "16";
const BASE = `https://cdn.jsdelivr.net/npm/emojibase-data@${VERSION}`;
/** Grupo 2 = componentes (tonos y pelos sueltos): no son emojis para pegar. */
const SKIP_GROUPS = new Set([2]);

async function load(lang) {
  const res = await fetch(`${BASE}/${lang}/data.json`);
  if (!res.ok) throw new Error(`${lang}: HTTP ${res.status}`);
  return res.json();
}

/** Tonos uniformes (un solo tono en toda la secuencia), en orden 1..5. */
function uniformSkins(entry) {
  if (!entry.skins) return null;
  const byTone = new Map();
  for (const skin of entry.skins) {
    if (typeof skin.tone === "number") byTone.set(skin.tone, skin.emoji);
    else if (Array.isArray(skin.tone) && new Set(skin.tone).size === 1) {
      byTone.set(skin.tone[0], skin.emoji);
    }
  }
  const skins = [1, 2, 3, 4, 5].map((tone) => byTone.get(tone));
  return skins.every(Boolean) ? skins : null;
}

const [en, es] = await Promise.all([load("en"), load("es")]);
const esByHex = new Map(es.map((entry) => [entry.hexcode, entry]));

const rows = en
  .filter((entry) => !SKIP_GROUPS.has(entry.group) && entry.group !== undefined)
  .sort((a, b) => a.order - b.order)
  .map((entry) => {
    const local = esByHex.get(entry.hexcode);
    const keywords = [...new Set([...(local?.tags ?? []), ...(entry.tags ?? [])])];
    const row = [
      entry.emoji,
      entry.group,
      local?.label ?? entry.label,
      entry.label,
      keywords.join(" "),
      entry.version,
    ];
    const skins = uniformSkins(entry);
    if (skins) row.push(skins);
    return row;
  });

const out = join(
  dirname(fileURLToPath(import.meta.url)),
  "../src/lib/features/emoji/emojiData.json",
);
writeFileSync(
  out,
  `${JSON.stringify({ source: `emojibase-data@${VERSION}`, rows })}\n`,
);
console.log(`${rows.length} emojis → ${out}`);
