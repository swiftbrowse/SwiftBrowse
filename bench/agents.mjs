// Agent task benchmark: SwiftBrowse MCP vs Playwright MCP, both over MCP stdio.
//
// Each run behaves like an agent: read the server's own snapshot, take element
// refs from it, act, and confirm the outcome. Measured per task: wall time,
// tool calls, bytes returned into the agent's context (text content of every
// tool result), and whether the outcome was confirmed.
//
// Fairness rules: same Chromium binary, pages, viewport and machine; Playwright
// MCP runs with its defaults and again with --timeout-settle 0 (its best case);
// no task-specific tuning of either server.
//
// Usage: node agents.mjs <swiftbrowse-binary> [--iter 15] [--cold 10]

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
const ITER = +opt('--iter', 15);
const COLD = +opt('--cold', 10);
const OUT = path.resolve(opt('--out', path.join(here, 'results')));
const CHROME = process.env.SWIFTBROWSE_CHROMIUM || JSON.parse(execFileSync(SB_BIN, ['doctor', '--json']).toString())
  .checks.find((c) => c.check === 'chromium').detail.split(' at ').pop();
const PW_MCP = path.join(here, 'node_modules/@playwright/mcp/cli.js');
const PW_MCP_VERSION = JSON.parse(fs.readFileSync(path.join(here, 'node_modules/@playwright/mcp/package.json'))).version;
const WORK = fs.mkdtempSync(path.join(os.tmpdir(), 'agents-bench-'));

// ---------- pages ----------
const formHtml = fs.readFileSync(path.join(here, '../crates/chromium/tests/fixtures/form.html'), 'utf8');
const rows = Array.from({ length: 400 }, (_, i) =>
  `<tr><td>Row ${i}</td><td>Item description number ${i}</td><td><input aria-label="Qty ${i}" value="${i % 7}"></td>` +
  `<td><button aria-label="Edit row ${i}" onclick="document.getElementById('st').textContent='Editing row ${i}'">Edit</button> <a href="#r${i}">Details</a></td></tr>`).join('');
const largeHtml = `<!doctype html><title>Inventory</title><h1>Inventory</h1><p id="st" role="status"></p><table>${rows}</table>`;
const server = http.createServer((req, res) => {
  const body = req.url.startsWith('/form') ? formHtml : req.url.startsWith('/large') ? largeHtml : null;
  res.writeHead(body ? 200 : 404, { 'content-type': 'text/html; charset=utf-8' });
  res.end(body || 'not found');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const BASE = `http://127.0.0.1:${server.address().port}`;

// ---------- generic MCP client ----------
class Mcp {
  static async start(cmd, argv) {
    const m = new Mcp();
    m.proc = spawn(cmd, argv, { stdio: ['pipe', 'pipe', 'ignore'], cwd: WORK });
    m.waiting = new Map();
    readline.createInterface({ input: m.proc.stdout }).on('line', (l) => {
      const msg = JSON.parse(l);
      if (msg.id && m.waiting.has(msg.id)) { m.waiting.get(msg.id)(msg); m.waiting.delete(msg.id); }
    });
    m.id = 0;
    await m.rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'agents-bench', version: '1' } });
    m.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
    m.calls = 0; m.bytes = 0;
    return m;
  }
  rpc(method, params) {
    return new Promise((resolve, reject) => {
      const id = ++this.id;
      const timer = setTimeout(() => reject(new Error(`${method} ${params?.name ?? ''} timed out`)), 60000);
      this.waiting.set(id, (m) => { clearTimeout(timer); resolve(m); });
      this.proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n');
    });
  }
  // Returns the text the agent would read; counts calls and bytes.
  async tool(name, a = {}) {
    const r = (await this.rpc('tools/call', { name, arguments: a })).result;
    const text = r.content.filter((c) => c.type === 'text').map((c) => c.text).join('\n');
    this.calls++; this.bytes += Buffer.byteLength(text);
    return { text, isError: !!r.isError };
  }
  resetCounters() { this.calls = 0; this.bytes = 0; }
  // Close stdin like an MCP client does; SIGTERM only if the server ignores that.
  async stop() {
    const exited = this.proc.exitCode !== null ? Promise.resolve() : new Promise((r) => this.proc.once('exit', r));
    this.proc.stdin.end();
    const timer = setTimeout(() => this.proc.kill('SIGTERM'), 5000);
    await exited;
    clearTimeout(timer);
  }
}

