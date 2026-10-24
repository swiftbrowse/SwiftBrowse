# SwiftBrowse vs Playwright

- **date**: 2026-10-24T05:33:46.088Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.80GHz x4
- **memory_gb**: 15.7
- **os**: Linux 6.18.44-fc-v153
- **node**: v22.22.0
- **playwright**: 1.64.0 (playwright-core, library API)
- **swiftbrowse**: swiftbrowse 0.1.0 (MCP over stdio)
- **chromium**: Google Chrome for Testing 153.0.8010.12 (headless shell, both tools)
- **iterations**: 50
- **warmup_discarded**: 3
- **cold_runs**: 20
- **order**: swiftbrowse-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 171.22 | 197.73 | 169.8 | 198.02 | 1.01 |
| form_task | navigate | 19.79 | 26.53 | 20.78 | 29.59 | 0.95 |
| form_task | fill_email | 6.15 | 9.75 | 36.87 | 47.01 | 0.17 |
| form_task | fill_password | 5.24 | 7.27 | 8.21 | 12.19 | 0.64 |
| form_task | select | 3.46 | 5.12 | 7.1 | 10.69 | 0.49 |
| form_task | check | 18.38 | 27.91 | 43.2 | 51.31 | 0.43 |
| form_task | submit_and_wait | 283.23 | 289.26 | 335.19 | 346.64 | 0.84 |
| form_task | total | 335.51 | 354.03 | 451.53 | 471.41 | 0.74 |
| snapshot_form | snapshot | 1.26 | 3.21 | 6.74 | 11.23 | 0.19 |
| snapshot_large | snapshot | 18.34 | 32.33 | 156.92 | 184.32 | 0.12 |
| semantic_lookup_large | role_and_name | 4.37 | 8.03 | 20.25 | 29.89 | 0.22 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
