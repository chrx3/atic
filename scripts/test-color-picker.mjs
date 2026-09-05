/** UI integration regressions against a running Vite server; IPC is mocked.
 * node scripts/test-color-picker.mjs [path-to-playwright-package]
 * Screenshots go to the ignored target/color-picker directory.
 */
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdir } from 'node:fs/promises';
const require = createRequire(import.meta.url);
const { chromium } = require(process.argv[2] || 'playwright');
const browser = await chromium.launch({ headless: true });
const page = await browser.newPage({ viewport: { width: 264, height: 112 } });
const errors = [];
page.on('pageerror', error => errors.push(error.message));
await page.addInitScript(() => {
  const callbacks = new Map();
  const listeners = new Map();
  let id = 0;
  const api = window.__colorTest = {
    calls: [], fail: false, delay: false, resolve: null,
    emit(event, payload) {
      for (const handler of listeners.get(event) || []) callbacks.get(handler)?.({ event, payload });
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'color-loupe' }, currentWebview: { label: 'color-loupe' } },
    transformCallback(cb) { callbacks.set(++id, cb); return id; },
    unregisterCallback(key) { callbacks.delete(key); },
    async invoke(cmd, args) {
      if (cmd === 'plugin:event|listen') {
        if (!listeners.has(args.event)) listeners.set(args.event, []);
        listeners.get(args.event).push(args.handler);
        return args.handler;
      }
      if (cmd === 'plugin:event|unlisten') return;
      if (cmd === 'get_config') return { ui_theme: 'dark', ui_language: 'es' };
      if (cmd === 'complete_color_pick') {
        api.calls.push({ cmd, ...args });
        if (api.delay) await new Promise(resolve => { api.resolve = resolve; });
        if (api.fail) throw new Error('clipboard busy');
        api.emit('color-picker-ended', args.session);
        return args.hex;
      }
      if (cmd === 'stop_color_picker') api.emit('color-picker-ended', args.session);
      return null;
    },
  };
});
try {
  await page.goto(process.env.COLOR_TEST_URL || 'http://localhost:1420/color-loupe');
  await page.waitForSelector('.loupe');
  const emit = (event, payload) => page.evaluate(({ event, payload }) => window.__colorTest.emit(event, payload), { event, payload });
  const patch = (session, r, g, b) => ({ session, r, g, b, size: 13,
    hex: '#' + [r, g, b].map(n => n.toString(16).padStart(2, '0')).join('').toUpperCase(),
    rgba: Array.from({ length: 169 }, () => [r, g, b, 255]).flat() });
  const waitValue = value => page.waitForFunction(value => document.querySelector('.read .hex')?.textContent === value, value);
  const openRose = async () => {
    await page.locator('.rose-btn').click();
    await page.waitForSelector('.rose');
    await page.setViewportSize({ width: 296, height: 540 });
  };
  await emit('color-patch', patch(1, 0, 0, 255));
  await waitValue('#0000FF');
  await mkdir('target/color-picker', { recursive: true });
  await page.screenshot({ path: 'target/color-picker/compact.png' });
  await openRose();
  const ringBox = await page.locator('.ring').boundingBox();
  await page.mouse.click(ringBox.x + ringBox.width / 2, ringBox.y + 8);
  await waitValue('#FF0000');
  await page.getByRole('button', { name: '240°', exact: true }).click();
  await waitValue('#0000FF');
  const saturation = page.getByLabel('Saturación', { exact: true });
  const brightness = page.getByLabel('Brillo', { exact: true });
  await saturation.fill('0');
  await waitValue('#FFFFFF');
  await saturation.fill('100');
  await waitValue('#0000FF');
  await brightness.fill('0');
  await page.getByRole('slider', { name: 'Matiz', exact: true }).focus();
  await page.keyboard.press('Home');
  await brightness.fill('100');
  await waitValue('#FF0000');
  assert.equal(await page.getByLabel('HEX', { exact: true }).inputValue(), '#FF0000');
  await page.getByLabel('HEX', { exact: true }).fill('#00FF00');
  await page.getByRole('button', { name: 'Aplicar', exact: true }).click();
  await waitValue('#00FF00');
  await page.locator('.code').nth(1).click();
  await page.locator('.rose-btn').click();
  await page.waitForSelector('.rose', { state: 'detached' });
  await page.setViewportSize({ width: 264, height: 112 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollHeight <= innerHeight), true, 'compact RGB value fits');
  await emit('color-request-commit', { session: 1, patch: patch(1, 0, 255, 255) });
  await page.waitForFunction(() => window.__colorTest.calls.length === 1);
  assert.equal(await page.evaluate(() => window.__colorTest.calls[0].hex), 'rgb(0, 255, 255)');
  await page.waitForFunction(() => localStorage.getItem('atic-color-recent') === '["#00FFFF"]');
  // A failed native write must keep the selected color, expose retry, and not add history.
  await emit('color-patch', patch(3, 255, 0, 255));
  await page.evaluate(() => { window.__colorTest.fail = true; });
  await emit('color-request-commit', { session: 3, patch: patch(3, 255, 0, 255) });
  await page.waitForSelector('[role="alert"]');
  await page.waitForSelector('.rose');
  await page.setViewportSize({ width: 296, height: 540 });
  assert.equal(await page.evaluate(() => localStorage.getItem('atic-color-recent')), '["#00FFFF"]');
  await page.screenshot({ path: 'target/color-picker/retry.png' });
  await page.evaluate(() => { window.__colorTest.fail = false; });
  await page.locator('.copy').click();
  await page.waitForFunction(() => localStorage.getItem('atic-color-recent') === '["#FF00FF","#00FFFF"]');
  // Late events from an ended session must not replace the new color or copy it.
  await emit('color-patch', patch(5, 255, 128, 0));
  await emit('color-patch', patch(3, 0, 0, 0));
  await emit('color-request-commit', { session: 3, patch: patch(3, 0, 0, 0) });
  await waitValue('rgb(255, 128, 0)');
  assert.equal(await page.evaluate(() => window.__colorTest.calls.length), 3);
  await openRose();
  await page.screenshot({ path: 'target/color-picker/rose.png' });
  assert.equal(await page.evaluate(() => document.documentElement.scrollHeight <= innerHeight), true, 'editor including recent colors fits');
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  await page.setViewportSize({ width: 240, height: 420 });
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  await page.locator('.cancel').click();
  await page.waitForFunction(() => document.querySelector('.read').disabled);
  // Enter and repeated commit requests share one in-flight write.
  await emit('color-patch', patch(7, 12, 34, 56));
  await page.evaluate(() => { window.__colorTest.delay = true; });
  await page.locator('.read').focus();
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => window.__colorTest.calls.length === 4);
  await emit('color-request-commit', { session: 7, patch: patch(7, 200, 200, 200) });
  assert.equal(await page.evaluate(() => window.__colorTest.calls.length), 4);
  assert.equal(await page.evaluate(() => window.__colorTest.calls.at(-1).hex), 'rgb(12, 34, 56)');
  // An old response arriving after cancellation must not reset a newer session.
  await emit('color-picker-ended', 7);
  await emit('color-patch', patch(9, 0, 255, 0));
  await page.evaluate(() => window.__colorTest.resolve());
  await waitValue('rgb(0, 255, 0)');
  assert.equal(await page.locator('.read').isEnabled(), true);
  assert.deepEqual(errors, []);
  console.log('PASS: pointer/keyboard hue, grayscale/black, HEX edit, format+history, copy failure+retry, stale events/responses, Enter, duplicate writes, responsive layout, cancellation; no browser errors.');
} finally {
  await browser.close();
}
