# Decisions

## 2026-10-23 · Benchmark against Playwright MCP, not only the Playwright library
- **Options:** (a) keep racing the in-process library; (b) race Playwright MCP on agent tasks; (c) both.
- **Chosen:** (c), with (b) as the headline. SwiftBrowse's buyers are agents, which choose between MCP servers. Against the library, every SwiftBrowse call pays an MCP process round trip (1–4 ms) the library never pays, so per-call latency there measures transport, not product.
- **Measured for agents:** time per task, bytes returned into the agent's context, tool calls, and verified success. Playwright MCP runs twice: with its defaults (what agents get) and with `--timeout-settle 0` (its best case).
- **Reversibility:** two-way door. **Revisit:** when Playwright MCP's response format changes, or when real agent transcripts give a better task set.

## 2026-10-23 · Snapshot text format as default
- Compact text for agents; `format: "json"` for programs. Small-page snapshot went from 972 to 288 bytes. Reversible: the default can flip without breaking JSON users.

## 2026-10-23 · Compact text for every MCP tool result
- Agents read `ok click button "Sign up" [e12] · verified: …` instead of the JSON result; programs keep `structuredContent`. Form-task context went from 3,212 to 1,002 bytes (Playwright MCP: 2,265). Errors keep kind, recoverability, message and details, so recovery isn't harmed.
- **Reversibility:** two-way door.

## 2026-10-23 · Servers must never leak browsers
- Found while benchmarking: a SIGTERMed `swiftbrowse mcp` orphaned its Chromium. Fixed with signal-driven shutdown, a Linux parent-death signal on the browser, and a sweep of profiles whose owner process is gone. Guarded by `crates/cli/tests/signals.rs`.

## 2026-10-24 · Real-site correctness over raw speed
- Live runs on pipilot.dev and the3rdacademy.com showed empty or partial snapshots on client-rendered pages, unnamed elements, duplicate ticker links, and clicks that did nothing before hydration. Fixed with a bounded settle (150 ms script quiet window, only when scripts are changing the DOM), accessible-name fallbacks, aria-hidden exclusion and click-effect reporting.
- **Cost:** only on pages whose scripts are changing the DOM; benchmarks unchanged within noise. **Reversibility:** two-way door (constants in `helper.js`).

## 2026-10-24 · CEF (Chromium Embedded Framework): not now
- Considered for the desktop shell. Gives one Chromium everywhere, but ships and patches a full browser per platform (on the order of 100+ MB), against the spec's lightweight goal, and adds nothing to automation, where the pinned Chromium backend already exists.
- **Revisit when** desktop work starts and system-WebView differences show up in tests; offer it then as an optional engine, not the default.

## 2026-10-24 · Headless shell by default for headless sessions
- Same Chromium version, about 3× faster process start (51 vs 146 ms). Cold start to first page: 419 → ~160 ms. Playwright's own library uses it by default too, so benchmarks give both tools the same binary (`--browser shell|full`).
- **Reversibility:** two-way door (`SWIFTBROWSE_CHROMIUM` overrides; headed sessions use full Chrome).

## 2026-10-24 · Pipe transport on Linux (and macOS since the daemon port)
- Measured no speed gain over WebSocket (161 vs 164 ms cold), but it opens no local TCP debugging port that other local processes could use to drive the browser. Kept for security. WebSocket remains for other platforms and via `SWIFTBROWSE_TRANSPORT=websocket`.

## 2026-10-24 · Cold start: stop at Chromium's floor
- Tried: not waiting for setup replies (no gain, and it lost early network events, so reverted), an initial startup tab (gain only over WebSocket), GPU flags (no gain). Against the Playwright library on the same binary, cold start is at parity. The next step would be a warm browser pool or daemon, which changes the isolation model; not taken without a decision.

## 2026-10-24 · Warm-browser daemon, on by default for headless sessions (Linux)
- **Asked for:** keep the first action's cold start, then make later sessions attach to a live, refreshed browser.
- **Built:** a per-user daemon on a `0700` Unix socket with a peer uid check. One Chromium; a spare tab with its session already set up; one browser context per client; ownership checks on every forwarded command; contexts disposed on disconnect; idle exit after 10 minutes.
- **Result:** first run ~160 ms (unchanged); later runs ~52 ms to a loaded page (Playwright MCP ~704 ms, Playwright library ~155 ms).
- **Trade-off:** sessions are isolated by browser context, not by process (the capability report says so). `--no-daemon` / `SWIFTBROWSE_DAEMON=off` restores one process per session.
- **Reversibility:** two-way door (the default can flip to off without breaking anything).

## 2026-10-24 · Daemon on macOS and Windows
- **Asked for:** the warm daemon on the remaining two platforms.
- **Built:** macOS reuses the Unix path (socket, uid check, pipe to the browser; Chromium exits when its pipe closes, standing in for Linux's parent-death signal). Windows uses a named pipe (first instance only, remote clients rejected), a localhost WebSocket to the browser, a job object that kills the browser with the daemon, and a detached process. All platforms now require a per-user random token in a hello message before any command is routed.
- **Evidence:** clippy is clean for x86_64/aarch64 macOS and x86_64 Windows; the daemon test is portable and runs in CI on all three. Runtime on macOS and Windows is proven only once CI runs; it was not run locally.
- **Trade-off:** on Windows the browser listens on a localhost debugging port (random, not the daemon's endpoint), which other local processes of any user could reach. Chromium can take its pipe on Windows too, but that needs inheritable handles passed the way the C runtime expects, which Rust's standard library doesn't do; a follow-up if the port matters.
- **Reversibility:** two-way door (`--no-daemon` everywhere).

## 2026-10-24 · Position as "the AI browser for browser automation and end-to-end testing"
- **Decided by:** the user. Reverses the earlier non-goal "being a general test runner".
- **Why it's credible today:** a flow file runs as a test (`swiftbrowse run`, exit 0/1, per-step `expect` and `assert_page_state`, evidence report), and the same steps are the MCP tools, so agent runs can become regression tests.
- **What the claim now requires:** run a directory of flows, JUnit output, screenshot on failure, retries; a fail-fast `expect`. Listed under Next in STRATEGY and as gaps in the README.
- **Reversibility:** two-way door (wording).
