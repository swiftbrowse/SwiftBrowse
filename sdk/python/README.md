# swiftbrowse

**The AI browser for browser automation and end-to-end testing.** A native engine (Rust, Chromium over the DevTools Protocol) with one contract across a test runner, an MCP server for AI agents, and this SDK. Standard library only.

```sh
pip install swiftbrowse
npx playwright install chromium chromium-headless-shell   # a Chromium for SwiftBrowse to drive
swiftbrowse doctor                                       # what this machine can run
```

## Tests in code

```python
# pytest or unittest
from swiftbrowse import launch

def test_signs_in():
    with launch() as browser, browser.new_session() as page:
        page.navigate("http://localhost:3000/login")
        page.fill("Email", "ada@example.com")
        page.click({"by": "role", "role": "button", "name": "Sign in"}, expect={"type": "url_contains", "value": "/dashboard"})
        page.expect({"type": "text_visible", "text": "Welcome, Ada"})
```

Every engine operation is a method with its documented signature (`navigate`, `click`, `fill`, `check`, `hover`, `upload_file`, `route`, `switch_tab`, `switch_frame`, `save_storage_state`, …): required arguments positionally, the rest by keyword. A failed action or `expect` raises `SwiftBrowseError` with the full result attached.

## The CLI

`swiftbrowse test` runs JSON test files with parallel workers, retries, traces, video and JUnit/HTML reports; `swiftbrowse mcp` serves the Model Context Protocol for agents. See the [main README](https://github.com/swiftbrowse/swiftbrowse#readme).

Platform wheels include the native binary (Linux x86_64/aarch64, macOS arm64/x86_64, Windows x64). Elsewhere, the pure-Python wheel uses a `swiftbrowse` on your `PATH`, or `SWIFTBROWSE_BIN`.
