# Agent tasks: SwiftBrowse MCP vs Playwright MCP

- **date**: 2026-10-23T23:37:56.280Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.10GHz x4
- **chromium**: Google Chrome for Testing 153.0.8010.12
- **swiftbrowse**: swiftbrowse 0.1.0
- **playwright_mcp**: 0.0.83
- **iterations**: 15
- **cold_runs**: 10

Both servers are driven over MCP stdio as an agent would. formTask and largeTask: snapshot, take refs from it, act, confirm the outcome. largeDirect: act by role and name without a snapshot, then confirm. Bytes = text returned into the agent context across all tool calls of the task.

| task | contender | success | p50 ms | p95 ms | tool calls | context bytes |
|---|---|---:|---:|---:|---:|---:|
| formTask | SwiftBrowse MCP | 15/15 | 353.3 | 369 | 7 | 1002 |
| formTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1285.1 | 1299.5 | 8 | 2268 |
| formTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 469.2 | 566.7 | 8 | 2265 |
| largeTask | SwiftBrowse MCP | 15/15 | 135.3 | 178.6 | 3 | 6588 |
| largeTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1383.8 | 1421.6 | 4 | 152550 |
| largeTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 868 | 1015.3 | 4 | 152550 |
| largeDirect | SwiftBrowse MCP | 15/15 | 117.2 | 221.2 | 2 | 199 |
| largeDirect | Playwright MCP 0.0.83 (defaults) | 15/15 | 1262.8 | 1355.5 | 3 | 755 |
| largeDirect | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 745.8 | 835.7 | 3 | 755 |
| cold_start | SwiftBrowse MCP | - | 378 | 393.2 | - | - |
| cold_start | Playwright MCP 0.0.83 (defaults) | - | 833.7 | 921.2 | - | - |
| cold_start | Playwright MCP 0.0.83 (--timeout-settle 0) | - | 826.6 | 895.9 | - | - |
