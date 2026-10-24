// Shared setup for the SDK-written suite: one `swiftbrowse sdk` process per test
// file (node:test runs files in parallel processes), one session per test.
import path from 'node:path';
import { after, before, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

import { launch } from '../../../sdk/typescript/src/index.js';

export const BASE = process.env.BASE_URL;
export const FIXTURES = path.join(path.dirname(fileURLToPath(import.meta.url)), '../../../examples/e2e/tests/fixtures');
const REPEAT = Number(process.env.REPEAT || 1);

// Launched on first use, whichever hook or test needs it first.
let launching;
const browser = () => (launching ??= launch({ uploadDirs: [FIXTURES], evidenceDir: process.env.SB_EVIDENCE }));
before(() => browser());
after(async () => (await launching)?.close());

/** Registers `fn` as a test that gets a fresh session (repeated REPEAT times). */
export function it(name, fn, setup) {
  for (let i = 0; i < REPEAT; i++) {
    test(i ? `${name} (repeat ${i + 1})` : name, async () => {
      const page = await (await browser()).newSession();
      try {
        if (setup) await setup(page);
        await fn(page);
      } finally {
        await page.close();
      }
    });
  }
}

export const sessionOf = async () => (await browser()).newSession();
export const role = (r, name, extra = {}) => ({ by: 'role', role: r, name, ...extra });

/** A file's tests; WORKERS > 1 runs them in parallel (default: in order, files in parallel). */
export const suite = (name, fn) => describe(name, { concurrency: Number(process.env.WORKERS || 1) }, fn);
