// End-to-end test runner benchmark: `swiftbrowse test` vs Playwright Test on the
// same 12-test suite (examples/e2e), same app server, same Chromium binary.
//
// Every run is a full command, from process start to exit, as CI would run it.
// Runs are interleaved so both tools see the same machine state.
//
// Usage: node e2e.mjs <path-to-swiftbrowse-binary> [--iter 5] [--workers 1,4] [--repeat 1,5] [--browser shell|full]

import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : def; };
const SB_BIN = path.resolve(args[0] && !args[0].startsWith('--') ? args[0] : path.join(here, '../target/release/swiftbrowse'));
const ITER = +opt('--iter', 5);
const WORKERS = opt('--workers', '1,4').split(',').map(Number);
const REPEAT = opt('--repeat', '1,5').split(',').map(Number);
const BROWSER = opt('--browser', 'shell');
const OUT = path.resolve(opt('--out', path.join(here, 'results')));

const checks = JSON.parse(execFileSync(SB_BIN, ['doctor', '--json']).toString()).checks;
const pathOf = (name) => checks.find((c) => c.check === name)?.detail.split(' at ').pop().replace(/ \(used for headless sessions\)$/, '');
const CHROME = BROWSER === 'full' ? pathOf('chromium') : pathOf('headless_shell');
if (!CHROME) throw new Error(`no ${BROWSER} browser found (see swiftbrowse doctor)`);

const SITE = path.join(here, '../examples/e2e/site');
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css' };
const server = http.createServer((req, res) => {
  const file = path.join(SITE, path.normalize(decodeURIComponent(req.url.split('?')[0])).replace(/^(\.\.[/\\])+/, ''));
  fs.readFile(file, (err, body) => {
    res.writeHead(err ? 404 : 200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(err ? 'not found' : body);
  });
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const BASE = `http://127.0.0.1:${server.address().port}/`;

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'sb-e2e-bench-'));
const env = {
  ...process.env,
  BASE_URL: BASE,
  CHROMIUM: CHROME,
  SWIFTBROWSE_CHROMIUM: CHROME,
  // A private daemon, so the benchmark never attaches to one from another build.
  SWIFTBROWSE_DAEMON_SOCKET: path.join(scratch, 'daemon.sock'),
};

function timed(cmd, argv, cwd) {
  return new Promise((resolve) => {
    const t0 = performance.now();
    const child = spawn(cmd, argv, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    child.stdout.on('data', (d) => { out += d; });
    child.stderr.on('data', (d) => { out += d; });
    child.on('exit', (code) => resolve({ ms: performance.now() - t0, code, out }));
  });
}

const PW_CLI = path.join(here, 'node_modules/@playwright/test/cli.js');
const runners = {
  'playwright test': (workers, repeat) => timed(process.execPath, [
    PW_CLI, 'test', '-c', 'playwright.config.mjs', '--workers', String(workers),
    '--repeat-each', String(repeat), '--reporter', 'dot', '--output', path.join(scratch, 'pw'),
  ], path.join(here, 'e2e')),
  'swiftbrowse test': (workers, repeat) => timed(SB_BIN, [
    'test', 'tests', '--base-url', BASE, '-j', String(workers), '--repeat-each', String(repeat),
    '--reporter', 'dot', '--output-dir', path.join(scratch, 'sb'),
  ], path.join(here, '../examples/e2e')),
};
const stopDaemon = () => { try { execFileSync(SB_BIN, ['daemon', 'stop'], { env, stdio: 'ignore' }); } catch {} };

const median = (xs) => { const s = [...xs].sort((a, b) => a - b); return s[Math.floor(s.length / 2)]; };
const rows = [];
for (const repeat of REPEAT) {
  for (const workers of WORKERS) {
    const times = { 'playwright test': [], 'swiftbrowse test': [], 'swiftbrowse test (first run)': [] };
    // One untimed run each, so file caches are warm for both.
    stopDaemon();
    for (const run of Object.values(runners)) await run(workers, repeat);
    for (let i = 0; i < ITER; i++) {
      const order = i % 2 ? ['swiftbrowse test', 'playwright test'] : ['playwright test', 'swiftbrowse test'];
      for (const name of order) {
        const r = await runners[name](workers, repeat);
        if (r.code !== 0) throw new Error(`${name} failed (workers=${workers}, repeat=${repeat}):\n${r.out}`);
        times[name].push(r.ms);
      }
      // No daemon running: the first run of the day, or the first after 10 idle minutes.
      stopDaemon();
      const cold = await runners['swiftbrowse test'](workers, repeat);
      if (cold.code !== 0) throw new Error(`swiftbrowse test (first run) failed:\n${cold.out}`);
      times['swiftbrowse test (first run)'].push(cold.ms);
    }
    const tests = 12 * repeat;
    const row = { tests, workers };
    for (const [name, xs] of Object.entries(times)) row[name] = { median_ms: Math.round(median(xs)), min_ms: Math.round(Math.min(...xs)), runs: xs.map(Math.round) };
    rows.push(row);
    const pw = row['playwright test'].median_ms;
    const sb = row['swiftbrowse test'].median_ms;
    const sbCold = row['swiftbrowse test (first run)'].median_ms;
    console.log(`${tests} tests, ${workers} worker(s): playwright ${pw} ms · swiftbrowse ${sb} ms (${(pw / sb).toFixed(1)}× faster) · swiftbrowse first run ${sbCold} ms (${(pw / sbCold).toFixed(1)}×)`);
  }
}
stopDaemon();
server.close();

fs.mkdirSync(OUT, { recursive: true });
const stamp = new Date().toISOString().replace(/[:.]/g, '-');
const pwVersion = JSON.parse(fs.readFileSync(path.join(here, 'node_modules/@playwright/test/package.json'))).version;
const meta = { browser: BROWSER, chromium: CHROME, playwright_test: pwVersion, cpus: os.cpus().length, iterations: ITER, node: process.version };
fs.writeFileSync(path.join(OUT, `e2e-${stamp}.json`), JSON.stringify({ meta, rows }, null, 2));
console.log(`\n| Tests | Workers | Playwright Test | SwiftBrowse | SwiftBrowse, first run | Speed-up |\n|---|---|---|---|---|---|`);
for (const r of rows) {
  const pw = r['playwright test'].median_ms, sb = r['swiftbrowse test'].median_ms, c = r['swiftbrowse test (first run)'].median_ms;
  console.log(`| ${r.tests} | ${r.workers} | ${pw} ms | **${sb} ms** | ${c} ms | ${(pw / sb).toFixed(1)}× |`);
}
console.log(`\n${JSON.stringify(meta)}`);
