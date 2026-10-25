# Continuous integration

On CI, SwiftBrowse needs the `swiftbrowse` binary and a Chromium. Optionally, install ffmpeg for video.

## GitHub Actions

```yaml
name: e2e
on: [push, pull_request]

jobs:
  e2e:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
      - run: npm ci
      - run: npx playwright install --with-deps chromium chromium-headless-shell
      - run: npx swiftbrowse test --retries 2 --forbid-only --reporter dot --junit junit.xml --html report.html --trace retain-on-failure
        env:
          TEST_PASSWORD: ${{ secrets.TEST_PASSWORD }}
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: test-results
          path: |
            test-results/
            report.html
```

This assumes `swiftbrowse` is a dev dependency (`npm install -D swiftbrowse`). With Python, use `pip install swiftbrowse` and run `swiftbrowse test`.

`--with-deps` installs the system libraries Chromium needs on a fresh Ubuntu runner.

### With your app's dev server

```yaml
      - run: npx swiftbrowse test --web-server "npm run start" --web-server-url http://localhost:3000 --base-url http://localhost:3000/
```

## Sharding

Split a large suite across machines:

```yaml
jobs:
  e2e:
    strategy:
      matrix:
        shard: [1, 2, 3, 4]
    steps:
      # …
      - run: npx swiftbrowse test --shard ${{ matrix.shard }}/4 --junit junit-${{ matrix.shard }}.xml
```

## Tips

- `--forbid-only` fails the run if someone committed a test marked `only`.
- `--retries 2` keeps the run green on rare flakes, but they're still reported as **flaky**, so you can see and fix them.
- `--trace retain-on-failure` costs nothing for passing tests and gives you a full trace for each failure.
- Keep secrets in environment variables and use `${env.NAME}` in test files.
- Running as root, as in many containers, turns off Chromium's sandbox, because it can't start otherwise. `doctor` warns about it; the container is then your isolation boundary.
- Exit codes: `0` passed, `1` test failures, `2` invalid configuration.
