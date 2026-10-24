// SwiftBrowse SDK for JavaScript and TypeScript: drives the `swiftbrowse`
// binary over its SDK protocol (JSON lines on stdin/stdout). No dependencies.

import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

import { findBinary } from './binary.js';
import { OPS, PROTOCOL } from './ops.generated.js';

/** Thrown when an operation fails (`result.ok === false`) and `throwOnError` is on. */
export class SwiftBrowseError extends Error {
  constructor(result) {
    const e = result.error || {};
    super(`${result.op} failed (${e.kind || 'error'}): ${e.message || 'unknown error'}${result.notes?.length ? ` [${result.notes.join('; ')}]` : ''}`);
    this.name = 'SwiftBrowseError';
    this.kind = e.kind;
    this.result = result;
  }
}

/** Starts a `swiftbrowse sdk` process. Sessions attach to the warm browser daemon. */
export async function launch(options = {}) {
  const bin = findBinary(options.executable);
  const args = ['sdk'];
  if (options.testDefaults !== false) args.push('--test-defaults');
  if (options.headed) args.push('--headed');
  if (options.noDaemon) args.push('--no-daemon');
  if (options.chromium) args.push('--chromium', options.chromium);
  if (options.evidenceDir) args.push('--evidence-dir', options.evidenceDir);
  if (options.timeoutMs) args.push('--timeout-ms', String(options.timeoutMs));
  for (const h of options.allowHosts || []) args.push('--allow-host', h);
  for (const h of options.blockHosts || []) args.push('--block-host', h);
  for (const d of options.uploadDirs || []) args.push('--allow-upload-dir', d);
  if (options.allowFileUrls) args.push('--allow-file-urls');
  const proc = spawn(bin, args, { stdio: ['pipe', 'pipe', 'inherit'] });
  const browser = new Browser(proc, options);
  const hello = await browser.request('hello');
  if (hello.protocol !== PROTOCOL) {
    await browser.close();
    throw new Error(`swiftbrowse binary speaks SDK protocol ${hello.protocol}, this SDK expects ${PROTOCOL}; update one of them`);
  }
  browser.version = hello.version;
  return browser;
}

export class Browser {
  constructor(proc, options) {
    this.proc = proc;
    this.options = options;
    this.nextId = 0;
    this.pending = new Map();
    this.exited = new Promise((resolve) => proc.on('exit', resolve));
    proc.on('error', (err) => this.failAll(err));
    proc.on('exit', (code) => this.failAll(new Error(`swiftbrowse exited (code ${code})`)));
    createInterface({ input: proc.stdout }).on('line', (line) => {
      let msg;
      try { msg = JSON.parse(line); } catch { return; }
      const waiter = this.pending.get(msg.id);
      if (!waiter) return;
      this.pending.delete(msg.id);
      if (msg.error) waiter.reject(new Error(msg.error.message));
      else waiter.resolve(msg.result);
    });
  }

  failAll(err) {
    for (const w of this.pending.values()) w.reject(err);
    this.pending.clear();
  }

  /** Sends one protocol request. */
  request(method, params = {}) {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.proc.stdin.write(`${JSON.stringify({ id, method, params })}\n`, (err) => {
        if (err) { this.pending.delete(id); reject(err); }
      });
    });
  }

  /** An isolated session: its own cookies, storage and tabs. */
  async newSession(options = {}) {
    const info = await this.request('create_session', {
      headless: options.headless,
      viewport: options.viewport,
    });
    return new Session(this, info, options.throwOnError ?? this.options.throwOnError ?? true);
  }

  /** Closes every session and stops the process. */
  async close() {
    if (this.proc.exitCode === null) {
      this.proc.stdin.end();
      const timer = setTimeout(() => this.proc.kill(), 5000);
      await this.exited;
      clearTimeout(timer);
    }
  }
}

export class Session {
  constructor(browser, info, throwOnError) {
    this.browser = browser;
    this.id = info.session_id;
    this.info = info;
    this.throwOnError = throwOnError;
  }

  /** Runs any operation by name with its arguments object. */
  async run(op, args = {}) {
    const result = await this.browser.request('execute', { session: this.id, op, args });
    if (!result.ok && this.throwOnError) throw new SwiftBrowseError(result);
    return result;
  }

  /** Auto-waiting assertion: passes as soon as every condition holds, fails after `timeoutMs` (5 s). */
  expect(condition, { timeoutMs = 5000, name } = {}) {
    const conditions = Array.isArray(condition) ? condition : [condition];
    return this.run('assert_page_state', { conditions, timeout_ms: timeoutMs, ...(name ? { name } : {}) });
  }

  /** The page as compact text: URL, title, headings, interactive elements with refs. */
  async text() {
    return (await this.run('get_page_snapshot', { format: 'text' })).data.text;
  }

  async close() {
    await this.browser.request('close_session', { session: this.id });
  }
}

// One method per engine operation: required arguments in order, then an options object.
for (const [op, method, positional] of OPS) {
  if (method in Session.prototype) continue;
  Session.prototype[method] = function (...values) {
    const args = {};
    positional.forEach((name, i) => { args[name] = values[i]; });
    Object.assign(args, values[positional.length] || {});
    return this.run(op, args);
  };
}
