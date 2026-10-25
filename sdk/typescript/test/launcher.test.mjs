// The `swiftbrowse` launcher without a platform package: under npx, the
// `swiftbrowse` on PATH is the launcher itself, which must fail with a clear
// message instead of starting itself forever.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const launcher = path.join(path.dirname(fileURLToPath(import.meta.url)), '../bin/swiftbrowse.js');

test('a launcher that finds itself on PATH stops with a clear error', { skip: process.platform === 'win32' }, () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'sb-launcher-'));
  fs.symlinkSync(launcher, path.join(dir, 'swiftbrowse'));
  const env = { ...process.env, PATH: `${dir}${path.delimiter}${process.env.PATH}` };
  delete env.SWIFTBROWSE_BIN;
  const r = spawnSync(process.execPath, [launcher, '--version'], { env, encoding: 'utf8', timeout: 10000 });
  fs.rmSync(dir, { recursive: true, force: true });
  assert.equal(r.error, undefined, 'finished in time');
  assert.equal(r.status, 127);
  assert.match(r.stderr, /package is not installed/);
});
