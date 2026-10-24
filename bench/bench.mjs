// SwiftBrowse vs Playwright on identical tasks.
//
// Fairness rules (keep these when changing the benchmark):
// - Same Chromium binary, same machine, same locally served pages, same viewport.
// - Each task performs the same user-visible steps and waits for the same condition.
// - Warm-up iterations are discarded for both tools; iteration counts are reported.
// - SwiftBrowse is measured through its agent-facing interface (MCP over stdio),
//   Playwright through its in-process library API. Work SwiftBrowse does in
//   addition (e.g. fill read-back verification) is not removed for the benchmark.
// - Wins come from engine changes, never from changing a task to suit one tool.
//
// Usage: node bench.mjs <path-to-swiftbrowse-binary> [--iter 30] [--cold 5] [--order swiftbrowse-first|playwright-first] [--out results]

import { chromium } from 'playwright-core';
import { spawn, execFileSync } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : def; };
const SB_BIN = path.resolve(args[0] || path.join(here, '../target/release/swiftbrowse'));
const ITER = +opt('--iter', 30);
const COLD = +opt('--cold', 20);
const WARMUP = 3;
const OUT = path.resolve(opt('--out', path.join(here, 'results')));

// Same binary for both tools: --browser shell (headless shell, default; Playwright's
// own headless default) or --browser full (full Chrome in new-headless mode).
const BROWSER = opt('--browser', 'shell');
const doctorChecks = JSON.parse(execFileSync(SB_BIN, ['doctor', '--json']).toString()).checks;
const pathOf = (name) => doctorChecks.find((c) => c.check === name)?.detail.split(' at ').pop().replace(/ \(used for headless sessions\)$/, '');
const CHROME = BROWSER === 'full' ? pathOf('chromium') : pathOf('headless_shell');
if (!CHROME) throw new Error(`no ${BROWSER} browser found (see swiftbrowse doctor)`);
process.env.SWIFTBROWSE_CHROMIUM = CHROME;
// Private warm-browser daemon for the "warm daemon" cold-start row.
const DAEMON_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'sb-bench-daemon-'));
process.env.SWIFTBROWSE_DAEMON_SOCKET = path.join(DAEMON_DIR, 'daemon.sock');

// ---------- pages ----------
const formHtml = fs.readFileSync(path.join(here, '../crates/chromium/tests/fixtures/form.html'), 'utf8');
const rows = Array.from({ length: 400 }, (_, i) =>
  `<tr><td>Row ${i}</td><td>Item description number ${i}</td><td><input aria-label="Qty ${i}" value="${i % 7}"></td>` +
  `<td><button aria-label="Edit row ${i}">Edit</button> <a href="#r${i}">Details</a></td></tr>`).join('');
const largeHtml = `<!doctype html><title>Large</title><h1>Inventory</h1><nav><a href="#a">Home</a><a href="#b">Reports</a></nav><table>${rows}</table>`;
const server = http.createServer((req, res) => {
  const body = req.url.startsWith('/form') ? formHtml : req.url.startsWith('/large') ? largeHtml : null;
  res.writeHead(body ? 200 : 404, { 'content-type': 'text/html; charset=utf-8' });
  res.end(body || 'not found');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const BASE = `http://127.0.0.1:${server.address().port}`;

// ---------- SwiftBrowse MCP client ----------
class SwiftBrowse {
  static async start(daemon = false) {
    const sb = new SwiftBrowse();
    sb.proc = spawn(SB_BIN, ['mcp', ...(daemon ? [] : ['--no-daemon']), '--no-approval', '--evidence-dir', path.join(os.tmpdir(), 'sb-bench-ev')], { stdio: ['pipe', 'pipe', 'inherit'] });
    sb.lines = readline.createInterface({ input: sb.proc.stdout });
    sb.waiting = [];
    sb.lines.on('line', (l) => sb.waiting.shift()?.(JSON.parse(l)));
    sb.id = 0;
    await sb.rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'bench', version: '1' } });
    sb.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
    return sb;
  }
  rpc(method, params) {
    return new Promise((resolve) => {
      this.waiting.push(resolve);
      this.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: ++this.id, method, params }) + '\n');
    });
  }
  async tool(name, a = {}) {
    const r = (await this.rpc('tools/call', { name, arguments: a })).result;
    const text = r.content[0].text;
    if (r.isError) throw new Error(`${name} failed: ${text.slice(0, 400)}`);
    // `bytes` is what the agent's context receives (the text content).
    return { data: r.structuredContent ?? JSON.parse(text), bytes: Buffer.byteLength(text) };
  }
  async stop() {
    this.proc.stdin.end();
    await new Promise((r) => this.proc.on('exit', r));
  }
}

