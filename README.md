# SwiftBrowse

Agent-native browser automation with one contract across MCP, CLI and (planned) SDKs. Rust core, Chromium backend over the DevTools Protocol.

> **Status: Phase 1 of the product specification roadmap, plus the MCP server and CLI from Phase 4.** The cloud/headless Chromium path works and is tested. The Tauri/Wry desktop shell and the TypeScript/Python SDKs are **not started**. See [What exists](#what-exists).

## Quick start

```sh
cargo build --release
./target/release/swiftbrowse doctor            # what this machine can run
./target/release/swiftbrowse run examples/form-signup.json
./target/release/swiftbrowse mcp               # MCP server on stdio
```

`doctor` finds Chromium via `SWIFTBROWSE_CHROMIUM`, then Playwright-managed builds (`npx playwright install chromium`), then standard install locations and `PATH`. Headless sessions prefer Chromium's headless shell (`npx playwright install chromium-headless-shell`), which starts about 3× faster. On Linux, SwiftBrowse talks to the browser over a pipe, so no local debugging port is opened; set `SWIFTBROWSE_TRANSPORT=websocket` to use a port instead.

MCP client config:

```json
{ "mcpServers": { "swiftbrowse": { "command": "/path/to/swiftbrowse", "args": ["mcp"] } } }
```

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
| cold start → first page loaded | **175 ms** | 759 ms | 773 ms |

Why it wins, and the caveats:
- **Context:** snapshots and action results reach the agent as compact text (`ok click button "Sign up" [e12] · verified: text_visible`). The full structured result stays in `structuredContent`. SwiftBrowse snapshots are bounded (150 elements), while Playwright MCP's `browser_snapshot` returns the whole page. On the 400-row page that's 6.6 KB vs 152.6 KB, though the bounded snapshot doesn't contain row 250; the agent has to locate it, which the snapshot itself tells it to do.
- **Calls:** `click` with `expect` acts and confirms in one call; Playwright MCP needs a separate `browser_wait_for`.
- **Cold start:** the headless shell, a browser launched while the client is still connecting, and pipe transport.
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
| cold start → loaded page | 171 / 166 | 170 / 161 | 1.01 / 1.03 (parity) |
| navigate | 19.8 / 19.0 | 20.8 / 18.8 | 0.95 / 1.01 |
| navigate, 145 interleaved pairs ×3 | | | SwiftBrowse faster by 1.31 ± 0.89, 1.74 ± 0.83, 0.56 ± 0.74 ms (95% CI) |
| fill (email) | 7.1 | 33.0 | 0.21 |
| click (checkbox label) | 16.9 | 37.6 | 0.45 |
| **form task total** | **336 / 334** | **452 / 437** | **0.74 / 0.77** |
| snapshot, 400-row page | 18.3 | 157 | 0.12 |
| role+name lookup, 400-row page | 4.4 | 20.3 | 0.22 |

Read these with care:
- SwiftBrowse is measured through MCP (a separate process, JSON over stdio); Playwright through its in-process library. SwiftBrowse still wins navigate by about 1 ms in paired runs, but block medians move ±5% between runs.
- **Cold start is at parity, not a win.** With the same binary, both tools are bounded by Chromium's own startup (~100 ms to a usable tab) plus the first page load. Tried and measured without gain: pipe vs WebSocket transport, not waiting for setup replies before the first navigation, an initial startup tab, `--disable-gpu`, `--in-process-gpu`.
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
| CLI (`crates/cli`) | `doctor`, `capabilities`, `run`, `snapshot`, `screenshot`, `mcp` |
| Tests | 23 core/MCP unit and engine tests, 4 real-browser integration suites, 1 signal-cleanup test of the real binary, 1 key-mapping test |
| CI (`.github/workflows/ci.yml`) | written, **not yet run** |
| Desktop shell (Tauri/Wry) | not started; `doctor` reports WebView availability |
| Remote/authenticated MCP transport | not started |
| TypeScript / Python SDKs | not started |
| Frames, downloads, uploads, visual grounding | reported `partial`/`unsupported` in capabilities |

Known limitations: only the main frame is supported, CSS doesn't pierce shadow roots, and consequential-action detection uses text patterns, not semantics. Set `SWIFTBROWSE_TRACE=1` for launch and navigation phase timings.
