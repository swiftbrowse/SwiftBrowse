# Saved logins

Log in once, save the session's state, and start other tests from it. The state is cookies plus the localStorage of the open origins, saved in Playwright's `storageState` format.

## With the test runner

**1. A setup file logs in and saves the state.** Files named `*.setup.json` run before all other tests; if one fails, every other test is skipped.

```json
{
  "name": "member login",
  "tests": [
    {
      "name": "logs in and saves the state",
      "steps": [
        { "op": "navigate", "url": "login.html" },
        { "op": "fill", "target": "Email", "value": "ada@example.com" },
        { "op": "fill", "target": "Password", "value": "${env.TEST_PASSWORD}" },
        { "op": "click", "target": { "by": "role", "role": "button", "name": "Sign in" },
          "expect": { "type": "url_contains", "value": "dashboard" } },
        { "op": "save_storage_state", "name": "member" }
      ]
    }
  ]
}
```

**2. Test files start from it** with `storage_state`:

```json
{
  "name": "members area",
  "storage_state": "member",
  "tests": [
    { "name": "starts logged in", "steps": [
      { "op": "navigate", "url": "members.html" },
      { "op": "expect", "condition": { "type": "text_visible", "text": "Members area for Ada" } }
    ] }
  ]
}
```

Every test gets **its own copy** of the state. A test that logs out or changes localStorage doesn't affect the others.

Files without `storage_state` start signed out, so a suite can test signed-in and guest behavior side by side.

## Where the state is stored

The runner writes it to `<output-dir>/.auth/<name>.json` (default `test-results/.auth/`). The file is readable by your user only. It contains live session cookies, so don't commit it: add `test-results/` to `.gitignore`.

## Anywhere else

`save_storage_state` and `load_storage_state` are ordinary operations, so they work over MCP and in the SDKs as well. Outside the test runner, the state is saved in the evidence directory.

```ts
await page.saveStorageState('member');
// later, in another session:
const other = await browser.newSession();
await other.loadStorageState('member');
await other.navigate(`${base}/members.html`);
```

```python
page.save_storage_state("member")
other = browser.new_session()
other.load_storage_state("member")
```

`load_storage_state` sets the cookies and seeds each origin's localStorage before your next navigation, so the page sees the state on its first load.
