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

`doctor` finds Chromium via `SWIFTBROWSE_CHROMIUM`, then Playwright-managed builds (`npx playwright install chromium`), then standard install locations and `PATH`.

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

`bench/agents.mjs` drives SwiftBrowse MCP and Playwright MCP 0.0.83 over the same transport (MCP stdio) the way an agent does: take a snapshot, read element refs from it, act, confirm the outcome. It counts wall time, tool calls, and the bytes each tool returns into the agent's context. Playwright MCP runs twice: with its defaults, and tuned with `--timeout-settle 0` (its best case; the default waits 500 ms after each action). Same Chromium, pages, viewport and machine; 15 iterations, 10 interleaved cold starts. All contenders succeeded on every iteration.

| task | SwiftBrowse MCP | Playwright MCP (tuned) | Playwright MCP (defaults) |
|---|---|---|---|
| sign-up form: fill ×2, select, check, submit, confirm | **353 ms · 7 calls · 1.0 KB** | 469 ms · 8 calls · 2.3 KB | 1,285 ms · 8 calls · 2.3 KB |
| 400-row page, look first: snapshot, click "Edit row 250", confirm | **135 ms · 3 calls · 6.6 KB** | 868 ms · 4 calls · 152.6 KB | 1,384 ms · 4 calls · 152.6 KB |
| 400-row page, direct: click by role and name, confirm | **117 ms · 2 calls · 0.2 KB** | 746 ms · 3 calls · 0.8 KB | 1,263 ms · 3 calls · 0.8 KB |
| cold start → first page loaded | **378 ms** | 827 ms | 834 ms |

Why it wins, and the caveats:
- **Context:** snapshots and action results reach the agent as compact text (`ok click button "Sign up" [e12] · verified: text_visible`). The full structured result stays in `structuredContent`. SwiftBrowse snapshots are bounded (150 elements), while Playwright MCP's `browser_snapshot` returns the whole page. On the 400-row page that's 6.6 KB vs 152.6 KB, though the bounded snapshot doesn't contain row 250. The agent has to locate it, which the snapshot itself tells it to do.
- **Calls:** `click` with `expect` acts and confirms in one call; Playwright MCP needs a separate `browser_wait_for`.
- Medians from one machine and two synthetic pages. The task flows are in `bench/agents.mjs`, and neither server was tuned per task. Results: `bench/results/agents-*.json`.

Run it: `cd bench && npm install && node agents.mjs ../target/release/swiftbrowse --iter 15 --cold 10`.

## Benchmark vs the Playwright library

`bench/bench.mjs` runs identical tasks with Playwright 1.64.0 (library API) and SwiftBrowse (through MCP over stdio). Both use the same Chromium binary, local pages and viewport, with warm-ups discarded. Its header lists the fairness rules; keep them when changing it.

Latest run: 4× Xeon 2.1 GHz cloud container, Chrome for Testing 153.0.8010.12, 50 warm iterations, 20 interleaved cold runs, both run orders (SwiftBrowse-first / Playwright-first). p50 in ms:

| task | SwiftBrowse | Playwright | ratio |
|---|---:|---:|---:|
| cold start → loaded page | 424 / 398 | 398 / 361 | 1.06 / 1.10 (near parity) |
| navigate | 32.1 / 30.3 | 27.3 / 27.9 | 1.18 / 1.09 (engine parity; see below) |
| fill (email) | 9.6 / 8.6 | 34.1 / 35.2 | 0.28 / 0.25 |
| click (checkbox label) | 18.0 / 16.4 | 41.7 / 40.1 | 0.43 / 0.41 |
| submit + wait for result | 286 / 286 | 338 / 338 | 0.85 / 0.85 |
| **form task total** | **360 / 352** | **454 / 459** | **0.79 / 0.77** |
| snapshot, small page | 1.6 | 6.7 | 0.23 |
| snapshot, 400-row page | 13.7 | 127 | 0.11 |
| role+name lookup, 400-row page | 3.3 | 17.0 | 0.20 |

Snapshot output an agent receives (bytes): small page **288 vs 349**; 400-row page 6,400 vs 125,549.

Read these with care:
- SwiftBrowse is measured through MCP (a separate process, JSON over stdio); Playwright through its in-process library. The MCP round trip costs 1–4 ms per call. Server-side, SwiftBrowse's navigate takes 26–28 ms against Playwright's 27, so the navigate row is transport, not engine. Cold start is also within run-to-run spread.
- SwiftBrowse does *more* work in some steps: `fill` reads the value back to verify it, and `navigate` returns the real title. Playwright's `goto` returns neither.
- Snapshots differ in format: SwiftBrowse sends line-per-element text with refs and the URL; Playwright's `ariaSnapshot()` is YAML without refs. On the large page SwiftBrowse is bounded to 150 elements, so those two sizes aren't equivalent content.
- One machine and one fixture set. Full tables: `bench/results/`.

Run it: `cd bench && npm install && node bench.mjs ../target/release/swiftbrowse --iter 50 --cold 10 [--order playwright-first]`.

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
