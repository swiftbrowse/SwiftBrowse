# swiftbrowse

**The AI browser for browser automation and end-to-end testing.** A native engine (Rust, driving Chromium over the DevTools Protocol) with one contract across a test runner, this Python SDK, a TypeScript SDK, and an MCP server for AI agents. Standard library only.

**📖 Documentation: https://steady-ember-82q7.here.now/**

```sh
pip install swiftbrowse
npx playwright install chromium chromium-headless-shell   # a Chromium for SwiftBrowse to drive
swiftbrowse doctor                                       # check what this machine can run
```

## Tests in code

```python
import pytest
from swiftbrowse import launch

@pytest.fixture(scope="session")
def browser():
    with launch() as b:
        yield b

@pytest.fixture
def page(browser):
    with browser.new_session() as s:   # isolated: own cookies, storage, tabs
        yield s

def test_signs_in(page):
    page.navigate("http://localhost:3000/login")
    page.fill("Email", "ada@example.com")
    page.click({"by": "role", "role": "button", "name": "Sign in"},
               expect={"type": "url_contains", "value": "/dashboard"})
    page.expect({"type": "text_visible", "text": "Welcome, Ada"})
```

- Every engine operation is a method with its documented signature (`navigate`, `click`, `fill`, `check`, `hover`, `select_option`, `upload_file`, `route`, `switch_tab`, `switch_frame`, `save_storage_state`, …), with required arguments positional and the rest by keyword.
- A target is a string (accessible name, label, test id or text) or a locator (`role`, `label`, `test_id`, `text`, `css`, `xpath`). Several matches are an error, never a guess.
- Actions wait until the element is visible, enabled, not covered and not moving. `expect` assertions retry until they pass.
- A failed action or assertion raises `SwiftBrowseError` with the full result attached.
- Sessions attach to a warm background browser, so creating one per test is cheap.

## Tests as JSON, with the built-in runner

```json
{
  "name": "sign in",
  "tests": [
    { "name": "signs in", "steps": [
      { "op": "navigate", "url": "login" },
      { "op": "fill", "target": "Email", "value": "ada@example.com" },
      { "op": "click", "target": "Sign in", "expect": { "type": "url_contains", "value": "/dashboard" } }
    ] }
  ]
}
```

```sh
swiftbrowse test --base-url http://localhost:3000/ -j 4 --retries 2 --html report.html --trace retain-on-failure
```

The runner supports parallel workers, retries and flaky detection, sharding, saved logins, network mocking, tabs, frames, uploads, traces with a live DOM, video, and JUnit/HTML reports. On the project's benchmark suite it runs 2–3× faster than Playwright Test.

## For AI agents

```json
{ "mcpServers": { "swiftbrowse": { "command": "swiftbrowse", "args": ["mcp"] } } }
```

## Learn more

- [Getting started](https://steady-ember-82q7.here.now/getting-started.html)
- [Python SDK](https://steady-ember-82q7.here.now/python.html)
- [Test files](https://steady-ember-82q7.here.now/writing-tests.html) and [the test runner](https://steady-ember-82q7.here.now/test-runner.html)
- [Operations reference](https://steady-ember-82q7.here.now/operations.html)
- [Issues](https://github.com/swiftbrowse/swiftbrowse/issues) and [source](https://github.com/swiftbrowse/swiftbrowse)

Platform wheels include the native binary (Linux x86_64/aarch64, macOS arm64/x86_64, Windows x64). Elsewhere, the pure-Python wheel uses a `swiftbrowse` on your `PATH`, or `SWIFTBROWSE_BIN`. Apache-2.0.
