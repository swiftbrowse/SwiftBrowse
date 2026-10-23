# SwiftBrowse strategy (one page)

**ICP** [assumption]: teams building AI agents that must complete web tasks reliably: agent-framework developers and QA/automation engineers moving tests to agents.

**Core pain** [assumption, to validate]: agents driving browsers waste context and money on page dumps, act on the wrong element, and report success when nothing happened. Each failure needs a human to inspect it.

**Positioning**: For developers whose agents drive a browser, SwiftBrowse is the agent-native browser runtime that returns compact, verifiable results, unlike Playwright MCP, which is built on a testing library and leaves verification and safety to the agent.

**Main alternative**: Playwright MCP (`@playwright/mcp`), the default MCP browser for coding agents. The Playwright library is the baseline for scripted tests, not for agents.

**North Star**: verified agent tasks completed per day, i.e. tasks whose outcome SwiftBrowse confirmed.
Inputs: task success rate · false-success rate (target 0) · context bytes per task · tool calls per task · time per task.

**Now**: beat Playwright MCP on the agent task benchmark, published with its method. **Next**: TypeScript/Python SDKs; remote authenticated MCP. **Later**: desktop shell.

**Non-goals (now)**: being a general test runner; replacing Playwright for scripted end-to-end suites.
