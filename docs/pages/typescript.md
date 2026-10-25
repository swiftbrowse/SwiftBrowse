# TypeScript and JavaScript

```sh
npm install -D swiftbrowse
```

The `swiftbrowse` package is an ES module with TypeScript types and no dependencies. It runs the native `swiftbrowse` binary as a child process and talks to it over JSON lines. Use it with any test framework: `node:test`, Vitest, Jest, Mocha.

## A test

```js
import { test, before, after } from 'node:test';
import { launch } from 'swiftbrowse';

let browser;
before(async () => { browser = await launch(); });
after(() => browser.close());

test('adds a todo', async () => {
  const page = await browser.newSession();
  await page.navigate('http://localhost:3000/');
  await page.fill('New todo', 'Write docs');
  await page.pressKey('Enter', { expect: { type: 'text_visible', text: 'Write docs' } });
  await page.expect([
    { type: 'count', target: { by: 'role', role: 'listitem' }, count: 1 },
    { type: 'no_console_errors' },
  ]);
  await page.close();
});
```

```sh
node --test
```

Start one `launch()` per test file (or per worker) and one `newSession()` per test. A session is isolated: its own cookies, storage and tabs. Sessions run concurrently, and each attaches to the warm browser daemon, so creating one takes milliseconds.

## API

### `launch(options?) → Promise<Browser>`

Starts a `swiftbrowse sdk` process.

| Option | Default | Meaning |
|---|---|---|
| `testDefaults` | `true` | No approval prompts for "Delete" or "Pay" clicks, `evaluate` allowed, uploads from the working directory. Set it to `false` for the agent-safe policy. |
| `headed` | `false` | Show browser windows. |
| `noDaemon` | `false` | Launch a browser for this process instead of attaching to the warm daemon. |
| `chromium` | discovered | Path to a Chromium binary. |
| `evidenceDir` | temp directory | Where screenshots, videos and saved states go. |
| `timeoutMs` | 10000 | How long actions wait for their target, in ms. |
| `allowHosts` / `blockHosts` | | Restrict navigation (`*.example.com` patterns allowed). |
| `uploadDirs` | | Extra directories `uploadFile` may read from. |
| `allowFileUrls` | `false` | Permit `file://` navigation. |
| `throwOnError` | `true` | Throw `SwiftBrowseError` when an operation fails. |
| `executable` | found | Path to the `swiftbrowse` binary. |

### `browser.newSession({ headless?, viewport?, throwOnError? }) → Promise<Session>`

`viewport` is `[width, height]`; the default is `[1280, 800]`.

### `browser.close()`

Closes every session and stops the process. Call it in `after`/`afterAll`.

### Session methods

Every [operation](operations.md) is a camelCase method: required arguments in order, then an options object whose keys are the operation's other arguments in snake_case.

```js
await page.navigate(url, { wait_until: 'network_idle' });
await page.click('Save', { expect: { type: 'text_visible', text: 'Saved' }, timeout_ms: 10000 });
await page.fill({ by: 'label', text: 'Email' }, 'ada@example.com');
await page.selectOption('Country', 'Norway');
await page.check('I agree');
await page.uploadFile('Documents', ['fixtures/notes.txt']);
await page.route({ url: '**/api/todos', json: [] });
await page.switchTab({ url_contains: 'checkout' });
await page.switchFrame({ name: 'payment' });
await page.saveStorageState('member');
const shot = await page.getScreenshot({ full_page: true });
```

Plus three helpers:

- `page.expect(condition | condition[], { timeoutMs = 5000, name })`: an auto-waiting assertion. See [Assertions](assertions.md).
- `page.text()`: the page as compact text (URL, title, headings, interactive elements).
- `page.run(op, args)`: any operation by name.

### Results and errors

Every method resolves to an [`ActionResult`](results.md). If the operation failed, it throws a `SwiftBrowseError` instead, unless `throwOnError` is `false`. The error carries `kind` (for example `not_found`, `ambiguous` or `verification_failed`) and the full `result`, and its message includes any notes. Test frameworks report it like any failed assertion.

```js
import { SwiftBrowseError } from 'swiftbrowse';
try {
  await page.click('Missing', { timeout_ms: 500 });
} catch (e) {
  if (e instanceof SwiftBrowseError && e.kind === 'not_found') { /* … */ }
}
```

## Types

Methods, options, `Target`, `Condition` and `ActionResult` are typed. The types are generated from the engine's schemas, so editors autocomplete every argument and condition, and CI fails if they drift from the engine.

## The binary

The SDK uses, in order: the `executable` option, `$SWIFTBROWSE_BIN`, the binary from the platform package npm installed (`swiftbrowse-<platform>`), then `swiftbrowse` on `PATH`. The package also installs the CLI: `npx swiftbrowse test`.

## Parallelism with `node:test`

`node --test` runs test files in parallel processes. Tests inside a file run in order unless you use `{ concurrency: true }`. Each file's `launch()` gets its own sessions on the shared warm browsers.
