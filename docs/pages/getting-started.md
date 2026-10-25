# Getting started

Install SwiftBrowse, give it a Chromium to drive, and run a first test as a JSON file, in TypeScript or in Python.

## Install

Pick one. Both ship the native `swiftbrowse` binary for your platform.

```sh
npm install -g swiftbrowse      # global CLI; or `npm install -D swiftbrowse` in a project, then `npx swiftbrowse`
```

```sh
pip install swiftbrowse         # installs the `swiftbrowse` command and the Python SDK
```

On npm, the binary comes from a platform package (`swiftbrowse-linux-x64`, `swiftbrowse-linux-arm64`, `swiftbrowse-darwin-arm64`, `swiftbrowse-darwin-x64` or `swiftbrowse-windows-x64`), which npm installs automatically. On PyPI, the wheel for your platform contains it. You can also download an archive from the [releases page](https://github.com/swiftbrowse/SwiftBrowse/releases), or build from source with `cargo build --release` (Rust 1.89+).

## Get a browser

SwiftBrowse drives Chromium. The easiest way to get the right build is Playwright's installer, which SwiftBrowse finds automatically:

```sh
npx playwright install chromium chromium-headless-shell
```

The headless shell is what headless sessions use; it starts about three times faster than full Chromium. Full Chromium is used for headed sessions (`--headed`). An installed Chrome or Chromium on your `PATH` or in a standard location also works. To pick a binary yourself, set `SWIFTBROWSE_CHROMIUM=/path/to/chrome`.

Check what SwiftBrowse found:

```sh
swiftbrowse doctor
```

`doctor` lists the operating system, the browsers it found and their versions, ffmpeg (needed only for video), and warnings, such as running as root (Chromium's sandbox is then off).

## Your first test

Make a folder with a tiny page to test, `site/index.html`:

```html
<!doctype html>
<title>Hello</title>
<h1>Hello</h1>
<label>Name <input id="name"></label>
<button onclick="document.querySelector('output').textContent = 'Hi, ' + document.querySelector('#name').value">Greet</button>
<output></output>
```

and a test, `hello.test.json`:

```json
{
  "name": "hello page",
  "tests": [
    {
      "name": "greets by name",
      "steps": [
        { "op": "navigate", "url": "index.html" },
        { "op": "expect", "condition": { "type": "visible", "target": { "by": "role", "role": "heading", "name": "Hello" } } },
        { "op": "fill", "target": "Name", "value": "Ada" },
        { "op": "click", "target": "Greet", "expect": { "type": "text_visible", "text": "Hi, Ada" } }
      ]
    }
  ]
}
```

Run it:

```sh
swiftbrowse test --serve site
```

`--serve site` serves the folder on a free local port and resolves relative URLs such as `index.html` against it. For a real app, use `--base-url http://localhost:3000/` or let the runner start your dev server with `--web-server` (see [The test runner](test-runner.md)).

`swiftbrowse test` finds every `*.test.json` and `*.spec.json` under the current directory (or the paths you give it). It runs the tests in parallel, each in its own isolated browser context, and prints a list of results. Results, failure screenshots and page snapshots go to `test-results/`.

Now break it on purpose: change `"Hi, Ada"` to `"Hi, Bob"` and run again. The failure shows what was expected, what the page actually had, a screenshot, and the page as text. Then try the other outputs:

```sh
swiftbrowse test --serve site --reporter dot --html report.html     # compact output plus an HTML report
swiftbrowse test --serve site --trace on                            # a step-by-step trace.html per test
```

Read [Test files](writing-tests.md) next for everything a test file can contain.

## The example suite

The repository has a complete example: a small demo app and 29 tests covering forms, tabs, popups, iframes, uploads, network mocking and a saved login.

```sh
git clone https://github.com/swiftbrowse/SwiftBrowse
cd SwiftBrowse/examples/e2e
swiftbrowse test tests --serve site       # serves the demo app and runs every test
```

## Tests in code

If you'd rather write tests in a programming language, use an SDK. It runs the same engine. Serve the `site` folder from above (for example `python3 -m http.server 3000 -d site`) and then:

TypeScript or JavaScript (`npm install -D swiftbrowse`), run with `node --test`:

```js
import { test } from 'node:test';
import { launch } from 'swiftbrowse';

test('greets by name', async () => {
  const browser = await launch();
  const page = await browser.newSession();
  await page.navigate('http://localhost:3000/');
  await page.fill('Name', 'Ada');
  await page.click('Greet', { expect: { type: 'text_visible', text: 'Hi, Ada' } });
  await page.expect({ type: 'title_equals', value: 'Hello' });
  await browser.close();
});
```

Python (`pip install swiftbrowse`), run with `pytest`:

```python
from swiftbrowse import launch

def test_greets_by_name():
    with launch() as browser, browser.new_session() as page:
        page.navigate("http://localhost:3000/")
        page.fill("Name", "Ada")
        page.click("Greet", expect={"type": "text_visible", "text": "Hi, Ada"})
        page.expect({"type": "title_equals", "value": "Hello"})
```

See [TypeScript and JavaScript](typescript.md) and [Python](python.md).

## For AI agents

Point an MCP client at `swiftbrowse mcp`:

```json
{ "mcpServers": { "swiftbrowse": { "command": "swiftbrowse", "args": ["mcp"] } } }
```

See [MCP server](mcp.md).

## Quick commands

```sh
swiftbrowse snapshot https://your-app.test             # the page as compact text, with element refs
swiftbrowse screenshot https://your-app.test --full-page
swiftbrowse run script.json                           # run a list of steps, print every result
swiftbrowse daemon status                             # the warm browsers new sessions attach to
```

## Something not working?

1. Run `swiftbrowse doctor` and check that a browser was found.
2. Run the failing test with `--trace on` and open the `trace.html` it prints.
3. [Open an issue](https://github.com/swiftbrowse/SwiftBrowse/issues). Include your OS, how you installed SwiftBrowse, the command you ran, its output, and `swiftbrowse doctor --json`.
