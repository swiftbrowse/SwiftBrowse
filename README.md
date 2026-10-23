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
| `get_page_snapshot` | compact view: headings, visible interactive elements with `ref`s, dialogs/alerts/validation; `since_last` returns a diff |
| `get_accessibility_tree` | bounded browser AX tree |
| `find_element` | resolve a locator (or `all: true`) |
| `click` | actionability checks, approval gate, optional `expect` verification |
| `fill` / `type_text` / `press_key` / `select_option` | input; `fill` and `select_option` verify the resulting value |
| `wait_for` / `assert_page_state` | condition-based waits and assertions (recorded in the report) |
| `get_screenshot` | PNG into the evidence directory |
| `get_console_errors` / `get_network_failures` | diagnostics |
| `get_test_report` / `get_capabilities` | verdict and evidence; backend features |

MCP adds `session_create`, `session_close`, `session_list`. Tools default to a `default` session, which the server starts launching at startup (`--no-prewarm` to disable).

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
- Each session gets its own browser process and a temporary profile, deleted on close or drop.
- Running as root disables Chromium's sandbox (it can't start otherwise). `doctor` and the capability report warn about it; rely on the container boundary.

## Benchmark vs Playwright

`bench/bench.mjs` runs identical tasks with Playwright 1.64.0 (library API) and SwiftBrowse (through MCP over stdio). Both use the same Chromium binary, local pages and viewport, with warm-ups discarded. Its header lists the fairness rules; keep them when changing it.

Latest run: 4× Xeon 2.1 GHz cloud container, Chrome for Testing 153.0.8010.12, 50 iterations, 10 cold runs, both run orders. p50 in ms:

| task | SwiftBrowse | Playwright | ratio |
|---|---:|---:|---:|
| cold start → loaded page | 415 / 443 | 441 / 428 | 0.94 / 1.04 (tie) |
| navigate | 33.6 / 33.9 | 35.7 / 32.3 | 0.94 / 1.05 (tie) |
| fill (email) | 9.7 | 44.3 | 0.22 |
| click (checkbox label) | 22.3 | 47.4 | 0.47 |
| submit + wait for result | 288 | 343 | 0.84 |
| **form task total** | **367** | **488** | **0.75** |
| snapshot, small page | 1.9 | 7.3 | 0.27 |
| snapshot, 400-row page | 17.1 | 141 | 0.12 |
| role+name lookup, 400-row page | 3.7 | 18.9 | 0.19 |

Read these with care:
- SwiftBrowse does *more* work in some steps: `fill` reads the value back to verify it, and `navigate` reads the real title. Playwright's `goto` returns neither.
- The snapshot comparison is compact JSON with refs vs. `ariaSnapshot()` YAML. **On the small page SwiftBrowse's output is larger (972 vs 349 bytes).** On the large page it is bounded to 150 elements and truncated, so the 14.7 KB vs 125 KB sizes aren't equivalent content.
- One machine and one fixture set. Cold start and navigate are within run-to-run noise. Full tables: `bench/results/`.

Run it: `cd bench && npm install && node bench.mjs ../target/release/swiftbrowse --iter 50 --cold 10 [--order playwright-first]`.

## What exists

| area | state |
|---|---|
| Shared Rust core (`crates/core`) | contract, locators, policy, sessions, verification, recovery, reports; mock backend for deterministic tests |
| Chromium backend (`crates/chromium`) | CDP over WebSocket, per-session process + profile, isolated-world helper, trusted input, diagnostics, screenshots, AX tree |
| MCP server (`crates/mcp`) | stdio, protocol 2024-11-05 / 2025-03-26 / 2025-06-18, generated schemas, output bounded to 256 KB |
| CLI (`crates/cli`) | `doctor`, `capabilities`, `run`, `snapshot`, `screenshot`, `mcp` |
| Tests | 22 core/MCP unit and engine tests, 4 real-browser integration suites, 1 key-mapping test |
| CI (`.github/workflows/ci.yml`) | written, **not yet run** |
| Desktop shell (Tauri/Wry) | not started; `doctor` reports WebView availability |
| Remote/authenticated MCP transport | not started |
| TypeScript / Python SDKs | not started |
| Frames, downloads, uploads, visual grounding | reported `partial`/`unsupported` in capabilities |

Known limitations: only the main frame is supported, CSS doesn't pierce shadow roots, and consequential-action detection uses text patterns, not semantics. Set `SWIFTBROWSE_TRACE=1` for launch and navigation phase timings.
