# Results and errors

Every operation, from every interface, returns the same `ActionResult`.

```json
{
  "ok": true,
  "op": "click",
  "action_id": "s1-2",
  "session_id": "s1",
  "target": {
    "element": { "ref": "e1", "role": "button", "name": "Sign in", "tag": "button" },
    "strategy": "role",
    "candidates": 1
  },
  "page": { "url": "http://localhost:3000/dashboard", "title": "Dashboard" },
  "verification": {
    "status": "passed",
    "checks": [
      { "condition": { "type": "url_contains", "value": "dashboard" }, "passed": true,
        "observed": { "url": "http://localhost:3000/dashboard" } }
    ]
  },
  "timing": { "started_at_ms": 1792905230645, "duration_ms": 59, "timeout_ms": 10000 }
}
```

Fields that would be empty (`data`, `error`, `evidence`, `notes`) are left out.

| Field | Meaning |
|---|---|
| `ok` | The operation succeeded **and** any requested verification passed. |
| `target` | The element acted on, which locator strategy found it, and how many candidates there were. |
| `page` | URL and title after the operation. |
| `verification.status` | `passed`, `failed`, or `not_checked`. A click without `expect` reports `not_checked`: the input was delivered, but nothing confirmed the outcome. |
| `verification.checks` | Each condition with `passed` and what was `observed`. |
| `data` | The operation's output: a snapshot, an evaluate result, a list of tabs, … |
| `error` | Present when `ok` is false: `kind`, `message`, `recoverable`, and sometimes `details` (such as the candidates of an ambiguous locator). |
| `evidence` | Files written (screenshots, videos, saved states), with paths. |
| `notes` | What else happened: `navigated`, `url_changed`, `dom_changed`, a dialog that was dismissed, a wait for a client-rendered page, … |

## Error kinds

| `kind` | Recoverable | Meaning |
|---|---|---|
| `not_found` | yes | No element matched (yet). |
| `not_actionable` | yes | The element is hidden, disabled, covered or moving. |
| `stale_element` | yes | The element was removed or replaced. |
| `timeout` | yes | A wait or an operation ran out of time. |
| `ambiguous` | no | Several elements matched and no `nth` was given. `details` lists them. |
| `verification_failed` | no | The action happened, but its `expect` didn't hold. Don't repeat it blindly. |
| `approval_required` | no | A consequential click needs `approved: true` (MCP's default policy). |
| `permission_denied` | no | The policy forbids it (host not allowed, upload outside allowed directories, `evaluate` off). |
| `navigation_failed` | no | The page didn't load (DNS, connection, certificate, …). |
| `invalid_argument` | no | The request is malformed. |
| `unsupported` | no | This backend can't do it. |
| `session_not_found`, `session_limit` | no | Session problems. |
| `backend` | no | The browser failed unexpectedly. |

"Recoverable" means a later attempt could succeed without changing the request. SwiftBrowse already retries these automatically until the action's timeout, but only when the failure happened **before** any input reached the page. A click that was delivered is never retried.

A failed interaction also attaches a screenshot to `evidence`.

## The test report

`get_test_report` returns the session's verdict: every assertion with its result, failed actions, console errors, network failures and evidence. The test runner builds its reports from the same data.