// ---------- helpers ----------
const now = () => performance.now();
async function timed(fn) { const t = now(); const v = await fn(); return [now() - t, v]; }
function stats(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return { n: s.length, p50: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), mean: +(s.reduce((a, b) => a + b, 0) / s.length).toFixed(2) };
}
const results = {};
function record(task, tool, step, ms) {
  ((results[task] ??= {})[step] ??= {})[tool] ??= [];
  results[task][step][tool].push(ms);
}
const sizes = {};

// ---------- tasks ----------
const sbForm = async (sb, rec) => {
  rec('navigate', (await timed(() => sb.tool('navigate', { url: `${BASE}/form` })))[0]);
  rec('fill_email', (await timed(() => sb.tool('fill', { target: { by: 'label', text: 'Email' }, value: 'ada@example.com' })))[0]);
  rec('fill_password', (await timed(() => sb.tool('fill', { target: { by: 'label', text: 'Password' }, value: 'secret' })))[0]);
  rec('select', (await timed(() => sb.tool('select_option', { target: { by: 'label', text: 'Country' }, option: 'nl' })))[0]);
  rec('check', (await timed(() => sb.tool('click', { target: { by: 'label', text: 'I agree to the terms' } })))[0]);
  rec('submit_and_wait', (await timed(() => sb.tool('click', {
    target: { by: 'role', role: 'button', name: 'Sign up' },
    expect: { type: 'text_visible', text: 'Welcome' },
  })))[0]);
};
const pwForm = async (page, rec) => {
  rec('navigate', (await timed(() => page.goto(`${BASE}/form`)))[0]);
  rec('fill_email', (await timed(() => page.getByLabel('Email', { exact: true }).fill('ada@example.com')))[0]);
  rec('fill_password', (await timed(() => page.getByLabel('Password', { exact: true }).fill('secret')))[0]);
  rec('select', (await timed(() => page.getByLabel('Country', { exact: true }).selectOption('nl')))[0]);
  rec('check', (await timed(() => page.getByLabel('I agree to the terms').click()))[0]);
  rec('submit_and_wait', (await timed(async () => {
    await page.getByRole('button', { name: 'Sign up' }).click();
    await page.getByText('Welcome').waitFor();
  }))[0]);
};

// Every run after the first attaches to the daemon's warm browser.
async function coldSwiftBrowseDaemon() {
  const t = now();
  const sb = await SwiftBrowse.start(true);
  await sb.tool('navigate', { url: `${BASE}/form` });
  record('cold_start', 'swiftbrowse', 'launch_to_loaded_page_warm_daemon', now() - t);
  await sb.stop();
}

async function coldSwiftBrowse() {
  const t = now();
  const sb = await SwiftBrowse.start();
  await sb.tool('navigate', { url: `${BASE}/form` });
  record('cold_start', 'swiftbrowse', 'launch_to_loaded_page', now() - t);
  await sb.stop();
}

async function coldPlaywright() {
  const t = now();
  const browser = await chromium.launch({ executablePath: CHROME, headless: true });
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  await page.goto(`${BASE}/form`);
  record('cold_start', 'playwright', 'launch_to_loaded_page', now() - t);
  await browser.close();
}

async function runSwiftBrowse() {
  const sb = await SwiftBrowse.start();
  for (let i = 0; i < WARMUP + ITER; i++) {
    const keep = i >= WARMUP;
    const steps = {};
    const total = (await timed(() => sbForm(sb, (k, ms) => { steps[k] = ms; })))[0];
    if (keep) { for (const [k, ms] of Object.entries(steps)) record('form_task', 'swiftbrowse', k, ms); record('form_task', 'swiftbrowse', 'total', total); }
  }
  for (const page of ['form', 'large']) {
    await sb.tool('navigate', { url: `${BASE}/${page}` });
    for (let i = 0; i < WARMUP + ITER; i++) {
      const [ms, r] = await timed(() => sb.tool('get_page_snapshot', {}));
      if (i >= WARMUP) record(`snapshot_${page}`, 'swiftbrowse', 'snapshot', ms);
      sizes[`snapshot_${page}`] = { ...sizes[`snapshot_${page}`], swiftbrowse: r.bytes, swiftbrowse_truncated: r.data.data.truncated };
    }
  }
  for (let i = 0; i < WARMUP + ITER; i++) {
    const n = 100 + (i * 7) % 300;
    const [ms] = await timed(() => sb.tool('find_element', { target: { by: 'role', role: 'button', name: `Edit row ${n}`, exact: true } }));
    if (i >= WARMUP) record('semantic_lookup_large', 'swiftbrowse', 'role_and_name', ms);
  }
  await sb.stop();
}

