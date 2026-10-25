# The test runner

```sh
swiftbrowse test [OPTIONS] [PATHS]...
```

`swiftbrowse test` finds test files, runs the tests in parallel isolated sessions, retries failures if you ask, and reports the results. Paths can be files or directories; the default is the current directory. Directories are searched recursively for `*.test.json`, `*.spec.json` and `*.setup.json`, skipping hidden directories, `node_modules` and `target`.

## Common runs

```sh
swiftbrowse test                                   # everything under the current directory
swiftbrowse test tests/checkout.test.json          # one file
swiftbrowse test -g @smoke                         # tests tagged smoke
swiftbrowse test -g "checkout › pays" -j 1         # one test, one worker
swiftbrowse test --retries 2 --reporter dot        # CI-style
swiftbrowse test --repeat-each 20 -g "flaky one"   # hunt a flaky test
swiftbrowse test --list                            # show what would run
```

## Parallelism

`-j/--workers N` runs N tests at a time; the default is half the CPU cores. Every test has its own browser context, so tests in the same file run in parallel too. Setup files run first, then everything else.

Sessions attach to warm background browsers (the [daemon](configuration.md#the-warm-browser-daemon)), one per parallel worker, so a new test starts in milliseconds instead of launching Chromium. The first run after the daemons have exited starts them, which costs about a tenth of a second per browser.

## Picking tests

| Option | Effect |
|---|---|
| `-g, --grep REGEX` | Only tests whose title matches. A title is `file › test @tag1 @tag2`. |
| `--grep-invert REGEX` | Skip tests whose title matches. |
| `--shard i/n` | Run part *i* of *n*, to split a suite across CI machines. |
| `only` in a file | Run only tests marked `only`. Use `--forbid-only` on CI so a forgotten `only` fails the run. |
| `skip` in a file | Report the test as skipped. |

## Retries, flakiness and failures

| Option | Effect |
|---|---|
| `--retries N` | Retry a failing test up to N times. A test that fails and then passes is reported as **flaky**, and the run still passes. |
| `--repeat-each N` | Run every test N times. |
| `-x`, `--max-failures N` | Stop starting new tests after N failures (`-x` is 1). |
| `--test-timeout-ms` | Time budget for one attempt, including hooks. Default 30000. |
| `--expect-timeout-ms` | How long `expect` steps and actions wait for the page. Default 5000. |

## Where the app comes from

| Option | Effect |
|---|---|
| `--base-url URL` | Resolve relative `navigate` URLs against this URL. |
| `--serve DIR` | Serve a static directory on a free local port and use it as the base URL. Good for static sites and demos. |
| `--web-server CMD` + `--web-server-url URL` | Start your dev server (e.g. `npm run dev`) before the tests, wait until the URL accepts connections, and stop the server afterwards. If something already answers at the URL, it's reused. |
| `--var name=value` | Set a `${name}` variable (repeatable). |

```sh
swiftbrowse test --web-server "npm run dev" --web-server-url http://localhost:3000 --base-url http://localhost:3000/
```

## Output

| Option | Effect |
|---|---|
| `--reporter list` | One line per test (the default). |
| `--reporter dot` | One character per test. |
| `--reporter json` | The full results as JSON on stdout. |
| `--reporter quiet` | Only the summary. |
| `--output-dir DIR` | Where `results.json`, screenshots, page snapshots, traces, videos and saved states go. Default `test-results`. |
| `--junit FILE` | Also write a JUnit XML report, which most CI systems display. |
| `--html FILE` | Also write a self-contained HTML report. |
| `--trace off\|on\|retain-on-failure` | Record a trace per test. |
| `--video off\|on\|retain-on-failure` | Record a WebM video per test. |

See [Traces, video and reports](debugging.md).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Every test passed (flaky tests count as passed). |
| `1` | At least one test failed. |
| `2` | Bad configuration or an invalid test file. Nothing ran. |

Test files are validated before any browser starts: unknown fields, unknown operations, invalid arguments and undefined variables are reported with the file and step number.

## Headed runs

`--headed` shows the browser window, which is useful while writing a test. Headed sessions launch their own browser instead of using the daemon.

```sh
swiftbrowse test tests/login.test.json --headed -j 1
```
