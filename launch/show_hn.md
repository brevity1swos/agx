# Show HN Post

## Title

Show HN: agx – step through your AI agent's session in the terminal

## URL

https://github.com/brevity1swos/agx

## Text

I kept scrolling thousands of lines of JSONL trying to figure out what a coding agent actually did in a session — which tool it called, with what input, and what came back. agx turns those files into a navigable timeline instead.

It reads the session files your agent CLI *already writes* — no SDK, no hosted dashboard, no telemetry. Point it at a Claude Code / Codex / Gemini session (or an OpenAI-generic, LangChain, Vercel AI SDK, or OpenTelemetry GenAI export) and you get a color-coded, vim-bound timeline of user turns, assistant turns, tool calls, and tool results. The part I wanted most: selecting a result shows the original tool-call input and the response in one detail view, so you stop scrolling back and forth. Per-step token counts and USD cost estimates, a corpus mode for aggregating across many sessions, and a heuristic PII scanner are in there too.

There's also `agx-mcp`, an MCP server that exposes the same introspection to the agent itself mid-run — it can ask "summarize my session," "what were my recent errors," "which tool am I calling in a loop" and self-correct without you reading the trace.

It's pure Rust (ratatui + crossterm), runs on Linux/macOS/Windows, and parses 8 formats by auto-detecting the file shape. It is deliberately *not* a Langfuse/LangSmith competitor — those are hosted team observability with retention and alerting; agx is the local thing you reach for when you're already in the terminal. Feedback welcome.

`cargo install agx-tui` (binary is `agx`).

## Likely questions to prep

- **vs Langfuse / LangSmith / Helicone?** Those are hosted, team-scale observability (retention, dashboards, alerting) and usually need instrumentation. agx is local, zero-setup, terminal-native, and reads files that already exist. Different job; they coexist.
- **Why not just `jq`/`less` the JSONL?** Raw traces don't pair `tool_use` ↔ `tool_result` (they're separated by many lines, and the linking differs per CLI), don't sum tokens/cost, and aren't navigable. agx normalizes 8 formats into one timeline.
- **The MCP server — why?** Agents can't cheaply scroll their own transcript (tokens, context cost). `agx-mcp` gives the agent a queryable index of its own session so it can detect retry loops / budget blowups mid-run.
- **AI-built?** Yes — happy to discuss the workflow. The tool is a normal Rust TUI; judge it on whether it's useful.
- **Vendor risk?** If Claude Code ships a first-party viewer, agx's edge is cross-CLI breadth + the agent-self-introspection MCP surface, which a single-vendor viewer won't cover.
