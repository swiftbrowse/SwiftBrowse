# SwiftBrowse vs Playwright

- **date**: 2026-10-23T19:51:38.729Z
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
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 471.61 | 512.13 | 388.59 | 436.74 | 1.21 |
| form_task | navigate | 37.94 | 47.67 | 30.09 | 38.77 | 1.26 |
| form_task | fill_email | 10.01 | 18.41 | 36.93 | 46.02 | 0.27 |
| form_task | fill_password | 8.26 | 16.02 | 9.98 | 14.92 | 0.83 |
| form_task | select | 5.43 | 8.64 | 7.16 | 11 | 0.76 |
| form_task | check | 21.69 | 27.76 | 44.82 | 51.09 | 0.48 |
| form_task | submit_and_wait | 288.43 | 294.61 | 338.49 | 346.55 | 0.85 |
| form_task | total | 370.47 | 399.17 | 466.98 | 485.28 | 0.79 |
| snapshot_form | snapshot | 2.04 | 2.54 | 6.41 | 8.55 | 0.32 |
| snapshot_large | snapshot | 17.11 | 23.68 | 134.11 | 165.49 | 0.13 |
| semantic_lookup_large | role_and_name | 3.52 | 5.6 | 16.78 | 23.41 | 0.21 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 972 | 349 | both complete |
| snapshot_large | 14724 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (structured JSON with refs vs. YAML-like ARIA text); sizes compare what each puts into an agent's context, not equivalent information.
