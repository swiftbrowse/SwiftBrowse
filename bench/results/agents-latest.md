# Agent tasks: SwiftBrowse MCP vs Playwright MCP

- **date**: 2026-10-24T04:42:09.570Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.80GHz x4
- **chromium**: Google Chrome for Testing 153.0.8010.12
- **swiftbrowse**: swiftbrowse 0.1.0
- **playwright_mcp**: 0.0.83
- **iterations**: 15
- **cold_runs**: 10

Both servers are driven over MCP stdio as an agent would. formTask and largeTask: snapshot, take refs from it, act, confirm the outcome. largeDirect: act by role and name without a snapshot, then confirm. Bytes = text returned into the agent context across all tool calls of the task.

| task | contender | success | p50 ms | p95 ms | tool calls | context bytes |
|---|---|---:|---:|---:|---:|---:|
| formTask | SwiftBrowse MCP | 15/15 | 349.6 | 372.6 | 7 | 1058 |
| formTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1247.9 | 1284.2 | 8 | 2268 |
| formTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 482.1 | 531.6 | 8 | 2265 |
| largeTask | SwiftBrowse MCP | 15/15 | 168 | 278.8 | 3 | 6588 |
| largeTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1469.2 | 1562.4 | 4 | 152550 |
| largeTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 1039.1 | 1088.7 | 4 | 152550 |
| largeDirect | SwiftBrowse MCP | 15/15 | 140.3 | 274 | 2 | 199 |
| largeDirect | Playwright MCP 0.0.83 (defaults) | 15/15 | 1333.8 | 1378.3 | 3 | 755 |
| largeDirect | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 962 | 1033 | 3 | 755 |
| cold_start | SwiftBrowse MCP | - | 419.4 | 452.7 | - | - |
| cold_start | Playwright MCP 0.0.83 (defaults) | - | 968.1 | 1023.6 | - | - |
| cold_start | Playwright MCP 0.0.83 (--timeout-settle 0) | - | 931.8 | 1013.1 | - | - |
