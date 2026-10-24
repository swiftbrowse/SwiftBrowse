# SwiftBrowse vs Playwright

- **date**: 2026-10-24T05:34:48.455Z
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
- **order**: playwright-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 165.65 | 195.27 | 160.81 | 168.59 | 1.03 |
| form_task | navigate | 19.03 | 27.52 | 18.76 | 23.26 | 1.01 |
| form_task | fill_email | 6.67 | 10.72 | 34.37 | 44.24 | 0.19 |
| form_task | fill_password | 5.38 | 7.75 | 7.34 | 10.1 | 0.73 |
| form_task | select | 3.39 | 7.81 | 6.32 | 8.89 | 0.54 |
| form_task | check | 17.09 | 26.32 | 39.88 | 49.28 | 0.43 |
| form_task | submit_and_wait | 282.47 | 290.19 | 333.21 | 342.51 | 0.85 |
| form_task | total | 334.48 | 352.92 | 436.99 | 459.48 | 0.77 |
| snapshot_form | snapshot | 1.17 | 1.63 | 6.63 | 10.23 | 0.18 |
| snapshot_large | snapshot | 18.22 | 30.18 | 146.38 | 173.79 | 0.12 |
| semantic_lookup_large | role_and_name | 4.17 | 6.4 | 19.73 | 26.68 | 0.21 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
