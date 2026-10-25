# Network mocking

`route` intercepts the page's requests. A matching request can be answered with your own response, failed with a network error, delayed, or let through. `get_requests` and the `request_seen` condition check what the page sent.

## Fulfill with JSON

```json
{ "op": "route", "url": "**/api/todos", "json": [ { "title": "Mocked one" }, { "title": "Mocked two" } ] }
```

`json` sets the body and `content-type: application/json`. Add the route **before** the page makes the request, which usually means before `navigate`.

## Status codes, bodies and headers

```json
{ "op": "route", "url": "**/api/todos", "status": 500, "body": "boom" }
{ "op": "route", "url": "**/api/me", "status": 401, "json": { "error": "signed out" }, "headers": { "x-request-id": "test" } }
```

## Network errors

```json
{ "op": "route", "url": "**/api/todos", "action": "abort", "error": "ConnectionRefused" }
```

Errors: `Failed`, `Aborted`, `TimedOut`, `AccessDenied`, `ConnectionClosed`, `ConnectionReset`, `ConnectionRefused`, `NameNotResolved`, `InternetDisconnected`, `BlockedByClient`.

## Slow responses

```json
{ "op": "route", "url": "**/api/todos", "json": [], "delay_ms": 600 }
```

Use it to test loading states:

```json
{ "op": "click", "target": "Retry", "expect": { "type": "text_visible", "text": "Loading…" } }
```

## Matching

| Field | Meaning |
|---|---|
| `url` | A glob: `**` matches anything, `*` anything except `/`, `?` one character. `**/api/todos*` matches `https://x.test/api/todos?page=2`. |
| `url_regex` | A regular expression matched against the full URL, instead of `url`. |
| `method` | Only this HTTP method, e.g. `"POST"`. |
| `times` | Only the first N matching requests; after that the route removes itself. |
| `action` | `fulfill` (the default when you give a status, body or JSON), `abort`, or `continue` to let the request through unchanged. |

With neither `url` nor `url_regex`, the route matches every request.

**The newest route wins.** Add a general route first and a more specific or temporary one after it:

```json
{ "op": "route", "url": "**/api/todos", "json": [ { "title": "Back again" } ] },
{ "op": "route", "url": "**/api/todos", "status": 503, "times": 1 },
{ "op": "navigate", "url": "api.html" },
{ "op": "expect", "condition": { "type": "text_visible", "text": "HTTP 503" } },
{ "op": "click", "target": "Retry", "expect": { "type": "text_visible", "text": "Back again" } }
```

The first request gets the 503. The one-time route then removes itself, and the retry gets the data.

## Different answers per method

```json
{ "op": "route", "url": "**/api/todos", "method": "GET", "json": [] },
{ "op": "route", "url": "**/api/todos", "method": "POST", "status": 201, "json": { "id": 42 } }
```

## Checking what the page sent

```json
{ "op": "expect", "condition": { "type": "request_seen", "url_contains": "/api/todos", "method": "POST" } }
```

`get_requests` returns every request the page made (method, URL, status, and whether it was mocked), newest last. `list_routes` shows the active routes and how many requests each has matched.

## Removing routes

```json
{ "op": "unroute", "id": "r1" }
{ "op": "unroute" }
```

The first form removes one route by the `id` that `route` returned; the second removes them all. Routes belong to the session, so they also go away when the test ends.

## Scope

Routes apply to every tab and frame in the session, including popups and cross-site iframes. Interception is switched on only while at least one route exists, so tests without routes pay nothing for it.

## In code

```ts
await page.route({ url: '**/api/todos', json: [{ title: 'Mocked' }] });
await page.navigate(`${base}/api.html`);
await page.expect({ type: 'text_visible', text: 'Mocked' });
```

```python
page.route(url="**/api/todos", status=500, body="boom")
```
