// Fusiona el updater de Mac sin perder las plataformas ya publicadas.
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export function mergeManifest(previous, version, signature, url, pubDate) {
  if (previous !== null) {
    if (previous.version !== version) {
      throw new Error('El latest.json publicado es de otra versión');
    }
    if (!previous.platforms || typeof previous.platforms !== 'object'
        || Array.isArray(previous.platforms) || !Object.keys(previous.platforms).length) {
      throw new Error('El latest.json publicado no tiene plataformas válidas');
    }
  }
  return {
    ...previous,
    version,
    notes: previous?.notes ?? `Atic v${version}`,
    pub_date: pubDate,
    platforms: {
      ...previous?.platforms,
      'darwin-x86_64': { signature, url },
      'darwin-aarch64': { signature, url },
    },
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [existingPath, outPath, version, signature, url, pubDate] = process.argv.slice(2);
  // Un JSON roto detiene el release: nunca se reemplaza por uno sin Windows.
  const previous = existingPath ? JSON.parse(readFileSync(existingPath, 'utf8')) : null;
  if (existingPath && previous === null) throw new Error('El latest.json publicado es null');
  writeFileSync(outPath, JSON.stringify(mergeManifest(previous, version, signature, url, pubDate), null, 2));
}
