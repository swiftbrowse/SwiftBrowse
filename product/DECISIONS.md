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
