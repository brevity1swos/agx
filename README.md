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

agx auto-detects the session format by inspecting the first line (JSONL) or the wrapper shape (single JSON object). All three major agent CLIs are supported out of the box:

| Agent CLI | Session location | Support |
|---|---|---|
| Claude Code | `~/.claude/projects/<encoded-path>/<uuid>.jsonl` | ✅ Full |
| Codex CLI (OpenAI) | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | ✅ Full |
| Gemini CLI (Google) | `~/.gemini/tmp/<project>/chats/session-*.json` | ✅ Full |

Each format has its own parser module (`src/session.rs`, `src/codex.rs`, `src/gemini.rs`) that converts format-specific entries into the shared `timeline::Step` model. Tool calls are paired with their results regardless of how the underlying format represents the relationship — Claude Code uses `tool_use_id`, Codex uses `call_id`, and Gemini packs the call and result into a single atomic `toolCall` object that agx splits for timeline navigation.

To add a new format, see CLAUDE.md's "Support a new agent trace format" common task.

## Try it

```bash
git clone https://github.com/brevity1swos/agx.git
cd agx
cargo build --release

# Pick one — all three work, format auto-detected
./target/release/agx assets/sample_session.jsonl          # Claude Code format
./target/release/agx assets/sample_codex_session.jsonl    # Codex CLI format
./target/release/agx assets/sample_gemini_session.json    # Gemini CLI format
```

Each fixture is a synthetic Fibonacci-writing conversation in its native schema — zero personal data, exercises every entry type the parser handles. Requires Rust 1.74+ (edition 2024).

## Use on your own sessions

Each CLI stores its sessions in a predictable location. Find recent ones with:

```bash
# Claude Code
ls -lt ~/.claude/projects/*/*.jsonl 2>/dev/null | head -5

# Codex CLI
ls -lt ~/.codex/sessions/*/*/*/rollout-*.jsonl 2>/dev/null | head -5

# Gemini CLI
ls -lt ~/.gemini/tmp/*/chats/session-*.json 2>/dev/null | head -5
```

Then launch agx on any of them — no flag needed, format is auto-detected:

```bash
./target/release/agx ~/.claude/projects/<project>/<session>.jsonl
./target/release/agx ~/.codex/sessions/<yyyy>/<mm>/<dd>/rollout-<ts>.jsonl
./target/release/agx ~/.gemini/tmp/<project>/chats/session-<ts>.json
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

Everything below works end-to-end on real sessions from all three supported CLIs, including sessions with thousands of entries.

### Working

- **Multi-format support**: Claude Code, Codex CLI, and Gemini CLI sessions with auto-detection (see Format support table)
- **Multi-session browser**: launch with no args to scan `~/.claude`, `~/.codex`, `~/.gemini` for recent sessions
- **Three-pane layout**: timeline / conversation view / detail pane (Tab toggles 2-pane fallback)
- **Bidirectional tool pairing**: each tool_result shows both the originating call input and the response
- **Alternating step colors** + **batch/fork markers** (`║` prefix for parallel tool dispatches)
- **Error detection**: heuristic-based tool error highlighting (red + bold) across all formats
- **Latency annotations**: per-step duration computed from timestamps, shown in detail pane
- **Filter** (`f`): case-insensitive substring match, hides non-matching rows
- **Search** (`/`): highlights matches with distinct bg, `n`/`N` to navigate hits
- **Bookmarks** (`m<char>` / `'<char>`): survive filter cycles, report hidden-by-filter
- **Jump to step** (`:N`): command-mode numeric jump
- **Time-travel scrubbing bar**: bottom progress gauge with position indicator
- **Vim count prefixes** (`3j`, `5k`, `42G`, etc.) on all navigation keys
- **Mouse support**: click-to-select on timeline, scroll wheel navigation
- **Tool usage statistics overlay** (`s`): per-tool use/result/error counts with error rate
- **Session comparison** (`--diff`): cross-format text summary comparing tool usage and errors
- **Non-interactive `--summary` mode** for scripts and CI
- **Help overlay** (`?` / `F1`) with keybinding reference and color legend
- **Panic-safe terminal cleanup** (Drop-guarded raw mode)
- 112 unit tests, clippy-clean under strict and pedantic lint groups, `cargo audit` clean

- **Heatmap mode** (`h`): color-codes timeline by tool-call density — warm colors for hot regions, cool for sparse
- **Clipboard copy** (`y`): copies current step detail to system clipboard
- **Live attach** (`--live`): watches session file for changes and auto-refreshes the TUI every 500ms
- **Generic conversation format**: OpenAI-compatible `{messages: [{role, content, tool_calls}]}` — covers Anthropic SDK, Vercel AI SDK, LangChain, OpenAI Assistants exports

## Why this exists

When an AI agent does something unexpected, today's debugging options are hosted dashboards (Langfuse, LangSmith, Helicone) or `cat session.jsonl | jq`. There is no terminal-native step-through debugger that lets you scrub through agent execution the way `gdb` lets you scrub through program execution, or the way rgx lets you step through regex matches.

agx is the rgx-style answer: deeply engineered, narrow scope, terminal-native. Multi-format eventually, but starting with the format that already has the largest user base — Claude Code session JSONL.

## Architecture

```
src/
├── main.rs       # CLI entry point (clap) + format dispatch
├── format.rs     # Format detection (Claude Code / Codex / Gemini)
├── session.rs    # Claude Code JSONL parser (serde Deserialize)
├── codex.rs      # Codex CLI JSONL parser (response_item + function_call pairing)
├── gemini.rs     # Gemini CLI single-JSON parser (toolCall splitting)
├── timeline.rs   # Shared Step / StepKind + step helpers used by all parsers
└── tui.rs        # ratatui TUI + event loop + panic-safe terminal guard
```

Each parser produces `Vec<Step>` directly; `timeline::build()` is the Claude Code adapter that converts the format's native `Entry` enum into Steps. All formats share the same step-helper functions (`user_text_step`, `assistant_text_step`, `tool_use_step`, `tool_result_step`) so the TUI renders every format identically.

6 direct dependencies: `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`, `clap`.

## Credits

- [rgx](https://github.com/brevity1swos/rgx) — same-family terminal regex debugger; agx inherits its design philosophy of narrow scope + deep engineering + terminal-native quality
- [ratatui](https://github.com/ratatui/ratatui) — Rust TUI framework
- [Claude Code](https://github.com/anthropics/claude-code) — the session JSONL format agx consumes

## License

Dual-licensed under MIT OR Apache-2.0.
