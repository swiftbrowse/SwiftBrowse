#!/usr/bin/env node
// `npx swiftbrowse …`: runs the native binary for this platform with the same
// arguments, stdio and exit code.

import { spawn } from 'node:child_process';

import { findBinary, platformPackage } from '../src/binary.js';

// Set when this launcher fell back to `swiftbrowse` on PATH. Seeing it here means
// PATH led back to a launcher (this one, under npx), which would start itself forever.
const LOOP = 'SWIFTBROWSE_LAUNCHER_FALLBACK';
const missing = `swiftbrowse: no binary for ${process.platform}-${process.arch}: the ${platformPackage()} package is not installed. Reinstall swiftbrowse (npm installs it as an optional dependency; it can be missing right after a release while npm catches up), or set SWIFTBROWSE_BIN to a swiftbrowse binary.`;
if (process.env[LOOP]) {
  console.error(missing);
  process.exit(127);
}

const bin = findBinary();
const env = bin === 'swiftbrowse' ? { ...process.env, [LOOP]: '1' } : process.env;
const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit', env });
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => child.kill(signal));
}
child.on('error', (err) => {
  console.error(err.code === 'ENOENT' ? missing : `swiftbrowse: ${err.message}`);
  process.exit(127);
});
child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 1);
});
