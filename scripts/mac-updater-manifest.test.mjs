import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { mergeManifest } from './mac-updater-manifest.mjs';

const version = '0.6.1';
const url = 'https://github.com/chrx3/atic/releases/download/v0.6.1/Atic.app.tar.gz';
const windows = { signature: 'firma-windows', url: 'https://example.test/Atic_0.6.1_x64-setup.exe' };
const previous = { version, notes: 'Novedades publicadas', platforms: { 'windows-x86_64': windows } };
const merge = value => mergeManifest(value, version, 'firma-mac', url, '2026-11-12T00:00:00Z');

test('conserva Windows y las notas, y agrega ambas arquitecturas de Mac', () => {
  const result = merge(previous);
  assert.deepEqual(result.platforms['windows-x86_64'], windows);
  assert.equal(result.notes, previous.notes);
  for (const arch of ['aarch64', 'x86_64']) {
    assert.deepEqual(result.platforms[`darwin-${arch}`], { signature: 'firma-mac', url });
  }
  assert.deepEqual(Object.keys(previous.platforms), ['windows-x86_64']);
});

test('permite el primer release sin manifiesto previo', () => {
  assert.equal(Object.keys(merge(null).platforms).length, 2);
});

test('rechaza otra versión y plataformas ausentes o inválidas', () => {
  assert.throws(() => merge({ ...previous, version: '0.6.0' }), /otra versión/);
  for (const platforms of [undefined, null, [], {}, 'windows']) {
    assert.throws(() => merge({ version, platforms }), /plataformas válidas/);
  }
});

test('la CLI no reemplaza la salida si el manifiesto descargado está roto', () => {
  const dir = mkdtempSync(join(tmpdir(), 'atic-manifest-'));
  try {
    const input = join(dir, 'previous.json');
    const output = join(dir, 'latest.json');
    for (const invalid of ['{roto', 'null']) {
      writeFileSync(input, invalid);
      writeFileSync(output, 'conservar');
      const result = spawnSync(process.execPath, [
        new URL('./mac-updater-manifest.mjs', import.meta.url).pathname,
        input, output, version, 'firma-mac', url, '2026-11-12T00:00:00Z',
      ]);
      assert.notEqual(result.status, 0);
      assert.equal(readFileSync(output, 'utf8'), 'conservar');
    }
  } finally {
    rmSync(dir, { recursive: true });
  }
});
