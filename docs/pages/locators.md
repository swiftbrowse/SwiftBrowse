# Targets and locators

Every operation that acts on an element takes a `target`: either a plain string or a locator object.

## Strings

A string is the quickest way to point at something:

```json
{ "op": "fill", "target": "Email", "value": "ada@example.com" }
{ "op": "click", "target": "Sign in" }
```

A string tries these strategies in order and uses the first that matches:

1. accessible name (what a screen reader announces: button text, `aria-label`, `alt`, …)
2. form label (`<label for>`, a wrapping `<label>`, or `aria-label`)
3. test id (`data-testid`, `data-test-id`, `data-test`)
4. exact text
5. partial text

The result reports which strategy matched (`target.strategy`), so you can tell what was found.

## Locator objects

When a string is ambiguous or you want to be exact, use a locator with `by`:

| Locator | Example | Matches |
|---|---|---|
| `role` | `{ "by": "role", "role": "button", "name": "Save" }` | Elements with this ARIA role (implicit or explicit) and, optionally, this accessible name. Add `"exact": true` for an exact name match. `"role": "*"` matches any non-generic role. |
| `label` | `{ "by": "label", "text": "Email" }` | Form controls whose label or `aria-label` matches. `exact` is optional. |
| `test_id` | `{ "by": "test_id", "id": "cart-count" }` | `data-testid`, `data-test-id` or `data-test`. |
| `text` | `{ "by": "text", "text": "Welcome back" }` | The innermost elements whose text matches. `exact` is optional. |
| `css` | `{ "by": "css", "selector": "form .error" }` | A CSS selector. |
| `xpath` | `{ "by": "xpath", "expr": "//table//tr[3]" }` | An XPath expression. |
| `ref` | `{ "by": "ref", "ref": "e12" }` | A handle from a page snapshot or an earlier lookup (mostly for agents). |
| `auto` | `{ "by": "auto", "query": "Email" }` | The same as a plain string. |

Name and text matching ignores case and collapses whitespace. Without `exact` it matches substrings ("Save" matches "Save draft"); with `"exact": true` the whole name must match.

Prefer `role`, `label`, `test_id` and text over CSS and XPath. They follow what the user sees, so they survive markup changes and they check that your page is accessible.

## Several matches: `nth` and the selection rule

The rule for choosing among several matches is strict, and the same everywhere:

1. If at least one match is **visible**, hidden matches are ignored.
2. If more than one match remains and no `nth` is given, the step fails with **`ambiguous`**. The error lists the candidates.
3. With `nth`, the zero-based match in document order is used.

```json
{ "op": "click", "target": { "by": "role", "role": "button", "name": "Delete", "nth": 2 } }
```

SwiftBrowse never silently picks the first of several matches, because that's how tests end up clicking the wrong row.

## What happens before an action

Before `click`, `hover`, `check`, `fill`, `select_option` and the other actions touch an element, SwiftBrowse waits (up to the action's timeout) until:

- the element exists and matches exactly one candidate
- it is visible and has a size
- it is enabled (for actions that need it)
- it isn't covered by another element at the point where the mouse will land
- it has stopped moving (animations and layout shifts have settled)
- the page has finished parsing its scripts

The point is checked again **as the mouse goes down and up**. If a layout shift would make the click land on a different element, the click is blocked and retried instead of hitting the wrong thing.

Failures that happen before any input reaches the page (not found yet, hidden, disabled, covered, moving, stale) are retried automatically until the timeout. A click that was actually delivered is **never** repeated, so an action can't be performed twice. See [Results and errors](results.md).

## Frames and shadow DOM

- Locators search the current frame. Use [`switch_frame`](tabs-frames-uploads.md) to act inside an iframe.
- `role`, `label`, `text` and `test_id` look inside open shadow roots; CSS selectors don't pierce shadow roots.