async function runPlaywright() {
  const launchOpts = { executablePath: CHROME, headless: true };
  const browser = await chromium.launch(launchOpts);
  const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
  for (let i = 0; i < WARMUP + ITER; i++) {
    const keep = i >= WARMUP;
    const steps = {};
    const total = (await timed(() => pwForm(page, (k, ms) => { steps[k] = ms; })))[0];
    if (keep) { for (const [k, ms] of Object.entries(steps)) record('form_task', 'playwright', k, ms); record('form_task', 'playwright', 'total', total); }
  }
  for (const p of ['form', 'large']) {
    await page.goto(`${BASE}/${p}`);
    for (let i = 0; i < WARMUP + ITER; i++) {
      const [ms, snap] = await timed(() => page.locator('body').ariaSnapshot());
      if (i >= WARMUP) record(`snapshot_${p}`, 'playwright', 'snapshot', ms);
      sizes[`snapshot_${p}`] = { ...sizes[`snapshot_${p}`], playwright: Buffer.byteLength(snap) };
    }
  }
  for (let i = 0; i < WARMUP + ITER; i++) {
    const n = 100 + (i * 7) % 300;
    const [ms] = await timed(() => page.getByRole('button', { name: `Edit row ${n}`, exact: true }).boundingBox());
    if (i >= WARMUP) record('semantic_lookup_large', 'playwright', 'role_and_name', ms);
  }
  await browser.close();
}

// Order can matter (thermal state, page cache); `--order playwright-first` checks for that.
const ORDER = opt('--order', 'swiftbrowse-first');
// Cold starts are interleaved run by run so drift (page cache, CPU state) hits both
// tools equally; one discarded warm-up each loads both binaries from disk.
const coldPair = ORDER === 'playwright-first' ? [coldPlaywright, coldSwiftBrowse] : [coldSwiftBrowse, coldPlaywright];
for (let i = 0; i < COLD + 1; i++) {
  for (const run of coldPair) await run();
  await coldSwiftBrowseDaemon();
  if (i === 0) delete results.cold_start;
}
if (ORDER === 'playwright-first') { await runPlaywright(); await runSwiftBrowse(); } else { await runSwiftBrowse(); await runPlaywright(); }
server.close();
execFileSync(SB_BIN, ['daemon', 'stop']);
fs.rmSync(DAEMON_DIR, { recursive: true, force: true });

// ---------- report ----------
const chromeVersion = execFileSync(CHROME, ['--version']).toString().trim();
const sbVersion = execFileSync(SB_BIN, ['--version']).toString().trim();
const env = {
  date: new Date().toISOString(),
  cpu: `${os.cpus()[0]?.model} x${os.cpus().length}`,
  memory_gb: +(os.totalmem() / 2 ** 30).toFixed(1),
  os: `${os.type()} ${os.release()}`,
  node: process.version,
  playwright: '1.64.0 (playwright-core, library API)',
  swiftbrowse: `${sbVersion} (MCP over stdio)`,
  chromium: `${chromeVersion} (${BROWSER === 'full' ? 'full Chrome, new headless' : 'headless shell'}, both tools)`,
  iterations: ITER, warmup_discarded: WARMUP, cold_runs: COLD, order: ORDER,
  network: 'loopback HTTP server in the benchmark process',
};
const summary = {};
let md = `# SwiftBrowse vs Playwright\n\n`;
md += Object.entries(env).map(([k, v]) => `- **${k}**: ${v}`).join('\n') + '\n\n';
md += `SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().\n\nTimes in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).\n\n`;
md += `| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |\n|---|---|---:|---:|---:|---:|---:|\n`;
for (const [task, steps] of Object.entries(results)) {
  for (const [step, tools] of Object.entries(steps)) {
    const sbs = tools.swiftbrowse && stats(tools.swiftbrowse);
    const pws = tools.playwright && stats(tools.playwright);
    (summary[task] ??= {})[step] = { swiftbrowse: sbs, playwright: pws };
    md += `| ${task} | ${step} | ${sbs?.p50 ?? '-'} | ${sbs?.p95 ?? '-'} | ${pws?.p50 ?? '-'} | ${pws?.p95 ?? '-'} | ${sbs && pws ? (sbs.p50 / pws.p50).toFixed(2) : '-'} |\n`;
  }
}
md += `\n## Snapshot output size (bytes an agent receives)\n\n| page | SwiftBrowse compact | Playwright ariaSnapshot | note |\n|---|---:|---:|---|\n`;
for (const [k, v] of Object.entries(sizes)) {
  md += `| ${k} | ${v.swiftbrowse} | ${v.playwright} | ${v.swiftbrowse_truncated ? 'SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree' : 'both complete'} |\n`;
}
md += `\nSnapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.\n`;
fs.mkdirSync(OUT, { recursive: true });
const stamp = env.date.replace(/[:.]/g, '-');
fs.writeFileSync(path.join(OUT, `${stamp}.json`), JSON.stringify({ env, summary, sizes, raw: results }, null, 2));
fs.writeFileSync(path.join(OUT, `${stamp}.md`), md);
fs.writeFileSync(path.join(OUT, 'latest.md'), md);
console.log(md);
