// Build first: node frontend/node_modules/vite/bin/vite.js build (in frontend).
// Run: node performance/deck-scroll-2026-10-04/measure.mjs
// Production UI, mocked Tauri, distinct 458x640 JPEGs, all decoded before scrolling.
import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createServer } from 'node:http';
import { resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '../../frontend/node_modules/@playwright/test/index.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const dist = fileURLToPath(new URL('../../frontend/dist/', import.meta.url));
const count = Number(process.env.CARDS || 100);
const repeats = Number(process.env.REPEATS || 3);
const cpuRate = Number(process.env.CPU || 1);
const deviceScaleFactor = Number(process.env.DPR || 1);
const variants = {
  baseline: '.card-tile { content-visibility: auto; }',
  candidate: '',
  visible: '.card-tile { content-visibility: visible; }',
  noHover: '.card-tile { content-visibility: auto; } .card-art, .preview-hint { transition: none !important; } .card-tile:hover .card-art { transform: none; box-shadow: none; }',
};
const chosen = (process.env.VARIANTS || 'baseline,candidate').split(',');
const server = createServer(async (req, res) => {
  try {
    const path = resolve(dist, '.' + (req.url === '/' ? '/index.html' : req.url));
    assert(path.startsWith(dist));
    res.setHeader('Content-Type', { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' }[extname(path)] || 'application/octet-stream');
    res.end(await readFile(path));
  } catch { res.writeHead(404).end(); }
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL || 'chrome' });
const rows = [];
try {
  const generator = await browser.newPage();
  const images = await generator.evaluate(count => {
    const canvas = document.createElement('canvas');
    canvas.width = 458; canvas.height = 640;
    const ctx = canvas.getContext('2d');
    let seed = 42;
    const random = () => ((seed = (1664525 * seed + 1013904223) >>> 0) / 4294967296);
    return Array.from({ length: count }, (_, i) => {
      ctx.fillStyle = `hsl(${i * 137.5 % 360} 35% 25%)`;
      ctx.fillRect(0, 0, 458, 640);
      for (let j = 0; j < 250; j++) {
        ctx.fillStyle = `hsl(${random() * 360} 45% ${20 + random() * 60}%)`;
        ctx.fillRect(15 + random() * 400, 50 + random() * 450, 5 + random() * 70, 5 + random() * 70);
      }
      ctx.fillStyle = '#ddd'; ctx.fillRect(25, 510, 408, 105);
      ctx.fillStyle = '#222'; ctx.font = '24px sans-serif'; ctx.fillText(`Artwork ${i + 1}`, 40, 545);
      return canvas.toDataURL('image/jpeg', 0.88);
    });
  }, count);
  await generator.close();
  for (let repetition = 0; repetition < repeats; repetition++) {
    for (const variant of repetition % 2 ? [...chosen].reverse() : chosen) {
      const page = await browser.newPage({ viewport: { width: 1100, height: 820 }, deviceScaleFactor });
      const cdp = await page.context().newCDPSession(page);
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: cpuRate });
      await cdp.send('Performance.enable');
      const errors = [];
      page.on('pageerror', e => errors.push(e.message));
      await page.addInitScript(({ images }) => {
        const callbacks = new Map(), listeners = new Map();
        let next = 0;
        const cards = images.map((_, i) => ({ face: 'front', drive_id: String(i), name: `Artwork ${i + 1}.png`, slots: [i], cardback: false, dpi: 1200 }));
        window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
        window.__TAURI_INTERNALS__ = {
          metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
          transformCallback: cb => (callbacks.set(++next, cb), next),
          unregisterCallback: id => callbacks.delete(id),
          convertFileSrc: path => images[Number(path)],
          invoke: async (command, args = {}) => {
            if (command === 'plugin:event|listen') { listeners.set(args.event, [...(listeners.get(args.event) || []), args.handler]); return args.handler; }
            if (command === 'plugin:event|unlisten') return;
            if (command === 'plugin:dialog|open') return 'C:/fixtures/scroll.xml';
            if (command === 'load_order') return { name: 'Scroll fixture', quantity: cards.length, cards, stock: 'S30', foil: false, path: 'C:/fixtures/scroll.xml', dirty: false, session_id: 1 };
            if (command === 'artwork_dpi') return {};
            if (command === 'thumbnails') {
              for (const card of cards) for (const id of listeners.get('thumb-ready') || []) callbacks.get(id)({ payload: { drive_id: card.drive_id, session_id: 1, path: card.drive_id } });
              return;
            }
            throw Error(`Unexpected command ${command}`);
          },
        };
      }, { images });
      await page.goto(`http://127.0.0.1:${server.address().port}/`);
      if (variants[variant]) await page.addStyleTag({ content: variants[variant] });
      const openedAt = Date.now();
      await page.getByRole('button', { name: 'Open order', exact: true }).click();
      await page.waitForFunction(count => document.querySelectorAll('.card-art > img').length === count, count);
      const readyMs = Date.now() - openedAt;
      assert.equal(await page.locator('.card-art > img').count(), count);
      await page.evaluate(async () => {
        const images = [...document.querySelectorAll('.card-art > img')];
        images.forEach(img => img.loading = 'eager');
        await Promise.all(images.map(img => img.decode()));
        for (let y = 0; y < document.documentElement.scrollHeight; y += innerHeight) {
          scrollTo(0, y); await new Promise(requestAnimationFrame);
        }
        scrollTo(0, 550);
      });
      await page.mouse.move(100, 420);
      await page.waitForTimeout(300);
      const before = (await cdp.send('Performance.getMetrics')).metrics;
      const trace = [];
      cdp.on('Tracing.dataCollected', e => trace.push(...e.value));
      if (process.env.TRACE) await cdp.send('Tracing.start', { categories: 'devtools.timeline,disabled-by-default-devtools.timeline,cc', transferMode: 'ReportEvents' });
      await page.evaluate(() => {
        window.__scroll = { frames: [], visibilityChanges: 0, mutations: 0, heights: [document.documentElement.scrollHeight], active: true };
        document.querySelectorAll('.card-tile').forEach(el => el.addEventListener('contentvisibilityautostatechange', () => window.__scroll.visibilityChanges++));
        new MutationObserver(m => window.__scroll.mutations += m.length).observe(document.querySelector('.card-grid'), { childList: true, subtree: true, attributes: true });
        let last;
        function sample(now) {
          if (last) window.__scroll.frames.push(now - last);
          last = now;
          if (window.__scroll.active) requestAnimationFrame(sample);
        }
        requestAnimationFrame(sample);
      });
      const distance = Math.min(3800, await page.evaluate(() => document.documentElement.scrollHeight - innerHeight - scrollY));
      for (const direction of [-1, 1]) await cdp.send('Input.synthesizeScrollGesture', { x: 100, y: 420, yDistance: direction * distance, speed: 2400, gestureSourceType: 'mouse' });
      const after = (await cdp.send('Performance.getMetrics')).metrics;
      const data = await page.evaluate(() => {
        window.__scroll.active = false;
        window.__scroll.heights.push(document.documentElement.scrollHeight);
        return window.__scroll;
      });
      if (process.env.TRACE) {
        const done = new Promise(r => cdp.once('Tracing.tracingComplete', r));
        await cdp.send('Tracing.end'); await done;
        await writeFile(resolve(here, `${variant}-${count}-${repetition}-trace.json`), JSON.stringify({ traceEvents: trace }));
      }
      const metrics = Object.fromEntries(['TaskDuration', 'LayoutDuration', 'RecalcStyleDuration', 'ScriptDuration', 'LayoutCount', 'RecalcStyleCount'].map(name => [name, after.find(m => m.name === name).value - before.find(m => m.name === name).value]));
      const sorted = [...data.frames].sort((a, b) => a - b);
      const row = { variant, count, repetition, distance, readyMs, heapMiB: after.find(m => m.name === 'JSHeapUsedSize').value / 1048576, ...metrics, frameP95: sorted[Math.floor(sorted.length * 0.95)], frameP99: sorted[Math.floor(sorted.length * 0.99)], framesOver12: sorted.filter(t => t > 12).length, framesOver25: sorted.filter(t => t > 25).length, frameCount: sorted.length, visibilityChanges: data.visibilityChanges, mutations: data.mutations, heights: data.heights, traceTotalsMs: Object.fromEntries(['Layout', 'Paint', 'RasterTask', 'UpdateLayoutTree', 'PrePaint', 'ImageDecodeTask'].map(name => [name, trace.filter(e => e.name === name && e.ph === 'X').reduce((sum, e) => sum + e.dur / 1000, 0)])), errors };
      assert.deepEqual(errors, []);
      assert.equal(data.heights[0], data.heights[1]);
      rows.push(row); console.log(JSON.stringify(row));
      if (variant === 'candidate' && repetition === 0)
        await page.screenshot({ path: resolve(here, `candidate-${count}.png`) });
      await page.close();
    }
  }
  await mkdir(here, { recursive: true });
  await writeFile(resolve(here, process.env.OUTPUT || `results-${count}.json`), JSON.stringify({ browser: browser.version(), count, repeats, cpuRate, deviceScaleFactor, rows }, null, 2));
} finally { await browser.close(); await new Promise(r => server.close(r)); }
