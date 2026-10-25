# Configuration and security

## Environment variables

| Variable | Effect |
|---|---|
| `SWIFTBROWSE_CHROMIUM` | Chromium binary to use for every session. It also disables the automatic switch to the headless shell. |
| `SWIFTBROWSE_HEADLESS_SHELL` | Headless-shell binary for headless sessions. |
| `SWIFTBROWSE_FFMPEG` | ffmpeg binary for video. |
| `SWIFTBROWSE_BIN` | The `swiftbrowse` binary the SDKs run. |
| `SWIFTBROWSE_DAEMON=off` | Don't use the warm-browser daemon; launch a browser per process (same as `--no-daemon`). |
| `SWIFTBROWSE_DAEMONS` | How many warm browsers may run at once. Default: the number of CPUs, at most 8. |
| `SWIFTBROWSE_DAEMON_SOCKET` | Use a private daemon at this socket path, for example to isolate a benchmark. |
| `SWIFTBROWSE_TRANSPORT=websocket` | On Linux and macOS, talk to Chromium over a local DevTools port instead of a pipe. |
| `SWIFTBROWSE_NO_SANDBOX=1` | Run Chromium without its sandbox. This is automatic as root, or when the system has no usable sandbox. |
| `SWIFTBROWSE_TRACE=1` | Print launch and navigation timings to stderr. |
| `PLAYWRIGHT_BROWSERS_PATH` | Where Playwright-installed browsers are looked for, besides the default cache folders. |

## Finding a browser

In order: `--chromium` / `SWIFTBROWSE_CHROMIUM`, then the newest Playwright-installed build (`~/.cache/ms-playwright`, `~/Library/Caches/ms-playwright` or `%LOCALAPPDATA%\ms-playwright`), then standard Chrome and Chromium install locations and `PATH`. Headless sessions use Chromium's headless shell when one is installed (`npx playwright install chromium-headless-shell`). `swiftbrowse doctor` shows what was found.

## The warm-browser daemon

The first headless session starts a background `swiftbrowse daemon` that keeps a Chromium running. Later sessions, whether from another test run, an SDK or an MCP server, attach to it instead of launching a browser. From process start to a loaded page takes about 50 ms instead of about 160 ms.

- **Prepared tabs.** The daemon keeps spare tabs ready, each in a fresh browser context with its renderer started and the session already set up. A new session takes one and navigates immediately.
- **Parallel runs.** Each parallel client gets its own daemon (up to `SWIFTBROWSE_DAEMONS`), so parallel workers don't share one browser's main thread.
- **Isolation.** Every session has its own browser context, with separate cookies, storage and cache, which is disposed when the session closes or its process disconnects. A client can only use the contexts, tabs and sessions it created. Browser-wide commands are refused.
- **Access.** On Linux and macOS, the daemon listens on a Unix socket in a private (`0700`) directory and checks the connecting user's id; the browser runs on a pipe, so no TCP port is opened. On Windows, it uses a named pipe that refuses remote clients. Everywhere, a client must present a random token stored in your private state directory.
- **Lifetime.** A daemon exits after 10 idle minutes, when its browser dies, or on `swiftbrowse daemon stop`. Daemons are tied to the SwiftBrowse version, so different builds never mix.
- **Trade-off.** Daemon sessions share a browser *process* and are separated by browser context. If you need a separate browser process per session, use `--no-daemon`. Headed sessions never use the daemon.

```sh
swiftbrowse daemon status
swiftbrowse daemon stop
```

## Policy

The same options exist on `run`, `test`, `mcp`, `sdk`, `snapshot`, `screenshot` and `capabilities`:

| Option | Effect |
|---|---|
| `--allow-host HOST` | Only allow navigation to these hosts (repeatable; `*.example.com` allowed). |
| `--block-host HOST` | Never navigate to these hosts. |
| `--allow-file-urls` | Permit `file://` URLs. Only `http`, `https`, `about` and `data` are allowed by default. |
| `--allow-evaluate` | Allow `evaluate`. Always on in `run`, `test` and the SDKs' test defaults. |
| `--allow-upload-dir DIR` | Let `upload_file` read files from DIR (repeatable). |
| `--no-approval` | Don't require approval for consequential clicks. On in `test` and the SDKs' test defaults. |
| `--timeout-ms MS` | Default per-operation timeout (10000; in `test`, `--expect-timeout-ms`). |
| `--evidence-dir DIR` | Where screenshots, videos and saved states are written. |
| `--headed` | Show the browser. |
| `--no-daemon` | Launch a browser instead of using the daemon. |

## Security model

- **Consequential clicks.** Clicks whose target text looks like buying, paying, checking out, deleting or transferring return `approval_required` until they are repeated with `approved: true`. The detection uses text patterns, not an understanding of the page, so treat it as a guard rail. You can also mark a click `consequential: true` yourself.
- **Dialogs.** `confirm()` and `prompt()` are dismissed, never accepted on the user's behalf. `alert()` is acknowledged. Both are reported.
- **Secrets.** Password field values are never echoed in results.
- **Files.** Writes go only to the evidence directory, and file names are validated (no path separators). Uploads read only from allowed directories.
- **Page isolation.** SwiftBrowse's in-page helper runs in an isolated JavaScript world, so page scripts can't see it or tamper with it.
- **Cleanup.** Each non-daemon session gets its own browser process and a temporary profile, which are deleted on close. The MCP server cleans up on SIGTERM, SIGINT and SIGHUP. On Linux, browsers also die if the server is killed, and the next launch sweeps up profiles left by dead processes.
- **Root.** Chromium's sandbox can't start as root, so it is turned off there. `doctor` and the capability report warn about it; rely on your container as the boundary.
