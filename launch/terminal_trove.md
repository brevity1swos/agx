# Terminal Trove Submission

Submit via https://terminaltrove.com/submit (curated form, no hard star bar;
rgx is already listed, so the channel is warm). Fields below map 1:1 to the
submission form; description fields respect the form's character limits.

> Image assets are ready: `assets/preview.png` (1400×800 still) and
> `assets/demo.gif` are both committed and serve over raw.githubusercontent.

## Basic Info

| Field | Value |
|-------|-------|
| Name | agx |
| Website | https://docs.rs/agx-core |
| Repository | https://github.com/brevity1swos/agx |
| Tagline | Step through your AI agent's session in the terminal |

## Description

**What it is** (≤300)

> agx is a terminal step-through debugger for AI agent sessions. It reads the JSONL/JSON files your agent CLI already writes — Claude Code, Codex, Gemini, and more — and turns them into a navigable, color-coded timeline of user turns, assistant turns, tool calls, and tool results. No SDK, no hosted dashboard, no telemetry.

**Core features** (≤300)

> Selecting a tool result shows the original call input and the response in a single detail view. Vim-bound navigation, per-step token counts and USD cost estimates, and auto-detection of 8 trace formats (Claude Code, Codex, Gemini, OpenAI-generic, LangChain, Vercel AI SDK, OpenTelemetry GenAI JSON, and binary OTLP).

**Other features** (≤300)

> A corpus mode aggregates stats across many sessions; a heuristic PII/credential scanner flags secrets before you share a trace; exports to Markdown/HTML/JSON. A companion `agx-mcp` MCP server lets an agent introspect its own session mid-run (summary, recent errors, tool-call distribution).

**Who it's for** (≤250)

> agx is for developers building or debugging AI coding agents who live in the terminal and want to scrub a session without a browser or a hosted dashboard. Install with `cargo install agx-tui` (binary `agx`); works on Linux, macOS, and Windows.

## Technical Details — Image Preview

| Field | URL |
|-------|-----|
| PNG | https://raw.githubusercontent.com/brevity1swos/agx/main/assets/preview.png |
| GIF | https://raw.githubusercontent.com/brevity1swos/agx/main/assets/demo.gif |

## Categories (select all that apply)

- [x] **Development** — primary fit (debugger / dev tool)
- [x] **Data & Text** — multi-format trace parsing
- [ ] DevOps & Infrastructure — optional (the corpus/CI-adjacent angle)
- [ ] Operating Systems
- [ ] Databases
- [ ] Networking

The sharpest angle ("AI agent observability") has no dedicated category, so
**Development + Data & Text** is the best available mapping.
