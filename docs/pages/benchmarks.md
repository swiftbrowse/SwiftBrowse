# Benchmarks

All numbers come from benchmark scripts in the repository, which you can run yourself. They come from one machine (a 4-CPU cloud container) and small demo apps. On a slow app, the app's own time dominates, so read them as the tool's overhead, not as a promise about your suite.

## Test runner vs Playwright Test

The same 29 tests, written three ways:

- JSON files run by `swiftbrowse test` ([`examples/e2e/tests`](https://github.com/swiftbrowse/SwiftBrowse/tree/main/examples/e2e/tests))
- hand-written TypeScript with the SDK under `node:test` ([`bench/e2e/sdk`](https://github.com/swiftbrowse/SwiftBrowse/tree/main/bench/e2e/sdk))
- idiomatic Playwright Test with a setup project and `fullyParallel` ([`bench/e2e/specs`](https://github.com/swiftbrowse/SwiftBrowse/tree/main/bench/e2e/specs))

They cover forms, filtering, hover menus, back and forward, reloads, `target=_blank` and popup tabs, same-site, cross-site and nested iframes, file upload, network mocking (data, errors, aborts, delays, `times`, POST checks) and a saved login.

All three used the same app server, the same Chromium headless-shell binary, and Playwright Test 1.64.0. Each run is a whole CLI command, start to exit, as CI runs it. The table shows the median of 5 interleaved runs. At most `workers` tests run at once in every tool. "First run" means no warm browser was running, which is what a fresh CI job gets. Every suite passed on every run.

| Tests | Workers | Playwright Test | `swiftbrowse test` | SwiftBrowse SDK | `swiftbrowse test`, first run |
|---|---|---|---|---|---|
| 29 | 1 | 10454 ms | **4519 ms** (2.3×) | **5065 ms** (2.1×) | **4657 ms** (2.2×) |
| 29 | 4 | 6511 ms | **2150 ms** (3.0×) | **2814 ms** (2.3×) | **2351 ms** (2.8×) |
| 145 | 1 | 46427 ms | **22900 ms** (2.0×) | **24188 ms** (1.9×) | **22973 ms** (2.0×) |
| 145 | 4 | 23904 ms | **10343 ms** (2.3×) | **12034 ms** (2.0×) | **10699 ms** (2.2×) |

Where the time goes: Playwright Test starts Node, transforms the test files and starts a browser per worker. SwiftBrowse is a native process whose sessions attach to warm browsers in milliseconds. Much of the remaining time is the demo app's own deliberate waits (120 ms to add an item, 150 ms to sign in, 600 ms in the loading-state test), which no tool can skip.

```sh
cd bench && npm install && node e2e.mjs ../target/release/swiftbrowse
```

## Agent tasks vs Playwright MCP

The benchmark drives SwiftBrowse MCP and Playwright MCP 0.0.83 over the same transport (MCP stdio), the way an agent does: take a snapshot, read element refs from it, act, confirm the outcome. Playwright MCP ran twice: with its defaults, and tuned with `--timeout-settle 0`, its best case. Both used the same Chromium binary; medians of 15 iterations.

| Task | SwiftBrowse MCP | Playwright MCP (tuned) | Playwright MCP (defaults) |
|---|---|---|---|
| Sign-up form: fill ×2, select, check, submit, confirm | **335 ms · 7 calls · 1.1 KB** | 472 ms · 8 calls · 2.2 KB | 1,302 ms · 8 calls · 2.2 KB |
| 400-row page: snapshot, click "Edit row 250", confirm | **166 ms · 3 calls · 6.6 KB** | 1,037 ms · 4 calls · 152.6 KB | 1,604 ms · 4 calls · 152.6 KB |
| 400-row page, direct: click by role and name, confirm | **153 ms · 2 calls · 0.2 KB** | 834 ms · 3 calls · 0.8 KB | 1,415 ms · 3 calls · 0.8 KB |
| Cold start to first loaded page, own browser | **162 ms** | 704 ms | 715 ms |
| Cold start to first loaded page, warm daemon | **52 ms** | 704 ms | 715 ms |

The KB column is how many bytes the tools returned into the agent's context. SwiftBrowse snapshots are bounded to 150 elements, so on the 400-row page the snapshot is smaller but doesn't contain row 250: the agent has to locate it, which the snapshot itself tells it to do.

```sh
cd bench && npm install && node agents.mjs ../target/release/swiftbrowse --iter 15 --cold 10
```

## Library operations vs Playwright

Identical tasks with the Playwright 1.64.0 library API and SwiftBrowse (through MCP over stdio), using the same headless-shell binary. p50 in ms:

| Task | SwiftBrowse | Playwright |
|---|---:|---:|
| Cold start to loaded page, own browser | 159 | 153 (parity) |
| Cold start to loaded page, warm daemon | **55** | 153 |
| `navigate` | 19.8 | 20.8 |
| `fill` (email) | **7.1** | 33.0 |
| `click` (checkbox label) | **16.9** | 37.6 |
| Whole form task | **336** | 452 |
| Snapshot, 400-row page | **18.3** | 157 |
| Role and name lookup, 400-row page | **4.4** | 20.3 |

With its own browser, cold start is at parity: both tools are bounded by Chromium's startup. SwiftBrowse does more work in some steps (`fill` reads the value back; `navigate` waits for client-rendered pages that are still empty).

```sh
cd bench && npm install && node bench.mjs ../target/release/swiftbrowse --iter 50 --cold 20
```
