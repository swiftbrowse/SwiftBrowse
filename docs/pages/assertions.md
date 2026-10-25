# Assertions

Assertions in SwiftBrowse **wait**: they check the page repeatedly until every condition holds, and fail only when the timeout runs out (5 s by default). You don't need sleeps or manual waits.

## Where conditions are used

| Where | Form |
|---|---|
| Test files | `{ "op": "expect", "condition": {…} }` or `"conditions": [{…}, {…}]` |
| Any interface | `assert_page_state` with `conditions` (recorded in the test report), `wait_for` with one `condition` |
| After an action | `click`, `hover` and `press_key` take `"expect": {…}` to verify their outcome |
| TypeScript SDK | `await page.expect({…})` or `page.expect([{…}, {…}], { timeoutMs: 10000 })` |
| Python SDK | `page.expect({…})` or `page.expect([...], timeout_ms=10000)` |

## Conditions

| `type` | Fields | Passes when |
|---|---|---|
| `url_equals` | `value` | The URL is exactly `value`. |
| `url_contains` | `value` | The URL contains `value`. |
| `url_matches` | `pattern` | The full URL matches the regular expression. |
| `title_equals` | `value` | The title is exactly `value`. |
| `title_contains` | `value` | The title contains `value`. |
| `visible` | `target` | At least one match is visible (or the `nth` match is). |
| `hidden` | `target` | No match is visible (including when nothing matches). |
| `count` | `target`, `count` | Exactly `count` elements match, visible or not. |
| `text_visible` | `text` | The text is visible somewhere on the page. |
| `text_absent` | `text` | The text is not visible anywhere. |
| `text_equals` | `target`, `text` | The element's whole visible text, whitespace-normalized, equals `text`. |
| `text_contains` | `target`, `text` | The element's visible text contains `text`. |
| `value_equals` | `target`, `value` | A form field's value equals `value`. |
| `enabled` | `target` | The element is enabled. |
| `checked` | `target`, `checked` | A checkbox, radio or `aria-checked` element is (`true`) or isn't (`false`) checked. |
| `no_console_errors` | | No console errors or uncaught exceptions so far in this session. |
| `no_network_failures` | | No failed requests or HTTP responses ≥ 400 so far in this session. |
| `request_seen` | `url_contains`, optional `method` | The page has sent a request whose URL contains this (with this method). |

`target` takes the same strings and locators as actions: see [Targets and locators](locators.md).

## Examples

```json
{ "op": "expect", "condition": { "type": "url_matches", "pattern": "/orders/\\d+$" } }

{ "op": "expect", "conditions": [
  { "type": "text_equals", "target": { "by": "role", "role": "status" }, "text": "3 items" },
  { "type": "hidden", "target": { "by": "css", "selector": ".spinner" } },
  { "type": "no_console_errors" }
] }

{ "op": "click", "target": "Save",
  "expect": { "type": "request_seen", "url_contains": "/api/profile", "method": "PUT" } }
```

## What a failure shows

When an assertion fails, the result lists every condition with `passed` and what was **observed**: the actual URL, the actual text, how many elements matched. You see why it failed, not just that it did. The test runner prints this and keeps a screenshot and a text snapshot of the page. With `--trace` you can also step through the run (see [Traces, video and reports](debugging.md)).

## Timeouts

| Setting | Default | Applies to |
|---|---|---|
| `--expect-timeout-ms` | 5000 | `expect` steps in test files |
| `timeout_ms` on a step | | That step only |
| `timeoutMs` / `timeout_ms` on SDK `expect` | 5000 | That assertion |
| `--test-timeout-ms` / `timeout_ms` on a test | 30000 | One attempt of a whole test, including hooks |
