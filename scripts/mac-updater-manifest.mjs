// Fusiona el updater de Mac sin perder las plataformas ya publicadas.
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { mergePlatforms } from './updater-manifest.mjs';

export function mergeManifest(previous, version, signature, url, pubDate) {
  // El .app universal sirve a las dos arquitecturas.
  const entry = { signature, url };
  return mergePlatforms(previous, version, { 'darwin-x86_64': entry, 'darwin-aarch64': entry }, pubDate);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [existingPath, outPath, version, signature, url, pubDate] = process.argv.slice(2);
  // Un JSON roto detiene el release: nunca se reemplaza por uno sin Windows.
  const previous = existingPath ? JSON.parse(readFileSync(existingPath, 'utf8')) : null;
  if (existingPath && previous === null) throw new Error('El latest.json publicado es null');
  writeFileSync(outPath, JSON.stringify(mergeManifest(previous, version, signature, url, pubDate), null, 2));
}
