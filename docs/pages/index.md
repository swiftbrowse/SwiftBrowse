# SwiftBrowse

The AI browser for browser automation and end-to-end testing. A native engine (Rust, driving Chromium over the DevTools Protocol) with one contract across a test runner, TypeScript and Python SDKs, and an MCP server for AI agents.

```sh
npm install -g swiftbrowse          # or: pip install swiftbrowse
npx playwright install chromium chromium-headless-shell
swiftbrowse doctor
```

## What it is

SwiftBrowse does one job three ways, with the same operations and the same results everywhere:

- **An end-to-end test runner.** `swiftbrowse test` runs tests written as JSON files: parallel workers, retries and flaky detection, sharding, saved logins, network mocking, traces, video, and JUnit/HTML reports.
- **Tests in code.** The [TypeScript](typescript.md) and [Python](python.md) SDKs expose every operation as a typed method, for use with `node:test`, Vitest, Jest, pytest or unittest.
- **A browser for AI agents.** `swiftbrowse mcp` serves the Model Context Protocol. A flow an agent works out over MCP can be saved as a test unchanged.

A step in a test file, an MCP tool call and an SDK method call are the same request:

```json
{ "op": "click", "target": "Sign in", "expect": { "type": "url_contains", "value": "/dashboard" } }
```

```ts
await page.click('Sign in', { expect: { type: 'url_contains', value: '/dashboard' } });
```

## Why use it

- **Fast.** On the same 29-test suite, `swiftbrowse test` finishes in a third to a half of Playwright Test's time. New sessions attach to a warm browser in about 50 ms. See [Benchmarks](benchmarks.md).
- **Reliable actions.** Every click waits until its target is visible, enabled, not covered and not moving. The click is checked again as the mouse goes down and up, so a layout shift can't send it to the wrong element.
- **Honest results.** An action can verify its own outcome (`expect`). Ambiguous locators are errors, never silent guesses. Every failure says what kind it is and whether retrying could help.
- **Evidence on failure.** You get a screenshot, the page as text, console errors and failed requests. Optionally there's a trace with a live, inspectable copy of the page at every step, and a video.
- **Small footprint.** A single native binary, with no Node runtime needed to run JSON tests. The Python SDK uses the standard library only; the TypeScript SDK has no dependencies.

## Where to go next

<div class="cards">
<a href="getting-started.html"><b>Getting started</b><span>Install, run your first test, try the SDK and MCP.</span></a>
<a href="writing-tests.html"><b>Test files</b><span>Steps, hooks, variables, tags, setup files.</span></a>
<a href="locators.html"><b>Targets and locators</b><span>How SwiftBrowse finds elements.</span></a>
<a href="assertions.html"><b>Assertions</b><span>Auto-waiting conditions for URL, text, elements and more.</span></a>
<a href="network.html"><b>Network mocking</b><span>Fulfill, fail, delay or watch requests.</span></a>
<a href="auth.html"><b>Saved logins</b><span>Log in once, start every test signed in.</span></a>
<a href="debugging.html"><b>Traces, video and reports</b><span>See exactly what happened when a test failed.</span></a>
<a href="ci.html"><b>Continuous integration</b><span>GitHub Actions, sharding, JUnit.</span></a>
<a href="operations.html"><b>Operations reference</b><span>Every operation and argument, generated from the engine.</span></a>
</div>

## Platforms

Prebuilt binaries are available for Linux (x64 and arm64, glibc 2.35+), macOS (Apple Silicon and Intel) and Windows x64. They ship inside the npm and PyPI packages and as archives on the [GitHub releases page](https://github.com/swiftbrowse/SwiftBrowse/releases). SwiftBrowse drives Chromium; other browser engines are not supported yet (see [Limitations](limitations.md)).

> SwiftBrowse is young (version {{version}}). If something breaks or the docs leave you guessing, please [open an issue](https://github.com/swiftbrowse/SwiftBrowse/issues). Include the output of `swiftbrowse doctor`.
