# SwiftBrowse

**The AI browser for browser automation and end-to-end testing.** One contract across MCP, CLI and the TypeScript and Python SDKs: agents drive it through MCP, and the same steps run as CI tests, written as JSON flows or in code. Rust core, Chromium backend over the DevTools Protocol.

> **Status: Phase 1 of the product specification roadmap, plus the MCP server and CLI from Phase 4.** The cloud/headless Chromium path works and is tested. The TypeScript and Python SDKs work against the same engine. The Tauri/Wry desktop shell is **not started**. See [What exists](#what-exists).

## Quick start

```sh
npm install swiftbrowse        # or: pip install swiftbrowse — both ship the native binary
npx playwright install chromium chromium-headless-shell   # a Chromium to drive
npx swiftbrowse doctor         # what this machine can run
npx swiftbrowse test           # run *.test.json files
npx swiftbrowse mcp            # MCP server on stdio, for agents
```

Prebuilt for Linux (x64, arm64; glibc 2.35+), macOS (Apple Silicon, Intel) and Windows x64; archives are on the GitHub releases page. From source: `cargo build --release` (Rust 1.89+).

`doctor` finds Chromium via `SWIFTBROWSE_CHROMIUM`, then Playwright-managed builds (`npx playwright install chromium`), then standard install locations and `PATH`. Headless sessions prefer Chromium's headless shell (`npx playwright install chromium-headless-shell`), which starts about 3× faster. On Linux and macOS, SwiftBrowse talks to the browser over a pipe, so no local debugging port is opened; set `SWIFTBROWSE_TRANSPORT=websocket` to use a port instead.

MCP client config:

```json
{ "mcpServers": { "swiftbrowse": { "command": "/path/to/swiftbrowse", "args": ["mcp"] } } }
```

## Warm-browser daemon (Linux, macOS, Windows)

The first headless session starts a background `swiftbrowse daemon` holding one Chromium. That first start costs the same as before (~160 ms to a loaded page). After that, every new session (a new CLI run, a new MCP server, `session_create`) attaches to the warm browser instead of launching one: **~52 ms from process start to a loaded page.**

- **Spare tab:** the daemon always keeps a spare tab ready, in a fresh browser context with its renderer booted and the session already set up (events, network, helper script, viewport). A new session takes it and navigates immediately, and the daemon prepares the next spare.
- **Isolation:** one browser context per session, with separate cookies, storage and cache. Contexts are disposed when the session closes or its process disconnects. A client can only use the contexts, tabs and sessions it created or claimed; browser-wide commands (`Browser.close`, `Target.getTargets`, …) are refused.
- **Access:** on Linux and macOS, a Unix socket in a `0700` directory (`$XDG_RUNTIME_DIR/swiftbrowse/` or `/tmp/swiftbrowse-<uid>/`), with the peer's user id checked, and the browser on a pipe: no TCP port at all. On Windows, a named pipe that refuses remote clients; the daemon reaches its browser over a localhost DevTools WebSocket, and a job object kills the browser with the daemon. Everywhere, a client must first present a random token stored in the user's private state directory, or it is dropped.
- **Lifetime:** the daemon exits after 10 idle minutes, when its browser dies, or on `swiftbrowse daemon stop`; the next session starts a fresh one. The socket name includes the version, so clients never mix builds.
- **Trade-off:** sessions share one browser *process* and are separated by browser context, not by process. The capability report says so for daemon sessions. Use `--no-daemon` (or `SWIFTBROWSE_DAEMON=off`) for one browser process per session. Headed sessions never use the daemon.

```sh
swiftbrowse daemon status    # running?, sessions served, spare ready
swiftbrowse daemon stop
```

## End-to-end testing

`swiftbrowse test` is a test runner. It finds every `*.test.json` / `*.spec.json`, runs the tests in parallel (each in its own isolated browser context), retries failures, and writes list, dot, JSON, JUnit and HTML reports. A failing test keeps a screenshot, a text snapshot of the page, and the console errors and failed requests it saw.

```sh
cd examples/e2e
swiftbrowse test tests --serve site            # 12 tests against the demo app
swiftbrowse test -j 4 --retries 2 --junit junit.xml --html report.html
swiftbrowse test -g @smoke --shard 1/3         # filter by title/tag, split across CI machines
swiftbrowse test --web-server "npm run dev" --web-server-url http://localhost:3000
```

A test file (see [`examples/e2e/tests`](examples/e2e/tests)):

```json
{
  "name": "sign in",
  "vars": { "email": "ada@example.com" },
  "before_each": [ { "op": "navigate", "url": "login.html" } ],
  "tests": [
    { "name": "signs in", "tags": ["smoke"], "steps": [
      { "op": "fill", "target": "Email", "value": "${email}" },
      { "op": "fill", "target": "Password", "value": "${env.TEST_PASSWORD}" },
      { "op": "click", "target": { "by": "role", "role": "button", "name": "Sign in" },
        "expect": { "type": "url_contains", "value": "dashboard" } },
      { "op": "expect", "condition": { "type": "text_equals", "target": { "by": "test_id", "id": "hello" }, "text": "Welcome, ${email}" } }
    ] }
  ]
}
```

- **Steps** are the same operations as the MCP tools and `run`: `navigate`, `click`, `fill`, `type_text`, `press_key`, `select_option`, `check`, `hover`, `upload_file`, `go_back`, `go_forward`, `reload`, `evaluate`, `wait_for`, `assert_page_state`, `get_screenshot`, network mocking (`route` by URL glob or regex and method: fulfill with status/body/JSON/headers and optional delay, abort with a network error, or let through; `times`; `unroute`, `list_routes`, `get_requests`), tabs (`new_tab`, `list_tabs`, `switch_tab` which waits for popups, `close_tab`) and frames (`list_frames`, `switch_frame` into same-site, cross-site or nested iframes), … A flow an agent worked out over MCP can be saved as a test unchanged.
- **Targets** are a string (`"Email"`: tries accessible name, label, test id, then text) or an explicit locator (`role`, `label`, `test_id`, `text`, `css`, `xpath`, with `nth`).
- **Assertions auto-wait**: `expect` (one `condition` or several `conditions`) and `assert_page_state` retry until they pass or `--expect-timeout-ms` (5 s) runs out. Conditions: URL, title, visible/hidden, count, text visible/absent, element text equals/contains, value, enabled, checked, no console errors, no failed requests, a request was sent (`request_seen`).
- **Saved logins**: a `*.setup.json` file runs before all other tests (if it fails, they are skipped). It logs in once and calls `save_storage_state` with a name; files that set `"storage_state": "<name>"` start every test from that state (cookies and localStorage, each test its own copy). The file uses Playwright's storageState format, is readable by your user only, and lives in `<output-dir>/.auth/`.
- **Files**: `base_url` (relative `navigate` URLs), `vars` (`${name}`, `${env.NAME}`, overridable with `--var`), `before_each`/`after_each` (`after_each` always runs), per-test `skip`, `only`, `tags`, `retries`, `timeout_ms`. A plain `run` script (`name` + `steps`) is a one-test file.
- **Traces and video**: `--trace on|retain-on-failure` records, after every step, a screenshot, the page as text and a **live DOM snapshot** into one self-contained `trace.html` per test: a step timeline; the page at each step as a screenshot or as a real, script-free page (current form values, shadow roots, the page's live CSS) that you can hover and click to inspect, with the step's target outlined and a suggested locator for any element, or open in your browser's devtools; expected vs observed; every request. `--video on|retain-on-failure` records a WebM of the run (ffmpeg: `npx playwright install ffmpeg`, or set `SWIFTBROWSE_FFMPEG`; without it the frames are kept as JPEGs). The HTML report links both. Over MCP: `start_video` / `stop_video`.
- **Runner options**: `-j/--workers`, `--retries` (pass on retry = flaky), `--repeat-each`, `-g/--grep`, `--grep-invert`, `--shard i/n`, `-x`/`--max-failures`, `--forbid-only`, `--list`, `--test-timeout-ms` (30 s), `--serve DIR`, `--web-server CMD`, `--output-dir`, `--reporter list|dot|json|quiet`, `--junit`, `--html`. Exit code 0 = passed, 1 = failures, 2 = bad config.
- **Reliability built into actions**: every click waits until the target is visible, enabled, not covered and not moving, and until the page's scripts have run. It is checked again *as the mouse goes down and up*: a click that would land elsewhere because the layout shifted is blocked and retried rather than lost.
- `upload_file` only reads from allowed directories: in the runner, the working directory and the test file's directory (paths are relative to the test file); over MCP, none unless `--allow-upload-dir DIR`.
- In the runner, `evaluate` is allowed and consequential clicks ("Delete", "Pay") need no approval: tests target test environments. Over MCP both stay off unless `--allow-evaluate` / `--no-approval` are passed.

**Not there yet** compared with Playwright Test: DOM snapshots are taken after each step (not before and after), and cross-origin stylesheets stay links; no visual comparison, no fixtures or reporters inside the SDKs (use your test framework's), Chromium only.

### Benchmark vs Playwright Test

The same 29 tests three ways: JSON flows run by `swiftbrowse test` ([`examples/e2e/tests`](examples/e2e/tests)), hand-written TypeScript with the SDK under `node:test` ([`bench/e2e/sdk`](bench/e2e/sdk)), and idiomatic Playwright Test with a setup project and `fullyParallel` ([`bench/e2e/specs`](bench/e2e/specs)). They cover forms, filtering, hover menus, back/forward, reloads, `target=_blank` and popup tabs, same-site, cross-site and nested iframes, file upload, network mocking (data, errors, aborts, delays, `times`, POST checks) and a saved login. Same app server, same Chromium headless-shell binary, Playwright Test 1.64.0. Each run is a whole CLI command, start to exit, as CI runs it; median of 5 interleaved runs on 4 CPUs (`node bench/e2e.mjs`). At most `workers` tests run at once in every tool. "First run" means no warm browser was running, which is what a fresh CI job gets. Every suite passed every run.

| Tests | Workers | Playwright Test | `swiftbrowse test` | SwiftBrowse SDK (node:test) | `swiftbrowse test`, first run |
|---|---|---|---|---|---|
| 29 | 1 | 10454 ms | **4519 ms** (2.3×) | **5065 ms** (2.1×) | **4657 ms** (2.2×) |
| 29 | 4 | 6511 ms | **2150 ms** (3.0×) | **2814 ms** (2.3×) | **2351 ms** (2.8×) |
| 145 | 1 | 46427 ms | **22900 ms** (2.0×) | **24188 ms** (1.9×) | **22973 ms** (2.0×) |
| 145 | 4 | 23904 ms | **10343 ms** (2.3×) | **12034 ms** (2.0×) | **10699 ms** (2.2×) |

Where the time goes: Playwright Test starts Node, transforms the test files and starts a browser per worker; SwiftBrowse is a native process whose sessions attach to warm browsers (one per parallel worker) in milliseconds. A good part of the remaining time is the demo app's own deliberate waits (120 ms to add an item, 150 ms to sign in, 600 ms in the loading-state test), which no tool can skip. The SDK pays for a Node process per test file. One machine, one small app: on a slow app, the app's own time dominates.

## Tests in code: TypeScript and Python SDKs

The SDKs drive the same engine through `swiftbrowse sdk` (JSON lines over stdin/stdout). Their methods and types are **generated from the engine's operation schemas** (CI fails if they drift), so every operation above is a method: required arguments in order, the rest as options. A failed action or `expect` throws, so any test framework reports it. Each session is isolated, sessions run concurrently, and they attach to the warm daemon.

```ts
// node --test   (or Vitest, Jest, …) — sdk/typescript, zero dependencies, typed
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

```python
# pytest or unittest — sdk/python, standard library only
from swiftbrowse import launch

def test_signs_in():
    with launch() as browser, browser.new_session() as page:
        page.navigate("http://localhost:3000/login")
        page.fill("Email", "ada@example.com")
        page.click({"by": "role", "role": "button", "name": "Sign in"}, expect={"type": "url_contains", "value": "/dashboard"})
        page.expect({"type": "text_visible", "text": "Welcome, Ada"})
```

`launch()` uses test defaults (no approval prompts, `evaluate` allowed, uploads from the working directory); `launch({ testDefaults: false })` / `launch(test_defaults=False)` keeps the agent-safe policy. Tests: [`sdk/typescript/test`](sdk/typescript/test), [`sdk/python/tests`](sdk/python/tests).

## The contract

Every interface sends the same `Request` and gets the same `ActionResult`. MCP tool names, CLI script `op`s and schemas are generated from one list (`crates/core/src/request.rs`).

| op | does |
|---|---|
| `navigate` | open URL, wait for `load` / `dom_content_loaded` / `network_idle`, report HTTP status |
| `get_page_snapshot` | compact view: headings, visible interactive elements with `ref`s, dialogs/alerts/validation. Default `format: "text"` is one line per element (`textbox "Email" [e3] type=email`); `format: "json"` gives objects; `since_last` returns a `+`/`~`/`-` diff |
| `get_accessibility_tree` | bounded browser AX tree |
| `find_element` | resolve a locator (or `all: true`) |
| `click` | actionability checks, approval gate, optional `expect` verification |
| `fill` / `type_text` / `press_key` / `select_option` | input; `fill` and `select_option` verify the resulting value |
| `wait_for` / `assert_page_state` | condition-based waits and assertions (recorded in the report) |
| `get_screenshot` | PNG into the evidence directory |
| `get_console_errors` / `get_network_failures` | diagnostics |
| `get_test_report` / `get_capabilities` | verdict and evidence; backend features |

MCP adds `session_create`, `session_close`, `session_list`. Tool results reach the agent as compact text; the full `ActionResult` is in `structuredContent`. Tools default to a `default` session, which the server starts launching at startup (`--no-prewarm` to disable).

### Locators

`{"by": "role", "role": "button", "name": "Save"}`, `label`, `test_id`, `text`, `css`, `xpath`, `ref` (from a snapshot), or `auto`, which tries accessible name → label → test id → exact text → partial text and reports which one matched.

**Selection rule:** if any match is visible, hidden matches are ignored. More than one remaining match without `nth` is an `ambiguous` error listing the candidates. The core never picks one silently.

### Results and verification

`ok` means the operation succeeded **and** any requested verification passed. A click without `expect` returns `verification.status = "not_checked"`, meaning the input was sent but nothing confirms the outcome. Errors carry a `kind` (`not_found`, `ambiguous`, `not_actionable`, `stale_element`, `timeout`, `approval_required`, `verification_failed`, `permission_denied`, …) and a `recoverable` flag. Failed interactions attach a screenshot.

### Recovery

The core retries only failures the backend guarantees happened *before* input reached the page: not found yet, not visible, disabled, covered, still animating, stale handle. A dispatched click is never retried. When `expect` fails, the result is `verification_failed` with "do not repeat it blindly".

### Policy and security

- Navigation allows `http`, `https`, `about` and `data` by default; `file:` needs `--allow-file-urls`. Host allow/block lists are available via `--allow-host` / `--block-host`.
- Clicks whose target text matches buy/pay/checkout/delete/transfer-style patterns return `approval_required` until repeated with `approved: true` (`--no-approval` disables this for CI test environments).
- `confirm()`/`prompt()` are dismissed, never accepted on the user's behalf; `alert()` is acknowledged. Both are reported.
- Password values are never echoed in results.
- Writes go only to the evidence directory, and names are validated.
- The page helper runs in an isolated JS world, so page scripts can't read or overwrite it.
- Each session gets its own browser process and a temporary profile, deleted on close or drop. The MCP server cleans up on SIGTERM/SIGINT/SIGHUP. On Linux, browsers also die with a SIGKILLed server (parent-death signal), and the next launch sweeps profiles left by dead owners.
- Running as root disables Chromium's sandbox (it can't start otherwise). `doctor` and the capability report warn about it; rely on the container boundary.

## Agent benchmark vs Playwright MCP

`bench/agents.mjs` drives SwiftBrowse MCP and Playwright MCP 0.0.83 over the same transport (MCP stdio) the way an agent does: take a snapshot, read element refs from it, act, confirm the outcome. It counts wall time, tool calls, and the bytes each tool returns into the agent's context. Playwright MCP runs twice: with its defaults, and tuned with `--timeout-settle 0` (its best case; the default waits 500 ms after each action). Both use the same Chromium binary (headless shell 153.0.8010.12), pages and viewport; 15 iterations, 10 interleaved cold starts. Every contender succeeded on every iteration.

| task | SwiftBrowse MCP | Playwright MCP (tuned) | Playwright MCP (defaults) |
|---|---|---|---|
| sign-up form: fill ×2, select, check, submit, confirm | **335 ms · 7 calls · 1.1 KB** | 472 ms · 8 calls · 2.2 KB | 1,302 ms · 8 calls · 2.2 KB |
| 400-row page, look first: snapshot, click "Edit row 250", confirm | **166 ms · 3 calls · 6.6 KB** | 1,037 ms · 4 calls · 152.6 KB | 1,604 ms · 4 calls · 152.6 KB |
| 400-row page, direct: click by role and name, confirm | **153 ms · 2 calls · 0.2 KB** | 834 ms · 3 calls · 0.8 KB | 1,415 ms · 3 calls · 0.8 KB |
| cold start → first page loaded, first-ever run (own browser) | **162 ms** | 704 ms | 715 ms |
| cold start → first page loaded, any later run (warm daemon) | **52 ms** | 704 ms | 715 ms |

Why it wins, and the caveats:
- **Context:** snapshots and action results reach the agent as compact text (`ok click button "Sign up" [e12] · verified: text_visible`). The full structured result stays in `structuredContent`. SwiftBrowse snapshots are bounded (150 elements), while Playwright MCP's `browser_snapshot` returns the whole page. On the 400-row page that's 6.6 KB vs 152.6 KB, though the bounded snapshot doesn't contain row 250; the agent has to locate it, which the snapshot itself tells it to do.
- **Calls:** `click` with `expect` acts and confirms in one call; Playwright MCP needs a separate `browser_wait_for`.
- **Cold start:** the headless shell, a browser launched while the client is still connecting, and pipe transport. After the first run, the warm-browser daemon hands each new session a prepared tab. Task timings are the same with and without the daemon (all rows above were also measured both ways).
- Medians from one machine and two synthetic pages. Task flows are in `bench/agents.mjs`; neither server was tuned per task. Results: `bench/results/agents-*`.

Run it: `cd bench && npm install && node agents.mjs ../target/release/swiftbrowse --iter 15 --cold 10 [--browser shell|full]`.

## Tested on live sites

`examples/live/` holds read-only agent task scripts for [pipilot.dev](https://pipilot.dev) and [the3rdacademy.com](https://the3rdacademy.com): navigate, assert, snapshot, open menus, follow links, type without submitting, check diagnostics. Run them with navigation restricted to those hosts:

```sh
swiftbrowse run examples/live/pipilot.json --allow-host pipilot.dev --allow-host '*.pipilot.dev'
```

Both pass every step (2 runs each, 0 console errors, 0 network failures, 0 leaked browsers). Running them exposed real-site behaviour the fixtures didn't have; each case now has a regression test in `crates/chromium/tests/`:

| seen on a live site | what SwiftBrowse does now |
|---|---|
| Page is blank at `load` and renders client-side | navigate waits for content (bounded) and says so in a note |
| App tears down its DOM after `load` and re-renders in stages | snapshots wait until scripts stop adding or removing elements for 150 ms; settled pages pay nothing |
| Link named only by a child `<svg role="img" aria-label>` or `<img alt>` | names are computed from content like the accessible-name algorithm |
| Off-screen sections with `content-visibility: auto` (empty `innerText`) | names and text fall back to the DOM |
| Decorative duplicates under `aria-hidden="true"` (news tickers) | excluded from snapshots and role/label lookups |
| A click lands before the app has attached its handlers | a click without `expect` reports what changed (`navigated`, `url_changed`, `state_changed`, `dom_changed`), or that nothing did |

## Benchmark vs the Playwright library

`bench/bench.mjs` runs identical tasks with Playwright 1.64.0 (library API) and SwiftBrowse (through MCP over stdio). Both use the same Chromium binary, local pages and viewport, with warm-ups discarded. Its header lists the fairness rules; keep them when changing it.

Latest run: 4× Xeon 2.1 GHz cloud container, Chrome for Testing 153.0.8010.12 headless shell for **both** tools (Playwright's own headless default), 50 warm iterations, 20 interleaved cold runs, both run orders (SwiftBrowse-first / Playwright-first). p50 in ms:

| task | SwiftBrowse | Playwright | ratio |
|---|---:|---:|---:|
| cold start → loaded page, own browser | 159 / 165 | 153 / 160 | 1.03 / 1.03 (parity) |
| cold start → loaded page, warm daemon | **55 / 55** | 153 / 160 | **0.36 / 0.34** |
| navigate | 19.8 / 19.0 | 20.8 / 18.8 | 0.95 / 1.01 |
| navigate, 145 interleaved pairs ×3 | | | SwiftBrowse faster by 1.31 ± 0.89, 1.74 ± 0.83, 0.56 ± 0.74 ms (95% CI) |
| fill (email) | 7.1 | 33.0 | 0.21 |
| click (checkbox label) | 16.9 | 37.6 | 0.45 |
| **form task total** | **336 / 334** | **452 / 437** | **0.74 / 0.77** |
| snapshot, 400-row page | 18.3 | 157 | 0.12 |
| role+name lookup, 400-row page | 4.4 | 20.3 | 0.22 |

Read these with care:
- SwiftBrowse is measured through MCP (a separate process, JSON over stdio); Playwright through its in-process library. SwiftBrowse still wins navigate by about 1 ms in paired runs, but block medians move ±5% between runs.
- **Cold start with its own browser is at parity.** With the same binary, both tools are bounded by Chromium's own startup (~100 ms to a usable tab) plus the first page load. The warm-daemon row compares attaching to SwiftBrowse's running browser with Playwright launching one. Playwright could also connect to a running browser (`connectOverCDP`), but it doesn't keep or prepare one for you. Tried and measured without gain: pipe vs WebSocket transport, not waiting for setup replies before the first navigation, an initial startup tab, `--disable-gpu`, `--in-process-gpu`.
- SwiftBrowse does *more* work in some steps: `fill` reads the value back, and `navigate` returns the real title and waits for client-rendered pages that are still empty.
- Snapshots differ in format (line-per-element text with refs vs. ARIA YAML); on the large page SwiftBrowse is bounded to 150 elements, so sizes aren't equivalent content.
- One machine and one fixture set. Full tables: `bench/results/`.

Run it: `cd bench && npm install && node bench.mjs ../target/release/swiftbrowse --iter 50 --cold 20 [--browser shell|full] [--order playwright-first]`.

## What exists

| area | state |
|---|---|
| Shared Rust core (`crates/core`) | contract, locators, policy, sessions, verification, recovery, reports; mock backend for deterministic tests |
| Chromium backend (`crates/chromium`) | CDP over WebSocket, per-session process + profile, isolated-world helper, trusted input, diagnostics, screenshots, AX tree |
| MCP server (`crates/mcp`) | stdio, protocol 2024-11-05 / 2025-03-26 / 2025-06-18, generated schemas, output bounded to 256 KB |
| CLI (`crates/cli`) | `doctor`, `capabilities`, `run`, `test`, `snapshot`, `screenshot`, `mcp`, `daemon run/status/stop` |
| Test runner (`crates/cli/src/testrun.rs`) | discovery, parallel workers, retries/flaky, repeat, grep/tags, shards, skip/only, hooks, variables, base URL, static server and web-server command, list/dot/JSON/JUnit/HTML reports, failure screenshot + page snapshot |
| Warm-browser daemon (`crates/chromium/src/daemon.rs`) | Linux, macOS, Windows; spare prepared tabs, per-client ownership, token auth, idle exit |
| Tests | 23 core/MCP unit and engine tests, 4 real-browser integration suites, 1 signal-cleanup test, 1 daemon test (speed, isolation, refusals, cleanup) and 2 test-runner tests (example suite, failures/retries/reports) of the real binary, 1 key-mapping test |
| CI (`.github/workflows/ci.yml`) | written, **not yet run** |
| Desktop shell (Tauri/Wry) | not started; `doctor` reports WebView availability |
| Remote/authenticated MCP transport | not started |
| TypeScript / Python SDKs (`sdk/`) | over `swiftbrowse sdk`; methods and types generated from the operation schemas; tested with node:test, tsc and unittest |
| Downloads, visual grounding | reported `unsupported` in capabilities |

Known limitations: actions work in one frame at a time (`switch_frame`); clicks inside a CSS-scaled iframe are refused (the click guard sees they would miss) rather than mapped; CSS doesn't pierce shadow roots, and consequential-action detection uses text patterns, not semantics. Set `SWIFTBROWSE_TRACE=1` for launch and navigation phase timings.
