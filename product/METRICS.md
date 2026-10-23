# Metrics

North Star: verified agent tasks completed per day (outcome confirmed by SwiftBrowse). Not yet instrumented; no telemetry exists.

Benchmark inputs, tracked per release with `bench/agents.mjs` (vs Playwright MCP) and `bench/bench.mjs` (vs the Playwright library):

| input | 2026-10-23 | target |
|---|---|---|
| task success (agent benchmark) | 100% (45/45 per contender) | 100% |
| false success (ok without confirmed outcome) | 0 by construction (`not_checked` when unverified) | 0 |
| context bytes, sign-up form task | 1,002 (Playwright MCP 2,265) | ≤ Playwright MCP |
| tool calls, sign-up form task | 7 (Playwright MCP 8) | ≤ Playwright MCP |
| time, sign-up form task (p50) | 353 ms (Playwright MCP tuned: 469) | ≤ Playwright MCP tuned |
| cold start to first page (p50) | 378 ms (Playwright MCP: 827) | ≤ Playwright MCP |
