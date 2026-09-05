/** Requires the debug executable running with --color-picker-smoke.
 * It connects only to that isolated WebView2 instance (port 9337).
 * node scripts/test-color-picker-native.mjs [path-to-playwright-package]
 */
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
const require = createRequire(import.meta.url);
const { chromium } = require(process.argv[2] || 'playwright');
const browser = await chromium.connectOverCDP('http://127.0.0.1:9337');
const page = browser.contexts().flatMap(c => c.pages()).find(p => p.url().includes('color-loupe'));
assert.ok(page, 'isolated color WebView exists');
page.setDefaultTimeout(6000);
const errors = [];
page.on('pageerror', error => errors.push(error.message));
await page.waitForSelector('.loupe');
await mkdir('target/color-picker', { recursive: true });
const ipc = (name, args = []) => page.evaluate(async ({ name, args }) => {
  const api = await import('/src/lib/ipc/captures.ts');
  return api[name](...args);
}, { name, args });
try {
  let state = await ipc('colorPickerState');
  if (state.active) await ipc('stopColorPicker', [state.session]);
  const started = Date.now();
  await ipc('startColorPicker');
  await page.waitForFunction(() => !document.querySelector('.read').disabled);
  const readyMs = Date.now() - started;
  state = await ipc('colorPickerState');
  assert.ok(state.active && state.patch?.session === state.session);
  const samples = await page.evaluate(async () => {
    const { on } = await import('/src/lib/ipc/events.ts');
    let count = 0;
    const off = await on('color-patch', () => count++);
    await new Promise(resolve => setTimeout(resolve, 500));
    off();
    return count;
  });
  assert.ok(samples >= 3, `continuous native sampling: ${samples} patches in 500 ms`);
  await page.screenshot({ path: 'target/color-picker/native-compact.png' });
  // Run several native resize/focus round trips, not mocked commands.
  const modes = [];
  for (let i = 0; i < 3; i++) {
    await page.locator('.read').focus();
    const start = Date.now();
    await page.keyboard.press('r');
    await page.waitForSelector('.rose');
    await page.waitForFunction(() => !document.querySelector('.rose-btn').disabled);
    state = await ipc('colorPickerState');
    assert.equal(state.open, true);
    modes.push(Date.now() - start);
    assert.equal(await page.evaluate(() => innerHeight), 540);
    await page.keyboard.press('r');
    await page.waitForSelector('.rose', { state: 'detached' });
    await page.waitForFunction(() => !document.querySelector('.rose-btn').disabled);
    assert.equal((await ipc('colorPickerState')).open, false);
    assert.equal(await page.evaluate(() => innerHeight), 112);
  }
  await page.keyboard.press('r');
  await page.waitForSelector('.rose');
  await page.waitForFunction(() => !document.querySelector('.rose-btn').disabled);
  assert.equal(await page.evaluate(() => document.documentElement.scrollHeight <= innerHeight), true, 'editor fits without scrolling');
  await page.screenshot({ path: 'target/color-picker/native-editor.png' });
  // Reload a still-open editor: the native snapshot must restore it.
  await page.reload();
  await page.waitForSelector('.rose');
  await page.waitForFunction(() => !document.querySelector('.read').disabled);
  assert.equal((await ipc('colorPickerState')).open, true);
  await page.locator('.cancel').click();
  assert.equal((await ipc('colorPickerState')).active, false);
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ readyMs, samplesIn500Ms: samples, nativeEditRoundTripsMs: modes, snapshotAfterReload: 'passed', cancellation: 'passed', browserErrors: errors }));
} finally {
  const state = await ipc('colorPickerState').catch(() => null);
  if (state?.active) await ipc('stopColorPicker', [state.session]);
  await browser.close();
}
