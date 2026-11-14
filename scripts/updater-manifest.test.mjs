import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { mergePlatforms } from './updater-manifest.mjs';

const version = '0.6.2';
const mac = { signature: 'firma-mac', url: 'https://example.test/Atic.app.tar.gz' };
const windows = { signature: 'firma-windows', url: 'https://example.test/Atic_0.6.2_x64-setup.exe' };
const all = { 'darwin-aarch64': mac, 'darwin-x86_64': mac, 'windows-x86_64': windows };
const script = new URL('./updater-manifest.mjs', import.meta.url).pathname;

test('arma las tres plataformas de una vez', () => {
  const result = mergePlatforms(null, version, all, '2026-11-14T00:00:00Z');
  assert.deepEqual(result.platforms, all);
  assert.equal(result.notes, `Atic v${version}`);
});

test('conserva las plataformas publicadas que no se tocan', () => {
  const previous = { version, notes: 'Notas', platforms: { 'linux-x86_64': mac } };
  const result = mergePlatforms(previous, version, { 'windows-x86_64': windows }, 'x');
  assert.deepEqual(Object.keys(result.platforms).sort(), ['linux-x86_64', 'windows-x86_64']);
  assert.equal(result.notes, 'Notas');
});

test('rechaza otra versión, plataformas previas inválidas y firmas vacías', () => {
  assert.throws(() => mergePlatforms({ version: '0.6.1', platforms: all }, version, all, 'x'), /otra versión/);
  assert.throws(() => mergePlatforms({ version, platforms: {} }, version, all, 'x'), /plataformas válidas/);
  assert.throws(() => mergePlatforms(null, version, { 'windows-x86_64': { url: 'u' } }, 'x'), /firma o la URL/);
});

test('la CLI lee las firmas y no pisa la salida con un previo roto', () => {
  const dir = mkdtempSync(join(tmpdir(), 'atic-manifest-'));
  try {
    const sig = join(dir, 'a.sig');
    const output = join(dir, 'latest.json');
    writeFileSync(sig, 'firma-windows\n');
    const ok = spawnSync(process.execPath, [script, '', output, version, 'x', 'windows-x86_64', sig, windows.url]);
    assert.equal(ok.status, 0, String(ok.stderr));
    assert.deepEqual(JSON.parse(readFileSync(output, 'utf8')).platforms, { 'windows-x86_64': windows });
    // «-» es «sin previo» (release-windows.ps1).
    const dash = spawnSync(process.execPath, [script, '-', output, version, 'x', 'windows-x86_64', sig, windows.url]);
    assert.equal(dash.status, 0, String(dash.stderr));
    assert.deepEqual(JSON.parse(readFileSync(output, 'utf8')).platforms, { 'windows-x86_64': windows });

    const input = join(dir, 'previous.json');
    writeFileSync(input, '{roto');
    writeFileSync(output, 'conservar');
    const broken = spawnSync(process.execPath, [script, input, output, version, 'x', 'windows-x86_64', sig, windows.url]);
    assert.notEqual(broken.status, 0);
    assert.equal(readFileSync(output, 'utf8'), 'conservar');
  } finally {
    rmSync(dir, { recursive: true });
  }
});
