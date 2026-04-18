# agx

*[rgx](https://github.com/brevity1swos/rgx) is to regex101 what agx is to your browser-based agent trace dashboard — the terminal-native sibling. Zero instrumentation, works on Claude Code / Codex / Gemini out of the box.*

![demo](assets/demo.gif)

A terminal TUI that turns AI agent session files into a navigable timeline of user turns, assistant turns, tool calls, and tool results — with the original call input and the response visible on a single screen. No SDK changes, no hosted dashboard, no telemetry — agx reads the JSONL/JSON files your agent CLI already writes.

You still have Langfuse / LangSmith / Helicone / your team's internal dashboard for the team-sharing, retention, and alerting side of the story. agx is what you reach for when you're already in the terminal and just want to scrub through a session with vim bindings.

Inspired by [rgx](https://github.com/brevity1swos/rgx) — same dual-cursor / heatmap / time-travel approach that rgx applies to regex matching, applied here to agent execution.

## Install

### From source (recommended)

```bash
git clone https://github.com/brevity1swos/agx.git
cd agx
cargo install --path .
```

Requires Rust 1.85+ (edition 2024).

Binary OTLP (`.pb` / `.otlp`) support is opt-in because `prost` adds meaningful binary size. If you consume protobuf trace files from `opentelemetry-collector`, add `--features otel-proto`:

```bash
cargo install --path . --features otel-proto
```

### Shell completions

```bash
agx --completions bash >> ~/.bashrc    # Bash
agx --completions zsh >> ~/.zshrc      # Zsh
agx --completions fish > ~/.config/fish/completions/agx.fish  # Fish
```

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

agx auto-detects the session format by inspecting the first line (JSONL) or the wrapper shape (single JSON object). Five formats ship out of the box — the three major agent CLIs, generic OpenAI-compatible conversations, and OpenTelemetry GenAI traces:

| Format | Session location | Support |
|---|---|---|
| Claude Code | `~/.claude/projects/<encoded-path>/<uuid>.jsonl` | ✅ Full |
| Codex CLI (OpenAI) | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | ✅ Full |
| Gemini CLI (Google) | `~/.gemini/tmp/<project>/chats/session-*.json` | ✅ Full |
| Generic (OpenAI-compatible) | any `{messages: [{role, content, tool_calls}]}` JSON | ✅ Full |
| LangChain / LangSmith | single-JSON run-tree export from `LangSmith → Export run` or LangChain tracer | ✅ Full |
| Vercel AI SDK | `generateText` / `streamText` result object (`{steps[], toolCalls, toolResults, usage, finishReason, ...}`) | ✅ Full |
| OpenTelemetry GenAI (JSON) | any OTLP-JSON traces export with `resourceSpans` + `gen_ai.*` attributes | ✅ Full |
| OpenTelemetry GenAI (binary protobuf `.pb` / `.otlp`) | OTLP exports from `opentelemetry-collector`, OTLP/HTTP endpoints | ✅ Full (feature-gated — rebuild with `--features otel-proto`) |

Each format has its own parser module (`src/session.rs`, `src/codex.rs`, `src/gemini.rs`, `src/generic.rs`, `src/otel_json.rs`) that converts format-specific entries into the shared `timeline::Step` model. Tool calls are paired with their results regardless of how the underlying format represents the relationship — Claude Code uses `tool_use_id`, Codex uses `call_id`, Gemini packs the call and result into a single atomic `toolCall` object that agx splits, OpenAI-compatible conversations pair by position, and OTel GenAI uses `gen_ai.tool.call.id` on execute_tool spans.

Because OTel GenAI is the converging instrumentation standard across LangChain, LlamaIndex, Vercel AI SDK, Pydantic AI, and any framework that wires in OpenLLMetry or Traceloop, that row covers most framework-level agentic workloads without per-framework parsers.

To add a new format, see CLAUDE.md's "Support a new agent trace format" common task.

## Try it

```bash
# Try the built-in sample fixtures (all formats auto-detected)
agx assets/sample_session.jsonl             # Claude Code
agx assets/sample_codex_session.jsonl       # Codex CLI
agx assets/sample_gemini_session.json       # Gemini CLI
agx assets/sample_generic_session.json      # OpenAI-compatible
agx assets/sample_otel_json_traces.json     # OpenTelemetry GenAI

# Or browse your recent sessions (no args)
agx

# Watch a live session as it's being written
agx --live ~/.claude/projects/<project>/<session>.jsonl

# Compare two sessions
agx session_a.jsonl --diff session_b.jsonl

# Non-interactive summary for scripts — includes token / cost totals when usage is present
agx --summary <session>
agx --summary --no-cost <session>           # Suppress cost estimate; keep token counts

# Export a transcript to stdout — formats: md | html | json
agx --export md   <session> > session.md
agx --export html <session> > session.html
agx --export json <session> > session.json

# Diagnose format drift — prints every entry type or field the parser
# didn't recognize, to stderr. Useful when a new CLI version lands.
agx --debug-unknowns <session>

# Aggregate stats across a directory of sessions (recursive walk,
# parallel parse). Filters AND-combine.
agx corpus <dir>
agx corpus <dir> --filter model=claude-opus-4-6 --filter tool=Bash
agx corpus <dir> --filter errored --json       # pretty-printed stats JSON
agx corpus <dir> --tui                         # interactive browser: list + detail, Enter drills in
agx corpus <dir> --jsonl | jq '.cost_usd'      # one JSON-per-session on stdout; parse errors on stderr
agx corpus <dir> --fail-on-errored             # exit nonzero if any parse error / tool error — CI-friendly
```

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

For non-interactive use (scripts, CI, piping), use `--summary` mode. When the session carries token-usage data, agx also prints totals and a USD cost estimate (rates are from a hand-curated pricing table — see `src/pricing.rs` for the current entries and their `last_verified` dates):

```bash
$ ./target/release/agx --summary assets/sample_session.jsonl
Loaded Claude Code session from assets/sample_session.jsonl
  11 timeline steps: 1 user, 4 assistant, 3 tool_uses, 3 tool_results
  740 input tokens, 345 output, 6810 cache_read, 1500 cache_create
  models: claude-opus-4-6
  estimated cost: $0.0753 USD
First 20:
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

Everything below works end-to-end on real sessions across all five supported formats, including sessions with thousands of entries. See `ROADMAP.md` for what's planned next (OTLP protobuf, framework-specific parsers, corpus analytics, RL trajectory export, library mode).

### Working

**Formats**
- **Multi-format support**: Claude Code, Codex CLI, Gemini CLI, generic OpenAI-compatible conversations, and OpenTelemetry GenAI JSON — all auto-detected (see Format support table)
- **Multi-session browser**: launch with no args to scan `~/.claude`, `~/.codex`, `~/.gemini` for recent sessions
- **Bidirectional tool pairing**: each tool_result shows both the originating call input and the response
- **`--debug-unknowns`**: per-format drift scanner reports unknown entry types / payload types / operation names to stderr with line-number samples — useful for diagnosing a new CLI release before it breaks parsing

**Observability & cost** (Phase 1, shipped 2026-04-15)
- **Per-step token usage**: `tokens_in`, `tokens_out`, `cache_read`, `cache_create` parsed from Claude Code, Codex, Gemini, generic OpenAI, and OTel GenAI sessions. Attached to the first step of each assistant message so corpus-level sums don't double-count.
- **USD cost estimates**: hand-curated pricing table in `src/pricing.rs` covers opus-4-6, sonnet-4-6, haiku-4-5, gpt-5, gpt-5-mini, gemini-2-5-pro, gemini-2-5-flash. Returns `None` on unknown models rather than fabricating cost.
- **`--summary`**: non-interactive total-tokens / total-cost / unique-models lines plus step counts and first 20 step labels
- **TUI cost rendering**: running session cost in the status bar, per-step tokens + cost in the detail pane, session totals in the stats overlay (`s`)
- **`--no-cost`**: suppresses cost estimates everywhere (summary, TUI, exports) while keeping token counts — for unpriced custom models or when cost is noise
- **`--export md | html | json`**: Markdown transcript, self-contained HTML (inline CSS, no JS, no external assets, HTML-escaped detail), and stable-schema JSON (the reserved public programmatic interface)

**Navigation & UI**
- **Three-pane layout**: timeline / conversation view / detail pane (Tab toggles 2-pane fallback)
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
- **Heatmap mode** (`h`): color-codes timeline by tool-call density — warm colors for hot regions, cool for sparse
- **Tool usage statistics overlay** (`s`): per-tool use/result/error counts with error rate; session totals (tokens / cost / unique models) at the top
- **Session comparison** (`--diff`): cross-format text summary comparing tool usage and errors
- **Clipboard copy** (`y`): copies current step detail to system clipboard
- **Live attach** (`--live`): watches session file for changes and auto-refreshes the TUI every 500ms
- **Help overlay** (`?` / `F1`) with keybinding reference and color legend
- **Panic-safe terminal cleanup** (Drop-guarded raw mode)

**Quality bar**
- **181 tests** (171 unit + 1 corpus + 9 integration), clippy-clean under strict and pedantic lint groups, `cargo audit` clean

## Why this exists

When an AI agent does something unexpected, today's debugging options are hosted dashboards (Langfuse, LangSmith, Helicone) or `cat session.jsonl | jq`. There is no terminal-native step-through debugger that lets you scrub through agent execution the way `gdb` lets you scrub through program execution, or the way rgx lets you step through regex matches.

agx is the rgx-style answer: deeply engineered, narrow scope, terminal-native. Multi-format eventually, but starting with the format that already has the largest user base — Claude Code session JSONL.

## Architecture

```
src/
├── main.rs             # CLI entry (clap) + format dispatch + --summary / --export / --diff branches
├── format.rs           # Format detection — returns ClaudeCode | Codex | Gemini | Generic | OtelJson
├── browser.rs          # Multi-session discovery + picker
├── session.rs          # Claude Code JSONL parser (serde Deserialize + ClaudeUsage)
├── codex.rs            # Codex CLI JSONL parser (response_item + function_call pairing)
├── gemini.rs           # Gemini CLI single-JSON parser (toolCall splitting + usageMetadata)
├── generic.rs          # OpenAI-compatible conversation parser
├── otel_json.rs        # OpenTelemetry GenAI JSON parser (OTLP-JSON + gen_ai.* semconv)
├── timeline.rs         # Shared Step / StepKind / Usage / SessionTotals + helpers + compute_* functions
├── pricing.rs          # Per-model USD rate table + cost computation
├── export.rs           # Markdown / HTML / JSON transcript writers (no I/O)
├── debug_unknowns.rs   # Per-format drift scanners for --debug-unknowns
└── tui.rs              # ratatui TUI + event loop + overlays + panic-safe terminal guard
```

Each parser produces `Vec<Step>` directly; `timeline::build()` is the Claude Code adapter that converts the format's native `Entry` enum into Steps. All parsers share the same step helpers (`user_text_step`, `assistant_text_step`, `tool_use_step`, `tool_result_step`) and the same `attach_usage_to_first` anchor for model/token data — so the TUI renders every format identically and corpus sums never double-count.

8 direct dependencies: `ratatui`, `crossterm`, `serde`, `serde_json`, `anyhow`, `clap`, `clap_complete`, `arboard`.

## Credits

- [rgx](https://github.com/brevity1swos/rgx) — same-family terminal regex debugger; agx inherits its design philosophy of narrow scope + deep engineering + terminal-native quality
- [ratatui](https://github.com/ratatui/ratatui) — Rust TUI framework
- [Claude Code](https://github.com/anthropics/claude-code) — the session JSONL format agx consumes

## License

Dual-licensed under MIT OR Apache-2.0.
