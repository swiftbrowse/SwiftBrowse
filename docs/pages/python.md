# Python

```sh
pip install swiftbrowse
```

The `swiftbrowse` package uses the standard library only and needs Python 3.8+. The platform wheels include the native binary. It works with pytest, unittest or plain scripts.

## A test with pytest

```python
import pytest
from swiftbrowse import launch

@pytest.fixture(scope="session")
def browser():
    with launch() as b:
        yield b

@pytest.fixture
def page(browser):
    with browser.new_session() as s:
        yield s

def test_adds_a_todo(page):
    page.navigate("http://localhost:3000/")
    page.fill("New todo", "Write docs")
    page.press_key("Enter", expect={"type": "text_visible", "text": "Write docs"})
    page.expect([
        {"type": "count", "target": {"by": "role", "role": "listitem"}, "count": 1},
        {"type": "no_console_errors"},
    ])
```

Each `new_session()` is isolated: its own cookies, storage and tabs. It attaches to the warm browser daemon, so creating one per test is cheap.

## API

### `launch(executable=None, *, ...) → Browser`

| Keyword | Default | Meaning |
|---|---|---|
| `test_defaults` | `True` | No approval prompts for "Delete" or "Pay" clicks, `evaluate` allowed, uploads from the working directory. Set it to `False` for the agent-safe policy. |
| `headed` | `False` | Show browser windows. |
| `no_daemon` | `False` | Launch a browser for this process instead of attaching to the warm daemon. |
| `chromium` | discovered | Path to a Chromium binary. |
| `evidence_dir` | temp directory | Where screenshots, videos and saved states go. |
| `timeout_ms` | 10000 | How long actions wait for their target, in ms. |
| `allow_hosts` / `block_hosts` | | Restrict navigation. |
| `upload_dirs` | | Extra directories `upload_file` may read from. |
| `allow_file_urls` | `False` | Permit `file://` navigation. |
| `throw_on_error` | `True` | Raise `SwiftBrowseError` when an operation fails. |

`Browser` and `Session` are context managers, so `with` closes them.

### `browser.new_session(*, headless=None, viewport=None, throw_on_error=None) → Session`

`viewport` is `(width, height)`; the default is `(1280, 800)`.

### Session methods

Every [operation](operations.md) is a method with its documented signature: required arguments positionally, the rest by keyword.

```python
page.navigate(url, wait_until="network_idle")
page.click("Save", expect={"type": "text_visible", "text": "Saved"}, timeout_ms=10000)
page.fill({"by": "label", "text": "Email"}, "ada@example.com")
page.select_option("Country", "Norway")
page.check("I agree")
page.upload_file("Documents", ["fixtures/notes.txt"])
page.route(url="**/api/todos", json=[])
page.switch_tab(url_contains="checkout")
page.switch_frame(name="payment")
page.save_storage_state("member")
shot = page.get_screenshot(full_page=True)
```

Plus:

- `page.expect(condition_or_list, *, timeout_ms=5000, name=None)`: an auto-waiting assertion. See [Assertions](assertions.md).
- `page.text()`: the page as compact text.
- `page.run(op, args)`: any operation by name.

### Results and errors

Methods return the [`ActionResult`](results.md) as a dict. If the operation failed, they raise `SwiftBrowseError` instead (unless `throw_on_error=False`), with `.kind` and the full `.result`.

```python
from swiftbrowse import SwiftBrowseError

try:
    page.click("Missing", timeout_ms=500)
except SwiftBrowseError as e:
    assert e.kind == "not_found"
```

## Threads

A `Browser` sends one request at a time; calls from several threads are serialized. For parallel tests, use pytest-xdist: each worker process gets its own `launch()`.

## The binary

The SDK uses, in order: the `executable` argument, `$SWIFTBROWSE_BIN`, the binary inside the installed wheel, then `swiftbrowse` on `PATH`. The package also installs the `swiftbrowse` command (`swiftbrowse test`, `swiftbrowse mcp`, …), and `python -m swiftbrowse` works too.
