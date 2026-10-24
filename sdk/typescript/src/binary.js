// Finds the `swiftbrowse` binary: an explicit path, $SWIFTBROWSE_BIN, the
// package built for this platform (installed as an optional dependency), then PATH.

import { createRequire } from 'node:module';
import path from 'node:path';

const require = createRequire(import.meta.url);

/** npm package carrying the binary for this platform, e.g. `swiftbrowse-linux-x64`. */
// Windows' package is `swiftbrowse-windows-x64` (npm refused the name with `win32`).
export const platformPackage = () =>
  `swiftbrowse-${process.platform === 'win32' ? 'windows' : process.platform}-${process.arch}`;

export function findBinary(explicit) {
  if (explicit) return explicit;
  if (process.env.SWIFTBROWSE_BIN) return process.env.SWIFTBROWSE_BIN;
  const exe = process.platform === 'win32' ? 'swiftbrowse.exe' : 'swiftbrowse';
  try {
    return path.join(path.dirname(require.resolve(`${platformPackage()}/package.json`)), 'bin', exe);
  } catch {
    return 'swiftbrowse';
  }
}
