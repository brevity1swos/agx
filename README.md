# agx

*Just another gdb, but for your agent.*

Step through and debug AI agent traces without leaving your terminal. Written in Rust.

A terminal-native TUI that turns AI agent session files into a navigable timeline of user turns, assistant turns, tool calls, and tool results — with the original call input and the response visible on a single screen.

Inspired by [rgx](https://github.com/brevity1swos/rgx) — same dual-cursor / heatmap / time-travel approach that rgx applies to regex matching, applied here to agent execution.

## What it shows

Each session becomes a timeline of color-coded steps:

| Kind | Color | What it represents |
|---|---|---|
| `[user]` | cyan | User message or input |
| `[asst]` | green | Assistant text response |
| `[tool]` | yellow | Assistant calling a tool (Read, Bash, Write, Edit, Agent, ...) |
| `[result]` | magenta | Tool output returned to the assistant |

Selecting a `[result]` step reveals **both the original tool call input and the response in one detail view** — you see what the agent asked for and what came back without scrolling back through the conversation. That's the differentiator.

## Format support

agx v0.1.0 supports **Claude Code session JSONL only**. Other agent CLI formats use fundamentally different schemas and are not yet parseable. Adding them is a planned v0.2.0+ expansion — each format needs its own parser that maps format-specific entries into the shared timeline model.

| Agent CLI | Session location | v0.1.0 support |
|---|---|---|
| Claude Code | `~/.claude/projects/<encoded-path>/<uuid>.jsonl` | ✅ Full |
| Codex CLI (OpenAI) | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | ❌ Planned. Different schema — wraps entries in `{timestamp, type, payload}` with its own type taxonomy (`session_meta`, `response_item`, `event_msg`, `turn_context`) and uses `role: developer`. Parses without crash but produces zero timeline steps. |
| Gemini CLI (Google) | `~/.gemini/tmp/<project>/chats/session-*.json` | ❌ Planned. Single JSON object wrapper — not JSONL. Needs a separate parser path. Current parser hard-errors on load. |

If you want to track progress toward multi-format support or contribute a parser for your format, open an issue.

## Try it

```bash
git clone https://github.com/brevity1swos/agx.git
cd agx
cargo build --release
./target/release/agx assets/sample_session.jsonl
```

`assets/sample_session.jsonl` is a synthetic 9-entry session that exercises every entry type agx renders. Requires Rust 1.74+ (edition 2024).

## Use on your own sessions

Claude Code stores sessions at `~/.claude/projects/<encoded-project-path>/<session-uuid>.jsonl`. Find recent ones:

```bash
ls -lt ~/.claude/projects/*/*.jsonl 2>/dev/null | head -10
```

Then launch:

```bash
./target/release/agx ~/.claude/projects/<project>/<session>.jsonl
```

For non-interactive use (scripts, CI, piping), use `--summary` mode:

```bash
$ ./target/release/agx --summary assets/sample_session.jsonl
Loaded 9 entries from assets/sample_session.jsonl
  user: 4  assistant: 4  other: 1  tool_uses: 3  tool_results: 3
Built 11 timeline steps. First 20:
    1  [user]   Write a Python function that returns the first n Fibonacci n…
    2  [asst]   I'll create a fib.py with an iterative implementation — sing…
    3  [tool]   Write (toolu_synth…)
    4  [result] Write → File created successfully at fib.py
    5  [asst]   Let me verify it works for n=10.
    6  [tool]   Bash (toolu_synth…)
    7  [result] Bash → [0, 1, 1, 2, 3, 5, 8, 13, 21, 34]
    ...
```

## Keys

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

## Status

**v0.1.0 — prototype.** Everything in "Working" below works end-to-end on real Claude Code session files, including sessions with thousands of entries.

### Working

- Claude Code session JSONL parser (serde-based, gracefully handles unknown entry types and schema drift)
- Timeline builder with bidirectional `tool_use ↔ tool_result` pairing via a two-pass name/input lookup
- Two-pane ratatui TUI with color-coded steps and contextual detail pane
- Help overlay with keybinding reference and color legend
- Vim-style and arrow-key navigation
- Panic-safe terminal cleanup (Drop-guarded raw mode, so a crash does not leave your shell broken)
- Non-interactive `--summary` mode for scripts and CI
- 17 unit tests, clippy-clean under strict and pedantic lint groups, `cargo audit` clean

### Not yet implemented

- Branching / backtrack visualization (like rgx's PCRE2 debugger)
- Heatmap mode showing hot tool-call regions
- Time-travel scrubbing with a progress bar
- Multi-format support (Codex CLI, Gemini CLI, Anthropic Agent SDK, Vercel AI SDK, LangChain, OpenAI Assistants) — see Format support table above
- Live attach mode (watch an in-progress session)
- Filter / search / jump-to-tool
- Cost and latency annotations
- Clipboard copy of step content
- Structural diff between two sessions

## Why this exists

When an AI agent does something unexpected, today's debugging options are hosted dashboards (Langfuse, LangSmith, Helicone) or `cat session.jsonl | jq`. There is no terminal-native step-through debugger that lets you scrub through agent execution the way `gdb` lets you scrub through program execution, or the way rgx lets you step through regex matches.

agx is the rgx-style answer: deeply engineered, narrow scope, terminal-native. Multi-format eventually, but starting with the format that already has the largest user base — Claude Code session JSONL.

## Architecture

```
src/
├── main.rs       # CLI entry point (clap) + glue
├── session.rs    # JSONL parser (serde Deserialize, graceful unknown handling)
├── timeline.rs   # Entry → step expansion + tool_use ↔ tool_result pairing
└── tui.rs        # ratatui TUI + event loop + panic-safe terminal guard
```

~900 LOC. 6 direct dependencies: `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`, `clap`.

## Credits

- [rgx](https://github.com/brevity1swos/rgx) — same-family terminal regex debugger; agx inherits its design philosophy of narrow scope + deep engineering + terminal-native quality
- [ratatui](https://github.com/ratatui/ratatui) — Rust TUI framework
- [Claude Code](https://github.com/anthropics/claude-code) — the session JSONL format agx consumes

## License

Dual-licensed under MIT OR Apache-2.0.
