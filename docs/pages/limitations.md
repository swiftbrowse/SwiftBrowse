# Limitations and roadmap

SwiftBrowse is at version {{version}}. Here is what it doesn't do yet, so you don't have to find out the hard way.

## Not supported yet

- **Chromium only.** No Firefox or WebKit.
- **No visual comparison.** There are no screenshot assertions or pixel diffs.
- **No downloads.** File downloads are reported as unsupported in the capability report.
- **No fixtures or custom reporters inside the SDKs.** Use your test framework's (pytest fixtures, `node:test` hooks, Vitest reporters). The JSON test runner has its own reporters.
- **No mobile emulation presets** beyond setting the viewport.

## Known limitations

- **One frame at a time.** Actions and locators work in the current frame; use `switch_frame` to move between frames.
- **CSS-scaled iframes.** Clicks inside an iframe scaled with CSS `transform` are refused rather than mapped, because the click guard detects that they would miss.
- **Shadow DOM and CSS.** CSS selectors don't pierce shadow roots. Role, label, text and test-id locators do look inside open shadow roots.
- **Consequential-action detection** (MCP's approval prompts) uses text patterns, not an understanding of the page.
- **Traces** take a DOM snapshot after each step, not before and after. Cross-origin stylesheets stay as links in the snapshot.
- **Daemon sessions** share a browser process and are isolated by browser context. Use `--no-daemon` if you need separate processes.

## Planned

- Desktop app shell (Tauri/Wry) for headed, local agent use
- Remote, authenticated MCP transport
- Visual comparison
- Downloads

Something missing that you need? [Open an issue](https://github.com/swiftbrowse/SwiftBrowse/issues) and say what you're trying to test.
