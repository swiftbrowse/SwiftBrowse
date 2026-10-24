# SwiftBrowse vs Playwright

- **date**: 2026-10-24T06:01:30.728Z
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
| cold_start | launch_to_loaded_page | 158.6 | 185.63 | 153.48 | 173.69 | 1.03 |
| cold_start | launch_to_loaded_page_warm_daemon | 54.61 | 62.54 | - | - | - |
| form_task | navigate | 20.1 | 25.09 | 18.87 | 26.41 | 1.07 |
| form_task | fill_email | 6.81 | 9.16 | 33.84 | 43.1 | 0.20 |
| form_task | fill_password | 5.54 | 7.5 | 7.42 | 9.91 | 0.75 |
| form_task | select | 3.58 | 5.51 | 6.46 | 9.3 | 0.55 |
| form_task | check | 16 | 24.67 | 38.47 | 48.7 | 0.42 |
| form_task | submit_and_wait | 282.26 | 287.83 | 333.19 | 343.33 | 0.85 |
| form_task | total | 334.37 | 349.31 | 440.28 | 464.79 | 0.76 |
| snapshot_form | snapshot | 1.07 | 1.32 | 5.82 | 7.27 | 0.18 |
| snapshot_large | snapshot | 17.59 | 22.41 | 151.67 | 183.43 | 0.12 |
| semantic_lookup_large | role_and_name | 3.11 | 5.51 | 20.99 | 30.09 | 0.15 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
