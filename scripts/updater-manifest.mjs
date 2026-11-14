// El latest.json del updater con varias plataformas a la vez (el release de
// GitHub Actions firma macOS y Windows en el mismo job), sin perder las que ya
// estén publicadas en ese release.
//
// Uso:
//   node scripts/updater-manifest.mjs <latest.json previo, "" o -> <salida> <versión> <pub_date> \
//     <plataforma> <archivo .sig> <url> [<plataforma> <archivo .sig> <url> …]
// «-» es lo mismo que "" (sin previo): PowerShell 5.1 no pasa un "" a node.
// Lo usan release.yml y scripts/release-windows.ps1.
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export function mergePlatforms(previous, version, platforms, pubDate) {
  if (previous !== null) {
    if (previous.version !== version) {
      throw new Error('El latest.json publicado es de otra versión');
    }
    if (!previous.platforms || typeof previous.platforms !== 'object'
        || Array.isArray(previous.platforms) || !Object.keys(previous.platforms).length) {
      throw new Error('El latest.json publicado no tiene plataformas válidas');
    }
  }
  for (const [name, entry] of Object.entries(platforms)) {
    if (!entry?.signature || !entry?.url) {
      throw new Error(`Falta la firma o la URL de ${name}`);
    }
  }
  return {
    ...previous,
    version,
    notes: previous?.notes ?? `Atic v${version}`,
    pub_date: pubDate,
    platforms: { ...previous?.platforms, ...platforms },
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [existingArg, outPath, version, pubDate, ...rest] = process.argv.slice(2);
  const existingPath = existingArg === '-' ? '' : existingArg;
  if (!outPath || !version || !pubDate || !rest.length || rest.length % 3 !== 0) {
    throw new Error('Uso: updater-manifest.mjs <previo|""> <salida> <versión> <pub_date> (<plataforma> <.sig> <url>)…');
  }
  const platforms = {};
  for (let i = 0; i < rest.length; i += 3) {
    platforms[rest[i]] = { signature: readFileSync(rest[i + 1], 'utf8').trim(), url: rest[i + 2] };
  }
  // Un JSON roto detiene el release: nunca se reemplaza por uno con menos plataformas.
  const previous = existingPath ? JSON.parse(readFileSync(existingPath, 'utf8')) : null;
  if (existingPath && previous === null) throw new Error('El latest.json publicado es null');
  writeFileSync(outPath, JSON.stringify(mergePlatforms(previous, version, platforms, pubDate), null, 2));
}