function refFrom(snapshot, role, name, style) {
  const q = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const re = style === 'pw' ? new RegExp(`${role} "${q}"[^\\n]*\\[ref=([^\\]]+)\\]`) : new RegExp(`^${role} "${q}" \\[(e\\d+)\\]`, 'm');
  const m = snapshot.match(re);
  if (!m) throw new Error(`no ${role} "${name}" in snapshot: ${snapshot.slice(0, 400)}`);
  return m[1];
}

// ---------- contenders ----------
const contenders = {
  swiftbrowse: {
    label: 'SwiftBrowse MCP',
    start: () => Mcp.start(SB_BIN, ['mcp', '--no-approval', '--evidence-dir', path.join(WORK, 'ev')]),
    async navigate(m, url) { return m.tool('navigate', { url }); },
    async formTask(m) {
      await m.tool('navigate', { url: `${BASE}/form` });
      const snap = (await m.tool('get_page_snapshot')).text;
      const ref = (role, name) => ({ by: 'ref', ref: refFrom(snap, role, name, 'sb') });
      await m.tool('fill', { target: ref('textbox', 'Email'), value: 'ada@example.com' });
      await m.tool('fill', { target: ref('textbox', 'Password'), value: 'secret' });
      await m.tool('select_option', { target: ref('combobox', 'Country'), option: 'Netherlands' });
      await m.tool('click', { target: ref('checkbox', 'I agree to the terms') });
      const r = await m.tool('click', { target: ref('button', 'Sign up'), expect: { type: 'text_visible', text: 'Welcome, ada@example.com from nl' } });
      return !r.isError;
    },
    async largeTask(m) {
      await m.tool('navigate', { url: `${BASE}/large` });
      await m.tool('get_page_snapshot');
      // Row 250 is beyond the bounded snapshot, which says to locate it directly.
      const r = await m.tool('click', {
        target: { by: 'role', role: 'button', name: 'Edit row 250', exact: true },
        expect: { type: 'text_visible', text: 'Editing row 250' },
      });
      return !r.isError;
    },
    // Without looking first: act by role and name, confirm in the same call.
    async largeDirect(m) {
      await m.tool('navigate', { url: `${BASE}/large` });
      const r = await m.tool('click', {
        target: { by: 'role', role: 'button', name: 'Edit row 250', exact: true },
        expect: { type: 'text_visible', text: 'Editing row 250' },
      });
      return !r.isError;
    },
  },
};
for (const [key, settle] of [['playwright_default', null], ['playwright_settle0', '0']]) {
  contenders[key] = {
    label: settle === null ? `Playwright MCP ${PW_MCP_VERSION} (defaults)` : `Playwright MCP ${PW_MCP_VERSION} (--timeout-settle 0)`,
    start: () => Mcp.start('node', [PW_MCP, '--headless', '--isolated', '--no-sandbox', '--executable-path', CHROME,
      '--viewport-size', '1280x800', '--output-dir', path.join(WORK, 'pw-out'), ...(settle === null ? [] : ['--timeout-settle', settle])]),
    async navigate(m, url) { return m.tool('browser_navigate', { url }); },
    async formTask(m) {
      await m.tool('browser_navigate', { url: `${BASE}/form` });
      const snap = (await m.tool('browser_snapshot')).text;
      const ref = (role, name) => refFrom(snap, role, name, 'pw');
      await m.tool('browser_type', { element: 'Email', target: ref('textbox', 'Email'), text: 'ada@example.com' });
      await m.tool('browser_type', { element: 'Password', target: ref('textbox', 'Password'), text: 'secret' });
      await m.tool('browser_select_option', { element: 'Country', target: ref('combobox', 'Country'), values: ['Netherlands'] });
      await m.tool('browser_click', { element: 'I agree', target: ref('checkbox', 'I agree to the terms') });
      await m.tool('browser_click', { element: 'Sign up', target: ref('button', 'Sign up') });
      const r = await m.tool('browser_wait_for', { text: 'Welcome, ada@example.com from nl' });
      return !r.isError && !/### Error/.test(r.text);
    },
    async largeTask(m) {
      await m.tool('browser_navigate', { url: `${BASE}/large` });
      const snap = (await m.tool('browser_snapshot')).text;
      await m.tool('browser_click', { element: 'Edit row 250', target: refFrom(snap, 'button', 'Edit row 250', 'pw') });
      const r = await m.tool('browser_wait_for', { text: 'Editing row 250' });
      return !r.isError && !/### Error/.test(r.text);
    },
    // Without looking first: Playwright MCP accepts a role selector as the target.
    async largeDirect(m) {
      await m.tool('browser_navigate', { url: `${BASE}/large` });
      const c = await m.tool('browser_click', { element: 'Edit row 250', target: 'role=button[name="Edit row 250"]' });
      if (c.isError || /### Error/.test(c.text)) return false;
      const r = await m.tool('browser_wait_for', { text: 'Editing row 250' });
      return !r.isError && !/### Error/.test(r.text);
    },
  };
}

// ---------- run ----------
const raw = {};
const rec = (who, task, sample) => ((raw[who] ??= {})[task] ??= []).push(sample);
const now = () => performance.now();

// Cold start, interleaved across contenders; first round discarded (disk cache).
const keys = Object.keys(contenders);
for (let i = 0; i < COLD + 1; i++) {
  for (const k of keys) {
    const c = contenders[k];
    const t = now();
    const m = await c.start();
    await c.navigate(m, `${BASE}/form`);
    if (i > 0) rec(k, 'cold_start', { ms: now() - t });
    await m.stop();
  }
}

for (const k of keys) {
  const c = contenders[k];
  const m = await c.start();
  for (const task of ['formTask', 'largeTask', 'largeDirect']) {
    for (let i = 0; i < ITER + 2; i++) {
      m.resetCounters();
      const t = now();
      let ok = false;
      try { ok = await c[task](m); } catch (e) { ok = false; if (process.env.BENCH_DEBUG) console.error(k, task, e.message); }
      const sample = { ms: now() - t, calls: m.calls, bytes: m.bytes, ok };
      if (i >= 2) rec(k, task, sample);
    }
  }
  await m.stop();
}
server.close();

// ---------- report ----------
const med = (xs) => { const s = [...xs].sort((a, b) => a - b); return s[Math.floor(s.length / 2)]; };
const p95 = (xs) => { const s = [...xs].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.floor(0.95 * s.length))]; };
const env = {
  date: new Date().toISOString(),
  cpu: `${os.cpus()[0]?.model} x${os.cpus().length}`,
  chromium: execFileSync(CHROME, ['--version']).toString().trim(),
  swiftbrowse: execFileSync(SB_BIN, ['--version']).toString().trim(),
  playwright_mcp: PW_MCP_VERSION,
  iterations: ITER, cold_runs: COLD,
};
let md = `# Agent tasks: SwiftBrowse MCP vs Playwright MCP\n\n${Object.entries(env).map(([k, v]) => `- **${k}**: ${v}`).join('\n')}\n\n`;
md += 'Both servers are driven over MCP stdio as an agent would. formTask and largeTask: snapshot, take refs from it, act, confirm the outcome. largeDirect: act by role and name without a snapshot, then confirm. Bytes = text returned into the agent context across all tool calls of the task.\n\n';
md += '| task | contender | success | p50 ms | p95 ms | tool calls | context bytes |\n|---|---|---:|---:|---:|---:|---:|\n';
const summary = {};
for (const task of ['formTask', 'largeTask', 'largeDirect', 'cold_start']) {
  for (const k of keys) {
    const xs = raw[k]?.[task] || [];
    const okRate = xs.every((x) => x.ok !== undefined) ? `${xs.filter((x) => x.ok).length}/${xs.length}` : '-';
    const row = { p50: +med(xs.map((x) => x.ms)).toFixed(1), p95: +p95(xs.map((x) => x.ms)).toFixed(1), calls: xs[0]?.calls, bytes: xs.length && xs[0].bytes !== undefined ? med(xs.map((x) => x.bytes)) : undefined, success: okRate };
    (summary[task] ??= {})[k] = row;
    md += `| ${task} | ${contenders[k].label} | ${row.success} | ${row.p50} | ${row.p95} | ${row.calls ?? '-'} | ${row.bytes ?? '-'} |\n`;
  }
}
fs.mkdirSync(OUT, { recursive: true });
const stamp = env.date.replace(/[:.]/g, '-');
fs.writeFileSync(path.join(OUT, `agents-${stamp}.json`), JSON.stringify({ env, summary, raw }, null, 2));
fs.writeFileSync(path.join(OUT, 'agents-latest.md'), md);
fs.rmSync(WORK, { recursive: true, force: true });
console.log(md);
