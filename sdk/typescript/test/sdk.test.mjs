// The SDK end to end against the example app: plain node:test, no other framework.
// Needs a built `swiftbrowse` (SWIFTBROWSE_BIN, default ../../target/debug/swiftbrowse).

import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { after, before, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { launch, SwiftBrowseError } from '../src/index.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const site = path.join(here, '../../../examples/e2e/site');
process.env.SWIFTBROWSE_BIN ??= path.join(here, '../../../target/debug/swiftbrowse');

let server, base, browser;
before(async () => {
  server = http.createServer((req, res) => {
    fs.readFile(path.join(site, req.url.split('?')[0]), (err, body) => {
      res.writeHead(err ? 404 : 200, { 'content-type': 'text/html' });
      res.end(err ? 'not found' : body);
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${server.address().port}/`;
  browser = await launch();
});
after(async () => {
  await browser?.close();
  server?.close();
});

test('adds and completes a todo', async () => {
  const page = await browser.newSession();
  await page.navigate(base + 'index.html');
  await page.fill('New todo', 'Write the SDK');
  await page.pressKey('Enter', { expect: { type: 'visible', target: { by: 'role', role: 'checkbox', name: 'Write the SDK' } } });
  await page.check({ by: 'role', role: 'checkbox', name: 'Write the SDK' });
  await page.expect({ type: 'text_equals', target: { by: 'test_id', id: 'count' }, text: '0 items left' });
  assert.match(await page.text(), /checkbox "Write the SDK" \[e\d+\] checked/);
  await page.close();
});

test('mocks the network and reads results', async () => {
  const page = await browser.newSession();
  const { data } = await page.route({ url: '**/api/todos', json: [{ title: 'From the mock' }] });
  assert.match(data.id, /^r\d+$/);
  await page.navigate(base + 'api.html');
  await page.expect([
    { type: 'text_visible', text: 'From the mock' },
    { type: 'request_seen', url_contains: '/api/todos', method: 'GET' },
  ]);
  const value = (await page.evaluate('document.querySelectorAll("#list li").length')).data.value;
  assert.equal(value, 1);
  await page.close();
});

test('failures throw with the result attached', async () => {
  const page = await browser.newSession();
  await page.navigate(base + 'index.html');
  await assert.rejects(
    page.click('No such button', { timeout_ms: 300 }),
    (e) => e instanceof SwiftBrowseError && e.kind === 'timeout' && e.result.op === 'click',
  );
  await page.close();
});

test('sessions are isolated and run concurrently', async () => {
  const [a, b] = await Promise.all([browser.newSession(), browser.newSession()]);
  await Promise.all([a.navigate(base + 'index.html'), b.navigate(base + 'index.html')]);
  await a.fill('New todo', 'Only in A');
  await a.pressKey('Enter', { expect: { type: 'text_visible', text: 'Only in A' } });
  await b.reload();
  await b.expect({ type: 'text_absent', text: 'Only in A' });
  await Promise.all([a.close(), b.close()]);
});
