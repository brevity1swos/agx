# agx

Step-through debugger for AI agent execution traces. Visualizes tool calls, branching decisions, retries, and backtracking in a terminal TUI.

Inspired by [rgx](https://github.com/brevity1swos/rgx) — same dual-cursor / heatmap / time-travel approach, applied to agent execution instead of regex matching.

## Status

**Prototype.** v0 scope:
- Parse a single Claude Code session JSONL file from disk
- Render the conversation and tool-call timeline in a basic ratatui TUI
- Step forward and backward through execution

Not yet implemented: branching visualization, backtrack markers, heatmap mode, multi-format support, live attach mode, time-travel scrubbing.

## Why

Today, when an AI agent does something unexpected, the debugging options are hosted dashboards (Langfuse, LangSmith, Helicone) or `cat session.jsonl | jq`. There is no terminal-native step-through debugger that lets you scrub through agent execution the way `gdb` lets you scrub through program execution.

agx is the rgx-style answer: deeply engineered, narrow scope, terminal-native, multi-format eventually but starting with Claude Code's session JSONL.

## Build

```bash
cargo build --release
./target/release/agx <path-to-session.jsonl>
```

## License

Dual-licensed under MIT OR Apache-2.0.
