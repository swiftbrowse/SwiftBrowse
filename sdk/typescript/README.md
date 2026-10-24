# swiftbrowse

**The AI browser for browser automation and end-to-end testing.** A native engine (Rust, Chromium over the DevTools Protocol) with one contract across a test runner, an MCP server for AI agents, and this SDK.

```sh
npm install swiftbrowse
npx playwright install chromium chromium-headless-shell   # a Chromium for SwiftBrowse to drive
npx swiftbrowse doctor                                   # what this machine can run
```

## Tests in code

```js
import { test } from 'node:test';
import { launch } from 'swiftbrowse';

test('signs in', async () => {
  const browser = await launch();
  const page = await browser.newSession();
  await page.navigate('http://localhost:3000/login');
  await page.fill('Email', 'ada@example.com');
  await page.click({ by: 'role', role: 'button', name: 'Sign in' }, { expect: { type: 'url_contains', value: '/dashboard' } });
  await page.expect({ type: 'text_visible', text: 'Welcome, Ada' });
  await browser.close();
});
```

Every engine operation is a typed method (`navigate`, `click`, `fill`, `check`, `hover`, `uploadFile`, `route`, `switchTab`, `switchFrame`, `saveStorageState`, …): required arguments in order, the rest as options. Targets are a string (accessible name, label, test id or text) or a locator (`role`, `label`, `test_id`, `text`, `css`, `xpath`). A failed action or `expect` throws `SwiftBrowseError`, so `node:test`, Vitest or Jest report it. Sessions are isolated and run concurrently.

## The CLI

`npx swiftbrowse test` runs JSON test files with parallel workers, retries, traces, video and JUnit/HTML reports; `npx swiftbrowse mcp` serves the Model Context Protocol for agents. See the [main README](https://github.com/swiftbrowse/swiftbrowse#readme).

The native binary comes from a platform package (`swiftbrowse-linux-x64`, `-linux-arm64`, `-darwin-arm64`, `-darwin-x64`, `-win32-x64`) installed automatically; set `SWIFTBROWSE_BIN` to use another.
