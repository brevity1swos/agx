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

## Build & try

```bash
cargo build --release
./target/release/agx assets/sample_session.jsonl
```

`assets/sample_session.jsonl` is a small synthetic session that exercises every entry type agx renders — user text, assistant text, tool_use, and tool_result. Replace with a path to your own Claude Code session JSONL (typically under `~/.claude/projects/`) to step through real traces.

### Keys

| Key | Action |
|---|---|
| `↓` / `j` | next step |
| `↑` / `k` | prev step |
| `PgDn` / `d` | jump 10 steps forward |
| `PgUp` / `u` | jump 10 steps back |
| `Home` / `g` | first step |
| `End` / `G` | last step |
| `?` / `F1` | toggle help overlay |
| `q` / `Esc` | quit |

## License

Dual-licensed under MIT OR Apache-2.0.
