# Agent tasks: SwiftBrowse MCP vs Playwright MCP

- **date**: 2026-10-24T05:37:16.106Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.80GHz x4
- **chromium**: Google Chrome for Testing 153.0.8010.12 (headless shell, both tools)
- **swiftbrowse**: swiftbrowse 0.1.0
- **playwright_mcp**: 0.0.83
- **iterations**: 15
- **cold_runs**: 10

Both servers are driven over MCP stdio as an agent would. formTask and largeTask: snapshot, take refs from it, act, confirm the outcome. largeDirect: act by role and name without a snapshot, then confirm. Bytes = text returned into the agent context across all tool calls of the task.

| task | contender | success | p50 ms | p95 ms | tool calls | context bytes |
|---|---|---:|---:|---:|---:|---:|
| formTask | SwiftBrowse MCP | 15/15 | 334.6 | 350.5 | 7 | 1058 |
| formTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1301.9 | 1368.3 | 8 | 2187 |
| formTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 471.5 | 500.2 | 8 | 2184 |
| largeTask | SwiftBrowse MCP | 15/15 | 165.9 | 217.2 | 3 | 6588 |
| largeTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1603.9 | 1651.4 | 4 | 152550 |
| largeTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 1037.1 | 1135.9 | 4 | 152550 |
| largeDirect | SwiftBrowse MCP | 15/15 | 153.3 | 230.8 | 2 | 199 |
| largeDirect | Playwright MCP 0.0.83 (defaults) | 15/15 | 1414.9 | 1439.4 | 3 | 755 |
| largeDirect | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 833.9 | 871.1 | 3 | 755 |
| cold_start | SwiftBrowse MCP | - | 175.3 | 184.4 | - | - |
| cold_start | Playwright MCP 0.0.83 (defaults) | - | 773.1 | 825.5 | - | - |
| cold_start | Playwright MCP 0.0.83 (--timeout-settle 0) | - | 758.7 | 812.9 | - | - |
