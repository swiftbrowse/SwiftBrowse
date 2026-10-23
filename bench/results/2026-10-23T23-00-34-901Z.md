# SwiftBrowse vs Playwright

- **date**: 2026-10-23T23:00:34.901Z
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
- **order**: playwright-first
- **network**: loopback HTTP server in the benchmark process

SwiftBrowse snapshot = compact get_page_snapshot; Playwright snapshot = locator('body').ariaSnapshot(). Lookup = find_element vs getByRole(...).boundingBox().

Times in ms. Lower is better. Ratio = SwiftBrowse p50 / Playwright p50 (< 1 means SwiftBrowse is faster).

| task | step | SwiftBrowse p50 | p95 | Playwright p50 | p95 | ratio |
|---|---|---:|---:|---:|---:|---:|
| cold_start | launch_to_loaded_page | 398.29 | 495.2 | 361.43 | 434.3 | 1.10 |
| form_task | navigate | 30.33 | 37.04 | 27.9 | 34.36 | 1.09 |
| form_task | fill_email | 8.62 | 12.66 | 35.16 | 43.99 | 0.25 |
| form_task | fill_password | 7.37 | 11.26 | 8.69 | 13.56 | 0.85 |
| form_task | select | 4.75 | 7.7 | 6.98 | 11.3 | 0.68 |
| form_task | check | 16.36 | 27.85 | 40.09 | 49.66 | 0.41 |
| form_task | submit_and_wait | 285.5 | 290.77 | 337.63 | 347.08 | 0.85 |
| form_task | total | 351.69 | 370.2 | 459.2 | 472.96 | 0.77 |
| snapshot_form | snapshot | 1.63 | 3.56 | 5.9 | 8.26 | 0.28 |
| snapshot_large | snapshot | 13.7 | 19.94 | 126.83 | 155.86 | 0.11 |
| semantic_lookup_large | role_and_name | 3.2 | 4.18 | 17.2 | 22.75 | 0.19 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
