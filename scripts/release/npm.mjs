// Assembles the npm packages for a release: one platform package (its `npm` name in
// platforms.json, e.g. `swiftbrowse-linux-x64`)
// per built binary, and `swiftbrowse` (the SDK and the `swiftbrowse` command)
// with those packages as optional dependencies pinned to the same version.
//
// Usage: node scripts/release/npm.mjs <version> <binaries-dir> <out-dir>
// <binaries-dir>/<platform-id>/<exe> as produced by the release workflow.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const [version, binaries, out] = process.argv.slice(2);
if (!version || !binaries || !out) throw new Error('usage: npm.mjs <version> <binaries-dir> <out-dir>');
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const platforms = JSON.parse(fs.readFileSync(path.join(root, 'scripts/release/platforms.json'), 'utf8'));
const repository = { type: 'git', url: 'git+https://github.com/swiftbrowse/swiftbrowse.git' };
fs.rmSync(out, { recursive: true, force: true });

const built = [];
for (const p of platforms) {
  const exe = path.join(binaries, p.id, p.exe);
  if (!fs.existsSync(exe)) {
    console.warn(`no binary for ${p.id}; skipping its package`);
    continue;
  }
  const dir = path.join(out, p.npm);
  fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });
  fs.copyFileSync(exe, path.join(dir, 'bin', p.exe));
  fs.chmodSync(path.join(dir, 'bin', p.exe), 0o755);
  fs.copyFileSync(path.join(root, 'LICENSE'), path.join(dir, 'LICENSE'));
  fs.writeFileSync(path.join(dir, 'README.md'), `# ${p.npm}\n\nThe \`swiftbrowse\` binary for ${p.os} ${p.cpu}. Install [\`swiftbrowse\`](https://www.npmjs.com/package/swiftbrowse) instead; it picks this package up automatically.\n`);
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({
    name: p.npm,
    version,
    description: `The swiftbrowse binary for ${p.os} ${p.cpu}`,
    license: 'Apache-2.0',
    repository,
    os: [p.os],
    cpu: [p.cpu],
    files: ['bin', 'LICENSE', 'README.md'],
  }, null, 2) + '\n');
  built.push(p.npm);
}

const main = path.join(out, 'swiftbrowse');
fs.cpSync(path.join(root, 'sdk/typescript'), main, {
  recursive: true,
  filter: (src) => !/[/\\](node_modules|test|scripts)([/\\]|$)/.test(src.slice(path.join(root, 'sdk/typescript').length)),
});
fs.copyFileSync(path.join(root, 'LICENSE'), path.join(main, 'LICENSE'));
const pkg = JSON.parse(fs.readFileSync(path.join(main, 'package.json'), 'utf8'));
pkg.version = version;
pkg.optionalDependencies = Object.fromEntries(built.map((name) => [name, version]));
pkg.files = ['src', 'bin', 'LICENSE', 'README.md'];
delete pkg.scripts;
fs.writeFileSync(path.join(main, 'package.json'), JSON.stringify(pkg, null, 2) + '\n');
console.log(`npm packages for ${version}: swiftbrowse + ${built.join(', ')}`);
