# Pitch: agent task benchmark vs Playwright MCP   (appetite: half a day · Standard)

**Problem:** "faster than Playwright" doesn't say whether agents do better. Agents pay in context tokens, tool calls and wrong or unverified actions. [assumption: these dominate agent cost more than single-digit milliseconds]

**Outcome:** a reproducible run where SwiftBrowse MCP beats Playwright MCP on time, context bytes and tool calls per verified task, with success held at 100% for both. **Guardrail:** no task-specific tuning; same Chromium, pages and viewport.

**Solution:** `bench/agents.mjs` drives both MCP servers over stdio as an agent would: read a snapshot, take refs from it, act, confirm the outcome.
- Task A, signup form (fill, fill, select, check, submit, confirm the welcome text).
- Task B, 400-row inventory page (find and click "Edit row 250", confirm the effect).
- Cold start: spawn to first loaded page.

**No-gos:** LLM-in-the-loop runs (cost and nondeterminism; a later step); third-party sites.

**Riskiest assumption → test:** Playwright MCP's default 500 ms settle explains its time. Test: also run it with `--timeout-settle 0`. Kill threshold: if SwiftBrowse loses to the tuned run on time, don't claim time; claim only context and verification.
