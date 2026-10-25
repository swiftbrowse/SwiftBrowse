# Tabs, frames and uploads

## Tabs and popups

A session can have several tabs: links with `target="_blank"`, `window.open` popups, and tabs you open yourself. Actions apply to the **active** tab.

```json
{ "op": "click", "target": "Open about in a new tab" },
{ "op": "switch_tab", "url_contains": "about.html" },
{ "op": "expect", "condition": { "type": "title_equals", "value": "About" } },
{ "op": "close_tab" }
```

| Operation | What it does |
|---|---|
| `switch_tab` | Makes a tab active by `index`, `url_contains`, `title_contains`, or `last: true` (the newest). It **waits** for a matching tab to appear, so you can call it right after the click that opens a popup. |
| `new_tab` | Opens a tab, optionally at a `url`, and makes it active. |
| `list_tabs` | Lists the tabs with their index, URL and title, and marks the active one. |
| `close_tab` | Closes the active tab (or `index`). Its opener, or else the last tab, becomes active. |

A popup sign-in:

```json
{ "op": "click", "target": "Sign in with popup" },
{ "op": "switch_tab", "last": true },
{ "op": "fill", "target": "Email", "value": "ada@example.com" },
{ "op": "click", "target": "Continue" },
{ "op": "switch_tab", "index": 0 },
{ "op": "expect", "condition": { "type": "text_visible", "text": "Signed in" } }
```

A click that opens a new tab returns as soon as the tab exists; it doesn't wait for the original page to change.

## Frames

Actions and locators work in one frame at a time. `switch_frame` moves into an iframe, same-site or cross-site, and nested iframes one level at a time:

```json
{ "op": "switch_frame", "target": { "by": "css", "selector": "iframe[name=payment]" } },
{ "op": "fill", "target": "Card number", "value": "4242 4242 4242 4242" },
{ "op": "switch_frame", "main": true }
```

| Argument | Selects |
|---|---|
| `target` | The `<iframe>` element, located in the current frame. Use this to go into nested frames step by step. |
| `name` | A frame by its `name` attribute, anywhere in the tab. |
| `url_contains` | The first frame whose URL contains this text, anywhere in the tab. |
| `main: true` | Back to the tab's top-level page. |

`switch_frame` waits for the frame to exist. You stay in the frame until the next `switch_frame` or a navigation of the tab. `list_frames` lists the tab's frames with their names and URLs.

Clicks inside frames use real mouse input at the right position on the page, including frames inside frames. Clicks inside an iframe scaled with CSS `transform` are refused rather than risk missing (see [Limitations](limitations.md)).

## File uploads

`upload_file` sets files on an `<input type="file">` without opening a file dialog, and verifies the selection. It works on inputs hidden behind a styled button:

```json
{ "op": "upload_file", "target": "Documents", "files": ["fixtures/notes.txt", "fixtures/contacts.csv"] }
```

The target must resolve to the file input itself. Its label works (`"Documents"` above), as do a test id or CSS. Targeting the styled button instead is an error that says to target the input.

For safety, uploads only read from allowed directories:

| Where | Allowed directories | Relative paths are resolved against |
|---|---|---|
| Test runner | the working directory and the test file's directory | the test file's directory |
| SDKs (default test settings) | the working directory | the working directory |
| MCP and `run` | none, unless you pass `--allow-upload-dir DIR` | the working directory |

A file outside these directories is refused with `permission_denied`.

## Dialogs

`alert()` is acknowledged. `confirm()` and `prompt()` are **dismissed**: SwiftBrowse never accepts on the user's behalf. Both are reported in the result's notes and in page snapshots.
