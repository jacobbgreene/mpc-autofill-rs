// Runs against the installed binary on a disposable CI account. WebView2's
// debug endpoint is enabled by this test process only; releases do not set it.
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createServer } from 'node:net';
import { chromium, expect } from '../frontend/node_modules/@playwright/test/index.mjs';

assert.equal(process.env.GITHUB_ACTIONS, 'true', 'Expanded package test requires a disposable CI account');
const binary = resolve(process.argv[2]);
const root = mkdtempSync(join(tmpdir(), 'mpc-native-ui-'));
const orderPath = join(root, 'Native fixture.xml');
writeFileSync(orderPath, '<order><details><quantity>1</quantity><stock>(S30) Standard Smooth</stock><foil>false</foil></details><fronts><card><id>native-fixture-front</id><name>Island.png</name><query>Island</query><slots>0</slots></card></fronts><cardback>native-fixture-back</cardback></order>');
const wait = ms => new Promise(resolve => setTimeout(resolve, ms));
async function freePort() {
  const server = createServer();
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  return port;
}
function control(app, action, button) {
  return Number(execFileSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', resolve('scripts/native-window.ps1'), '-AppProcessId', String(app.pid), '-Action', action, ...(button ? ['-Button', button] : [])], { windowsHide: true, stdio: 'pipe', timeout: 12000, encoding: 'utf8' }).trim());
}
async function scenario(name, exercise) {
  const port = await freePort();
  const app = spawn(binary, [], { windowsHide: true, stdio: 'ignore', env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` } });
  let browser;
  try {
    for (let i = 0; i < 80; i++) {
      if (app.exitCode !== null) throw new Error(`App exited during ${name}`);
      try { browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 1000 }); break; } catch { await wait(250); }
    }
    assert(browser, 'Installed WebView2 did not expose its test endpoint');
    let page;
    for (let i = 0; i < 40; i++) {
      page = browser.contexts().flatMap(context => context.pages()).find(page => /tauri|localhost/.test(page.url()));
      if (page) break;
      await wait(250);
    }
    assert(page, 'Installed app did not create its WebView page');
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await expect(page.getByRole('button', { name: 'Paste a decklist', exact: true })).toBeEnabled({ timeout: 15000 });
    const confirmation = await exercise(page, app);
    let started = control(app, 'close');
    if (confirmation) started = control(app, 'choose', confirmation);
    await wait(0);
    while (app.exitCode === null && Date.now() - started < 5000) await wait(50);
    assert.equal(app.exitCode, 0, `${name}: process must exit cleanly within five seconds`);
    assert.deepEqual(errors, [], `${name}: no WebView runtime errors`);
    console.log(`PASS: installed Windows app — ${name}`);
  } finally {
    await browser?.close().catch(() => {});
    if (app.exitCode === null) app.kill(); // Failure cleanup never satisfies a test.
  }
}
await scenario('saved deck native close', async page => {
  // Stub only the system file picker; the load, parse, summary and UI use real IPC.
  await page.evaluate(path => {
    const original = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (command, args) => command === 'plugin:dialog|open' ? Promise.resolve(path) : original(command, args);
  }, orderPath);
  await page.getByRole('button', { name: /Open.*file|Open.*order/i }).first().click();
  await expect(page.getByText('Native fixture', { exact: true })).toBeVisible();
  await expect(page.getByText('Saved', { exact: true })).toBeVisible();
});
await scenario('empty untouched import native close', async page => {
  await page.getByRole('button', { name: 'Paste a decklist', exact: true }).click();
  await expect(page.getByLabel('Decklist', { exact: true })).toBeVisible();
});
await scenario('dirty import keep editing and discard', async (page, app) => {
  await page.getByRole('button', { name: 'Paste a decklist', exact: true }).click();
  await page.getByLabel('Decklist', { exact: true }).fill('# Native close fixture');
  control(app, 'close');
  control(app, 'choose', 'Keep editing');
  await expect(page.getByLabel('Decklist', { exact: true })).toHaveValue('# Native close fixture');
  assert.equal(app.exitCode, null);
  return 'Discard and close';
});
await scenario('nested gallery discard and close', async page => {
  await page.getByRole('button', { name: 'Paste a decklist', exact: true }).click();
  await page.getByLabel('Decklist', { exact: true }).fill('1 Island');
  await page.getByRole('button', { name: 'Choose card back', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'Choose a card back' })).toBeVisible();
  return 'Discard and close';
});
