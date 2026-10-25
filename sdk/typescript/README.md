# swiftbrowse

**The AI browser for browser automation and end-to-end testing.** A native engine (Rust, driving Chromium over the DevTools Protocol) with one contract across a test runner, this TypeScript/JavaScript SDK, a Python SDK, and an MCP server for AI agents.

**📖 Documentation: https://steady-ember-82q7.here.now/**

```sh
npm install -D swiftbrowse
npx playwright install chromium chromium-headless-shell   # a Chromium for SwiftBrowse to drive
npx swiftbrowse doctor                                   # check what this machine can run
```

## Tests in code

```js
import { test, before, after } from 'node:test';
import { launch } from 'swiftbrowse';

let browser;
before(async () => { browser = await launch(); });
after(() => browser.close());

test('signs in', async () => {
  const page = await browser.newSession();      // isolated: own cookies, storage, tabs
  await page.navigate('http://localhost:3000/login');
  await page.fill('Email', 'ada@example.com');
  await page.click({ by: 'role', role: 'button', name: 'Sign in' },
    { expect: { type: 'url_contains', value: '/dashboard' } });
  await page.expect({ type: 'text_visible', text: 'Welcome, Ada' });
  await page.close();
});
```

- Every engine operation is a typed method (`navigate`, `click`, `fill`, `check`, `hover`, `selectOption`, `uploadFile`, `route`, `switchTab`, `switchFrame`, `saveStorageState`, …), with required arguments in order and the rest as options.
- A target is a string (accessible name, label, test id or text) or a locator (`role`, `label`, `test_id`, `text`, `css`, `xpath`). Several matches are an error, never a guess.
- Actions wait until the element is visible, enabled, not covered and not moving. `expect` assertions retry until they pass.
- A failed action or assertion throws `SwiftBrowseError`, so `node:test`, Vitest or Jest report it.
- Sessions attach to a warm background browser, so creating one takes milliseconds.

## Tests as JSON, with the built-in runner

```json
{
  "name": "sign in",
  "tests": [
    { "name": "signs in", "steps": [
      { "op": "navigate", "url": "login" },
      { "op": "fill", "target": "Email", "value": "ada@example.com" },
      { "op": "click", "target": "Sign in", "expect": { "type": "url_contains", "value": "/dashboard" } }
    ] }
  ]
}
```

```sh
npx swiftbrowse test --base-url http://localhost:3000/ -j 4 --retries 2 --html report.html --trace retain-on-failure
```

The runner supports parallel workers, retries and flaky detection, sharding, saved logins, network mocking, tabs, frames, uploads, traces with a live DOM, video, and JUnit/HTML reports. On the project's benchmark suite it runs 2–3× faster than Playwright Test.

## For AI agents

```json
{ "mcpServers": { "swiftbrowse": { "command": "npx", "args": ["-y", "swiftbrowse", "mcp"] } } }
```

## Learn more

- [Getting started](https://steady-ember-82q7.here.now/getting-started.html)
- [TypeScript and JavaScript SDK](https://steady-ember-82q7.here.now/typescript.html)
- [Test files](https://steady-ember-82q7.here.now/writing-tests.html) and [the test runner](https://steady-ember-82q7.here.now/test-runner.html)
- [Operations reference](https://steady-ember-82q7.here.now/operations.html)
- [Issues](https://github.com/swiftbrowse/swiftbrowse/issues) and [source](https://github.com/swiftbrowse/swiftbrowse)

The native binary comes from a platform package (`swiftbrowse-linux-x64`, `-linux-arm64`, `-darwin-arm64`, `-darwin-x64` or `-windows-x64`) that npm installs automatically. Set `SWIFTBROWSE_BIN` to use another binary. Apache-2.0.
