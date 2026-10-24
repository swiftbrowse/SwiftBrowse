#!/usr/bin/env node
// `npx swiftbrowse …`: runs the native binary for this platform with the same
// arguments, stdio and exit code.

import { spawn } from 'node:child_process';

import { findBinary, platformPackage } from '../src/binary.js';

const bin = findBinary();
const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' });
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => child.kill(signal));
}
child.on('error', (err) => {
  console.error(err.code === 'ENOENT'
    ? `swiftbrowse: no binary for ${process.platform}-${process.arch} (looked for the ${platformPackage()} package, then PATH). Set SWIFTBROWSE_BIN to a swiftbrowse binary.`
    : `swiftbrowse: ${err.message}`);
  process.exit(127);
});
child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 1);
});
