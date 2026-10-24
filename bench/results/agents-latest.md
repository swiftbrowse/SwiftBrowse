# Agent tasks: SwiftBrowse MCP vs Playwright MCP

- **date**: 2026-10-24T06:00:05.265Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.80GHz x4
- **chromium**: Google Chrome for Testing 153.0.8010.12 (headless shell, both tools)
- **swiftbrowse**: swiftbrowse 0.1.0
- **playwright_mcp**: 0.0.83
- **iterations**: 15
- **cold_runs**: 10
- **note**: warm daemon: cold_start is attach to an already-running daemon browser; its first-ever start costs the same as own browser

Both servers are driven over MCP stdio as an agent would. formTask and largeTask: snapshot, take refs from it, act, confirm the outcome. largeDirect: act by role and name without a snapshot, then confirm. Bytes = text returned into the agent context across all tool calls of the task.

| task | contender | success | p50 ms | p95 ms | tool calls | context bytes |
|---|---|---:|---:|---:|---:|---:|
| formTask | SwiftBrowse MCP (own browser) | 15/15 | 333.6 | 337.4 | 7 | 1058 |
| formTask | SwiftBrowse MCP (warm daemon) | 15/15 | 333.6 | 349.8 | 7 | 1058 |
| formTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1273.1 | 1297.7 | 8 | 2187 |
| formTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 467.2 | 568.1 | 8 | 2184 |
| largeTask | SwiftBrowse MCP (own browser) | 15/15 | 158 | 192.2 | 3 | 6588 |
| largeTask | SwiftBrowse MCP (warm daemon) | 15/15 | 160.6 | 216.5 | 3 | 6588 |
| largeTask | Playwright MCP 0.0.83 (defaults) | 15/15 | 1589.8 | 1661.5 | 4 | 152550 |
| largeTask | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 1032.8 | 1112.7 | 4 | 152550 |
| largeDirect | SwiftBrowse MCP (own browser) | 15/15 | 133.3 | 162.9 | 2 | 199 |
| largeDirect | SwiftBrowse MCP (warm daemon) | 15/15 | 132.7 | 173.2 | 2 | 199 |
| largeDirect | Playwright MCP 0.0.83 (defaults) | 15/15 | 1399.5 | 1460.4 | 3 | 755 |
| largeDirect | Playwright MCP 0.0.83 (--timeout-settle 0) | 15/15 | 924 | 1043.2 | 3 | 755 |
| cold_start | SwiftBrowse MCP (own browser) | - | 162.5 | 174.9 | - | - |
| cold_start | SwiftBrowse MCP (warm daemon) | - | 52.4 | 66.7 | - | - |
| cold_start | Playwright MCP 0.0.83 (defaults) | - | 715.4 | 778.7 | - | - |
| cold_start | Playwright MCP 0.0.83 (--timeout-settle 0) | - | 704 | 729.1 | - | - |
