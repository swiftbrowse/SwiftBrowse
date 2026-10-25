# Test files

A test file is JSON. The runner picks up every `*.test.json` and `*.spec.json`, plus `*.setup.json` files that run before everything else. Each test is a list of steps, and each step is one [operation](operations.md).

## A complete file

```json
{
  "name": "sign in",
  "base_url": "http://localhost:3000/",
  "vars": { "email": "ada@example.com" },
  "tags": ["auth"],
  "before_each": [ { "op": "navigate", "url": "login" } ],
  "after_each": [ { "op": "expect", "condition": { "type": "no_console_errors" } } ],
  "tests": [
    {
      "name": "signs in",
      "tags": ["smoke"],
      "steps": [
        { "op": "fill", "target": "Email", "value": "${email}" },
        { "op": "fill", "target": "Password", "value": "${env.TEST_PASSWORD}" },
        { "op": "click", "target": { "by": "role", "role": "button", "name": "Sign in" },
          "expect": { "type": "url_contains", "value": "dashboard" } },
        { "op": "expect", "condition": { "type": "text_equals", "target": { "by": "test_id", "id": "hello" }, "text": "Welcome, ${email}" } }
      ]
    },
    {
      "name": "rejects a wrong password",
      "retries": 2,
      "timeout_ms": 10000,
      "steps": [
        { "op": "fill", "target": "Email", "value": "${email}" },
        { "op": "fill", "target": "Password", "value": "wrong" },
        { "op": "click", "target": "Sign in" },
        { "op": "expect", "condition": { "type": "text_visible", "text": "Wrong email or password" } }
      ]
    },
    { "name": "resets the password", "skip": "waiting for the mail server", "steps": [] }
  ]
}
```

## File fields

| Field | Meaning |
|---|---|
| `name` | Suite name shown in reports. Default: the file name without `.test.json`. |
| `base_url` | Relative `navigate` and `new_tab` URLs are resolved against it. `--base-url` and `--serve` override it. |
| `vars` | Values for `${name}` placeholders anywhere in the file. `--var name=value` overrides them. |
| `tags` | Tags added to every test in the file. |
| `before_each` | Steps run before every test, in the test's session. |
| `after_each` | Steps run after every test, **even when the test failed**. |
| `storage_state` | Name of a saved state to start every test from: see [Saved logins](auth.md). |
| `tests` | The tests. |
| `steps` | Instead of `tests`: a file that is one test (the same format as a `swiftbrowse run` script). |

Unknown fields are an error, so a typo such as `befor_each` is reported instead of being ignored.

## Test fields

| Field | Meaning |
|---|---|
| `name` | Required. Shown as `suite › name`, with tags appended as `@tag`. |
| `steps` | Required. The steps, in order. The test stops at the first failing step. |
| `tags` | Tags for filtering with `--grep @smoke`. |
| `skip` | `true` or a reason string. The test is reported as skipped. |
| `only` | Run only the tests marked `only` (across all files). `--forbid-only` makes this an error on CI. |
| `retries` | Retries for this test, overriding `--retries`. |
| `timeout_ms` | Time budget for one attempt of this test, including hooks. Default 30 s (`--test-timeout-ms`). |

## Steps

A step is an object with `op` plus the operation's arguments:

```json
{ "op": "select_option", "target": "Country", "option": "Norway" }
```

Every [operation](operations.md) can be a step: `navigate`, `click`, `fill`, `type_text`, `press_key`, `select_option`, `check`, `hover`, `upload_file`, `go_back`, `go_forward`, `reload`, `evaluate`, `wait_for`, `route`, `switch_tab`, `switch_frame`, `save_storage_state`, `get_screenshot` and the rest.

The test runner adds one more: **`expect`**, an auto-waiting assertion with either one `condition` or a list of `conditions`. It retries until every condition holds or the expect timeout (5 s, `--expect-timeout-ms`, or `timeout_ms` on the step) runs out. See [Assertions](assertions.md).

```json
{ "op": "expect", "conditions": [
  { "type": "url_contains", "value": "/cart" },
  { "type": "count", "target": { "by": "css", "selector": ".item" }, "count": 3 }
] }
```

### Verifying an action's outcome

`click`, `hover` and `press_key` accept an `expect` condition of their own. The action counts as successful only if the condition holds afterwards:

```json
{ "op": "click", "target": "Add to cart", "expect": { "type": "text_visible", "text": "1 item" } }
```

`fill`, `select_option`, `check` and `upload_file` always verify their result (the field's value, the selected option, the checked state, the chosen files).

## Variables

- `${name}`: from the file's `vars`, overridden by `--var name=value`.
- `${env.NAME}`: from the environment. Use it for secrets so they stay out of test files.

An undefined variable is an error before any test runs (exit code 2). Variables work in any string, including URLs, targets and conditions.

## Setup files

A file named `*.setup.json` runs before all other tests. If any setup test fails, the other tests are skipped. Use it to log in once and save the state; see [Saved logins](auth.md).

## Scripts

A file with top-level `steps` instead of `tests` is a single test. That's also the format of `swiftbrowse run`, which runs one script and prints every result as JSON:

```json
{
  "name": "check the homepage",
  "steps": [
    { "op": "navigate", "url": "http://localhost:3000/" },
    { "op": "assert_page_state", "conditions": [ { "type": "title_equals", "value": "Hello" } ] }
  ]
}
```

```sh
swiftbrowse run check.json
```

A script stops at its first failing step unless it sets `"stop_on_failure": false` (only `run` supports that).

## Isolation

Every test runs in a fresh, isolated browser context with its own cookies, localStorage, cache and tabs. Tests don't share state unless you share it on purpose with a [saved state](auth.md). Tests in different files and tests in the same file run in parallel up to `--workers`.

## Policy inside tests

Tests target test environments, so the runner is permissive where the MCP server is careful:

- `evaluate` (running JavaScript in the page) is allowed.
- Clicks on buttons such as "Delete" or "Pay" need no approval.
- `upload_file` may read files from the working directory and the test file's directory. Relative paths are resolved against the test file.

See [Configuration and security](configuration.md) for the full policy.
