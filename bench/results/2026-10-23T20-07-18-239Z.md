# SwiftBrowse vs Playwright

- **date**: 2026-10-23T20:07:18.239Z
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
- **order**: swiftbrowse-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 414.97 | 502.75 | 441.01 | 476.37 | 0.94 |
| form_task | navigate | 33.59 | 44.39 | 35.65 | 46.08 | 0.94 |
| form_task | fill_email | 9.66 | 15.39 | 44.26 | 63.26 | 0.22 |
| form_task | fill_password | 8.77 | 14.59 | 12.46 | 17.96 | 0.70 |
| form_task | select | 4.84 | 9.97 | 8.91 | 12.97 | 0.54 |
| form_task | check | 22.34 | 28.46 | 47.38 | 54.24 | 0.47 |
| form_task | submit_and_wait | 288.01 | 300.62 | 342.83 | 351.02 | 0.84 |
| form_task | total | 367.09 | 393.99 | 488.04 | 525.89 | 0.75 |
| snapshot_form | snapshot | 1.94 | 2.94 | 7.29 | 16.03 | 0.27 |
| snapshot_large | snapshot | 17.08 | 25.41 | 141.13 | 176.13 | 0.12 |
| semantic_lookup_large | role_and_name | 3.65 | 4.99 | 18.89 | 27.77 | 0.19 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 972 | 349 | both complete |
| snapshot_large | 14724 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (structured JSON with refs vs. YAML-like ARIA text); sizes compare what each puts into an agent's context, not equivalent information.
