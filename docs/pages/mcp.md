# MCP server

`swiftbrowse mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) over stdio, so AI agents (Claude Code, Claude Desktop, Cursor and other MCP clients) can drive a real browser.

## Setup

```json
{
  "mcpServers": {
    "swiftbrowse": { "command": "swiftbrowse", "args": ["mcp"] }
  }
}
```

With npx: `"command": "npx", "args": ["-y", "swiftbrowse", "mcp"]`. In Claude Code:

```sh
claude mcp add swiftbrowse -- swiftbrowse mcp
```

The server starts launching the default session's browser right away, so the first tool call doesn't wait (turn this off with `--no-prewarm`). It supports MCP protocol versions 2024-11-05, 2025-03-26 and 2025-06-18.

## Tools

Every [operation](operations.md) is a tool with the same name and arguments, plus:

| Tool | Does |
|---|---|
| `session_create` | Start another isolated session (its own cookies, storage and tabs). |
| `session_close` | Close a session. |
| `session_list` | List open sessions. |

Tools use the `default` session unless given a `session_id`.

## Built for agents

- **Compact output.** Tool results reach the agent as short text, for example `ok click button "Sign up" [e12] · verified: text_visible`. The full structured result is in `structuredContent`.
- **Bounded snapshots.** `get_page_snapshot` returns one line per interactive element with a `ref` (`textbox "Email" [e3] type=email`), capped at 150 elements. `since_last: true` returns only what changed (`+`, `~`, `-`).
- **Act and verify in one call.** `click` with `expect` performs the click and confirms the outcome, so the agent doesn't need a separate wait.
- **Honest failures.** Errors say what kind they are and whether retrying could help. A click that was delivered is never silently retried.

In a benchmark of typical agent tasks, this used about half the tool calls of Playwright MCP and far fewer bytes of context (see [Benchmarks](benchmarks.md)).

## Safety defaults

The MCP server is careful by default, because an agent is acting on the user's behalf:

- Clicks on elements that look consequential (buy, pay, checkout, delete, transfer, …) return `approval_required` until they are repeated with `approved: true`. Pass `--no-approval` only for test environments.
- `evaluate` (running JavaScript) is off unless you pass `--allow-evaluate`.
- `upload_file` reads nothing unless you pass `--allow-upload-dir DIR`.
- `file://` URLs need `--allow-file-urls`. Restrict navigation with `--allow-host` and `--block-host`.
- `confirm()` and `prompt()` dialogs are dismissed, never accepted. Password values never appear in results.

```json
{ "command": "swiftbrowse", "args": ["mcp", "--allow-host", "staging.example.com", "--allow-upload-dir", "/home/me/fixtures"] }
```

## From agent session to test

Every tool call is an operation, so a flow an agent worked out can be saved as a test file. Put the tool calls in `steps` as `{ "op": <tool>, ...arguments }` and run them with `swiftbrowse test` or `swiftbrowse run`.
