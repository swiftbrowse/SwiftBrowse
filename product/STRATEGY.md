# SwiftBrowse strategy (one page)

**ICP** [assumption]: teams building AI agents that must complete web tasks reliably: agent-framework developers and QA/automation engineers moving tests to agents.

**Core pain** [assumption, to validate]: agents driving browsers waste context and money on page dumps, act on the wrong element, and report success when nothing happened. Each failure needs a human to inspect it.

**Positioning**: SwiftBrowse is the AI browser for browser automation and end-to-end testing. For developers whose agents drive a browser, and teams that want those runs to become CI tests, it returns compact, verifiable results from one contract, unlike Playwright MCP, which leaves verification and safety to the agent, and unlike Playwright Test, which needs hand-written selectors and code.

**Main alternative**: Playwright MCP (`@playwright/mcp`), the default MCP browser for coding agents. The Playwright library is the baseline for scripted tests, not for agents.

**North Star**: verified agent tasks completed per day, i.e. tasks whose outcome SwiftBrowse confirmed.
Inputs: task success rate · false-success rate (target 0) · context bytes per task · tool calls per task · time per task.

**Now**: beat Playwright MCP on the agent task benchmark, published with its method. **Next**: publish the SDKs (npm, PyPI) with prebuilt binaries; re-run the Playwright Test benchmark with code-based suites; DOM snapshots in traces; remote authenticated MCP. **Later**: desktop shell.

**Non-goals (now)**: cross-browser testing (Chromium only); parity with Playwright Test's full feature set before the basics above.
