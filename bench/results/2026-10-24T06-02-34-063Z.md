# SwiftBrowse vs Playwright

- **date**: 2026-10-24T06:02:34.063Z
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
| cold_start | launch_to_loaded_page | 165.03 | 202.34 | 160 | 248.54 | 1.03 |
| cold_start | launch_to_loaded_page_warm_daemon | 54.96 | 72.71 | - | - | - |
| form_task | navigate | 18.97 | 25.51 | 19.6 | 25.84 | 0.97 |
| form_task | fill_email | 6.49 | 9.61 | 34.34 | 39.59 | 0.19 |
| form_task | fill_password | 4.67 | 6.2 | 7.65 | 12.69 | 0.61 |
| form_task | select | 3.4 | 4.56 | 6.46 | 8.26 | 0.53 |
| form_task | check | 17.55 | 25.59 | 43.96 | 53.12 | 0.40 |
| form_task | submit_and_wait | 281.52 | 286.32 | 335.68 | 344.49 | 0.84 |
| form_task | total | 333.82 | 348.37 | 447.75 | 464.61 | 0.75 |
| snapshot_form | snapshot | 1.12 | 1.62 | 6.47 | 8.88 | 0.17 |
| snapshot_large | snapshot | 18.24 | 25.13 | 152.75 | 181.91 | 0.12 |
| semantic_lookup_large | role_and_name | 3.29 | 5.82 | 19.21 | 28.27 | 0.17 |

## Snapshot output size (bytes an agent receives)

| page | SwiftBrowse compact | Playwright ariaSnapshot | note |
|---|---:|---:|---|
| snapshot_form | 288 | 349 | both complete |
| snapshot_large | 6400 | 125549 | SwiftBrowse output bounded to 150 elements (truncated=true); Playwright output is the full tree |

Snapshots are different formats (SwiftBrowse line-per-element text with refs and URL vs. Playwright YAML-like ARIA text without refs); sizes compare what each puts into an agent's context, not equivalent information.
