# SwiftBrowse vs Playwright

- **date**: 2026-10-24T04:42:56.515Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.80GHz x4
- **memory_gb**: 15.7
- **os**: Linux 6.18.44-fc-v153
- **node**: v22.22.0
- **playwright**: 1.64.0 (playwright-core, library API)
- **swiftbrowse**: swiftbrowse 0.1.0 (MCP over stdio)
- **chromium**: Google Chrome for Testing 153.0.8010.12
- **iterations**: 30
- **warmup_discarded**: 3
- **cold_runs**: 10
- **order**: swiftbrowse-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 399.31 | 454.19 | 400.19 | 428.78 | 1.00 |
| form_task | navigate | 32.8 | 46.9 | 29.3 | 38.36 | 1.12 |
| form_task | fill_email | 10.04 | 14.91 | 35.89 | 45.87 | 0.28 |
| form_task | fill_password | 7.59 | 12.96 | 8.26 | 12.09 | 0.92 |
| form_task | select | 4.96 | 7.49 | 6.68 | 8.65 | 0.74 |
| form_task | check | 20.86 | 26.95 | 40.31 | 47.13 | 0.52 |
| form_task | submit_and_wait | 284.24 | 286.2 | 334.08 | 342.3 | 0.85 |
| form_task | total | 365.33 | 368.67 | 450.71 | 477.09 | 0.81 |
| snapshot_form | snapshot | 1.42 | 1.9 | 4.99 | 5.8 | 0.28 |
| snapshot_large | snapshot | 18.1 | 27.85 | 146.46 | 187.81 | 0.12 |
| semantic_lookup_large | role_and_name | 3.33 | 4.91 | 18.72 | 24.76 | 0.18 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
