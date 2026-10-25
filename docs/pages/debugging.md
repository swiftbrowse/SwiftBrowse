# Traces, video and reports

## What every failure keeps

Even with no options, a failing test leaves in `test-results/`:

- a **screenshot** of the page at the moment of failure
- the **page as text**: URL, title, headings, and the visible interactive elements
- the **console errors** and **failed requests** the session saw
- the **error**: its kind (`not_found`, `ambiguous`, `timeout`, `verification_failed`, …), the step, and for assertions every condition with what was observed

The list reporter prints this. `test-results/results.json` has it all in machine-readable form.

## Traces

```sh
swiftbrowse test --trace retain-on-failure     # keep traces of failed tests only
swiftbrowse test --trace on                    # keep every test's trace
```

A trace is one self-contained `trace.html` per test. Open it in any browser; it needs no viewer and no server. It contains:

- **A step timeline**, with each step's duration and result.
- **Screenshot**: the page after each step.
- **Live DOM**: a real, script-free copy of the page after each step, including current form values, open shadow roots and the page's live CSS. Hover over and click elements to inspect them. The step's target is outlined, and the inspector suggests a locator for any element you pick. You can also open the snapshot in a new tab and use your browser's devtools on it.
- **Text**: the page as SwiftBrowse reads it.
- For assertions, **expected vs observed** for every condition.
- **Every request** the page made, with status, and whether it was mocked.

A trace is the fastest way to see why a test failed on CI: download it and open it.

## Video

```sh
swiftbrowse test --video retain-on-failure
```

Records a WebM video of each test and follows tab switches. Encoding needs ffmpeg. Install it with `npx playwright install ffmpeg`, put `ffmpeg` on your `PATH`, or set `SWIFTBROWSE_FFMPEG`. Without ffmpeg, the frames are kept as JPEG images instead.

Over MCP or in the SDKs, use `start_video` and `stop_video`.

## HTML report

```sh
swiftbrowse test --html report.html
```

A single self-contained file with every test, its status, attempts, durations and errors, with failure screenshots embedded and links to traces and videos.

## JSON and JUnit

- `--junit junit.xml`: JUnit XML, which GitHub Actions reporters, GitLab, Jenkins, CircleCI and Azure DevOps understand. Skipped and failed tests are marked as such.
- `test-results/results.json`, always written: the summary (passed, failed, flaky, skipped, duration) and every test with every attempt, error, and evidence path.
- `--reporter json`: the same JSON on stdout.

## Debugging locally

```sh
swiftbrowse test tests/cart.test.json -g "removes an item" --headed -j 1 --trace on
```

- `--headed` shows the browser.
- `swiftbrowse snapshot URL` prints what SwiftBrowse sees on a page: each element with its role, name and a `ref`. It's the quickest way to choose a target.
- `SWIFTBROWSE_TRACE=1` prints launch and navigation timings to stderr.
