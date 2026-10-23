# SwiftBrowse vs Playwright

- **date**: 2026-10-23T20:08:26.340Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.10GHz x4
- **memory_gb**: 15.7
- **os**: Linux 6.18.44-fc-v153
- **node**: v22.22.0
- **playwright**: 1.64.0 (playwright-core, library API)
- **swiftbrowse**: swiftbrowse 0.1.0 (MCP over stdio)
- **chromium**: Google Chrome for Testing 153.0.8010.12
- **iterations**: 50
- **warmup_discarded**: 3
- **cold_runs**: 10
- **order**: playwright-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 443.27 | 479.28 | 428.09 | 481 | 1.04 |
| form_task | navigate | 33.94 | 42.21 | 32.27 | 38.47 | 1.05 |
| form_task | fill_email | 9.41 | 14.11 | 40.52 | 56.34 | 0.23 |
| form_task | fill_password | 8.22 | 12.13 | 10.07 | 15.53 | 0.82 |
| form_task | select | 5.11 | 10.61 | 7.62 | 11.08 | 0.67 |
| form_task | check | 21 | 28.09 | 45.82 | 54.19 | 0.46 |
| form_task | submit_and_wait | 288.78 | 293.93 | 340.32 | 347.41 | 0.85 |
| form_task | total | 366.23 | 387.83 | 478.01 | 505.11 | 0.77 |
| snapshot_form | snapshot | 1.74 | 2.42 | 7.3 | 10.6 | 0.24 |
| snapshot_large | snapshot | 16.3 | 24.06 | 134.26 | 165.73 | 0.12 |
| semantic_lookup_large | role_and_name | 3.38 | 5 | 18.48 | 25.05 | 0.18 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 972 | 349 | both complete |
| snapshot_large | 14724 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (structured JSON with refs vs. YAML-like ARIA text); sizes compare what each puts into an agent's context, not equivalent information.
