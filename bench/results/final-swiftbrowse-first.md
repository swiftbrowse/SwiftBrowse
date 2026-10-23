# SwiftBrowse vs Playwright

- **date**: 2026-10-23T21:59:12.001Z
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
| cold_start | launch_to_loaded_page | 389.45 | 462.27 | 370.76 | 398.11 | 1.05 |
| form_task | navigate | 31.73 | 42.67 | 30.58 | 35.96 | 1.04 |
| form_task | fill_email | 8.64 | 12.73 | 38.05 | 47.91 | 0.23 |
| form_task | fill_password | 7.7 | 11.57 | 9.2 | 13.08 | 0.84 |
| form_task | select | 4.85 | 7.97 | 7.51 | 10.07 | 0.65 |
| form_task | check | 16.72 | 28.51 | 43.12 | 52.04 | 0.39 |
| form_task | submit_and_wait | 287.22 | 294.09 | 338.7 | 344.87 | 0.85 |
| form_task | total | 361.8 | 378.36 | 467.48 | 484.71 | 0.77 |
| snapshot_form | snapshot | 1.77 | 2.49 | 6.54 | 8.35 | 0.27 |
| snapshot_large | snapshot | 14.56 | 21.18 | 147.96 | 202.53 | 0.10 |
| semantic_lookup_large | role_and_name | 3.26 | 4.34 | 18.16 | 22.11 | 0.18 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
