# SwiftBrowse vs Playwright

- **date**: 2026-10-23T22:59:19.946Z
- **cpu**: Intel(R) Xeon(R) Processor @ 2.10GHz x4
- **memory_gb**: 15.7
- **os**: Linux 6.18.44-fc-v153
- **node**: v22.22.0
- **playwright**: 1.64.0 (playwright-core, library API)
- **swiftbrowse**: swiftbrowse 0.1.0 (MCP over stdio)
- **chromium**: Google Chrome for Testing 153.0.8010.12
- **iterations**: 50
- **warmup_discarded**: 3
- **cold_runs**: 20
- **order**: swiftbrowse-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 423.56 | 472.37 | 397.95 | 429.36 | 1.06 |
| form_task | navigate | 32.08 | 40.16 | 27.27 | 33.25 | 1.18 |
| form_task | fill_email | 9.57 | 12.92 | 34.06 | 40.71 | 0.28 |
| form_task | fill_password | 7.59 | 11.95 | 9.19 | 13.96 | 0.83 |
| form_task | select | 4.54 | 7.93 | 6.73 | 9 | 0.67 |
| form_task | check | 18.04 | 27.04 | 41.71 | 54.13 | 0.43 |
| form_task | submit_and_wait | 286.04 | 294.03 | 338.04 | 342.17 | 0.85 |
| form_task | total | 360.28 | 383.18 | 453.86 | 472.17 | 0.79 |
| snapshot_form | snapshot | 1.56 | 2.23 | 6.73 | 11.09 | 0.23 |
| snapshot_large | snapshot | 13.69 | 20.76 | 127.33 | 159.97 | 0.11 |
| semantic_lookup_large | role_and_name | 3.33 | 4.49 | 16.97 | 21.72 | 0.20 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
