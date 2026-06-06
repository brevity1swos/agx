# agx

[![crates.io](https://img.shields.io/crates/v/agx-tui.svg?label=agx-tui)](https://crates.io/crates/agx-tui)
[![crates.io](https://img.shields.io/crates/v/agx-core.svg?label=agx-core)](https://crates.io/crates/agx-core)
[![docs.rs](https://img.shields.io/docsrs/agx-core)](https://docs.rs/agx-core)
[![CI](https://github.com/brevity1swos/agx/actions/workflows/ci.yml/badge.svg)](https://github.com/brevity1swos/agx/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/agx-tui.svg)](https://github.com/brevity1swos/agx#license)

*[rgx](https://github.com/brevity1swos/rgx) is to regex101 what agx is to your browser-based agent trace dashboard — the terminal-native sibling. Zero instrumentation, works on Claude Code / Codex / Gemini out of the box.*

![demo](assets/demo.gif)

A terminal TUI that turns AI agent session files into a navigable timeline of user turns, assistant turns, tool calls, and tool results — with the original call input and the response visible on a single screen. No SDK changes, no hosted dashboard, no telemetry — agx reads the JSONL/JSON files your agent CLI already writes.

You still have Langfuse / LangSmith / Helicone / your team's internal dashboard for the team-sharing, retention, and alerting side of the story. agx is what you reach for when you're already in the terminal and just want to scrub through a session with vim bindings.

Inspired by [rgx](https://github.com/brevity1swos/rgx) — same dual-cursor / heatmap / time-travel approach that rgx applies to regex matching, applied here to agent execution.

## Install

### From crates.io

```bash
cargo install agx-tui
```

The published crate is `agx-tui` (the unqualified `agx` name on crates.io was taken by an unrelated project before this one started); the installed binary is `agx`.

Requires Rust 1.85+ (edition 2024).

Binary OTLP (`.pb` / `.otlp`) support is opt-in because `prost` adds meaningful binary size. If you consume protobuf trace files from `opentelemetry-collector`:

```bash
cargo install agx-tui --features otel-proto
```

Other opt-in features: `embedding-search` (semantic `//query`, pulls a ~90MB MiniLM model on first use), `notifications` (desktop notifications for `--live` mode).

### As a library — `agx-core` on [crates.io](https://crates.io/crates/agx-core)

If you're building a custom eval harness, RL trajectory pipeline, or any tool that needs to parse agent traces without the TUI, depend on [`agx-core`](https://crates.io/crates/agx-core) directly:

```bash
cargo add agx-core
```

`agx-core` is the pure, TUI-free heart of the binary — every parser (Claude Code, Codex, Gemini, OpenAI-generic, LangChain, Vercel AI SDK, OTel GenAI JSON, OTel binary protobuf), the timeline / step model, corpus aggregation, cost / pricing, PII scanner, and the JSON export shape that `agx --export json` writes. Zero dependency on ratatui / crossterm / arboard. Documentation: [docs.rs/agx-core](https://docs.rs/agx-core). The Python bindings (`agx-py` on PyPI) and WASM bindings (`agx-wasm` on npm) wrap this same crate.

### From source

```bash
git clone https://github.com/brevity1swos/agx.git
cd agx
cargo install --path .
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

agx auto-detects the session format by inspecting the first line (JSONL) or the wrapper shape (single JSON object). Eight formats ship out of the box — the three major agent CLIs, framework-level traces, and OpenTelemetry GenAI:

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

Each format has its own parser module in `crates/agx-core/src/` that converts format-specific entries into the shared `timeline::Step` model. Tool calls are paired with their results regardless of how the underlying format represents the relationship — Claude Code uses `tool_use_id`, Codex uses `call_id`, Gemini packs the call and result into a single atomic `toolCall` object that agx splits, OpenAI-compatible conversations pair by position, and OTel GenAI uses `gen_ai.tool.call.id` on execute_tool spans.

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

# Live mode with desktop notifications (opt-in: rebuild with `--features notifications`)
agx --live --notify-on-error <session>          # OS notify on every new error tool_result
agx --live --notify-on-idle 10m <session>       # OS notify when session hasn't grown for 10 minutes

# Compare two sessions — text summary
agx session_a.jsonl --diff session_b.jsonl
# …or the interactive two-pane TUI (synchronized scrolling, color-coded alignment)
agx session_a.jsonl --diff session_b.jsonl --diff-tui

# Non-interactive summary for scripts — includes token / cost totals when usage is present
agx --summary <session>
agx --summary --no-cost <session>           # Suppress cost estimate; keep token counts

# Slice by step index or by time offset from the session's first step
agx --range 100..500 <session>              # Exclusive end; open-ended forms like `..500` / `100..` also work
agx --after-step 100 --before-step 500 <session>
agx --after 30m --before 1h <session>       # Duration grammar: 30s / 5m / 2h / 1d, or compounds like 1h30m
# Inside the TUI: `:@1h30m` jumps to the first step ≥ that offset from session start

# Launch the TUI with the cursor pre-positioned at a specific step
# (0-indexed; clamps to last if out of range). Public contract used by
# sift's Timeline-jump integration.
agx --jump-to 42 <session>

# Annotate a step: press `a` in the TUI. Notes persist under ~/.agx/notes/ and
# render as a magenta `*` prefix in the list + `[note: ...]` in the detail pane.
# Press `A` for a list of every note with Enter-to-jump navigation.

# Export a transcript to stdout — formats: md | html | json | trajectory-openai.
# Notes (if any) are surfaced in md/html/json: md blockquote, html `<div class="note">`,
# json `annotations` array. trajectory-openai emits one line of OpenAI fine-tuning
# JSONL (`{messages: [{role, content, tool_calls?, tool_call_id?}]}`) — ready to
# `cat *.openai.jsonl | openai file upload …`.
agx --export md                <session> > session.md
agx --export html              <session> > session.html
agx --export json              <session> > session.json
agx --export trajectory-openai <session> > session.openai.jsonl

# Strip secrets before publishing a dataset — literal substring mask, repeatable,
# applies to every --export format.
agx --export trajectory-openai --redact 'sk-abc123' --redact 'bearer xyz' <session>

# Heuristic scan for credentials / PII before publishing — catches AWS/Stripe/GitHub/
# OpenAI/Anthropic keys, JWTs, SSH private-key PEM headers, emails, IPv4 addresses.
# Read-only: reports matches + step indices, doesn't mutate. Pair with --redact.
# Full workflow + adapter recipes for inspect-ai, lm-eval-harness, and custom
# pipelines: docs/eval-integration.md.
agx --scan-pii <session>

# Diagnose format drift — prints every entry type or field the parser
# didn't recognize, to stderr. Useful when a new CLI version lands.
agx --debug-unknowns <session>

# Aggregate stats across a directory of sessions (recursive walk,
# parallel parse). Filters AND-combine.
agx corpus <dir>
agx corpus <dir> --filter model=claude-opus-4-6 --filter tool=Bash
agx corpus <dir> --filter errored --json       # pretty-printed stats JSON
agx corpus <dir> --filter annotated             # keep only sessions with ≥1 annotation
agx corpus <dir> --tui                         # interactive browser: list + detail, Enter drills in
agx corpus <dir> --jsonl | jq '.cost_usd'      # one JSON-per-session on stdout; parse errors on stderr
agx corpus <dir> --fail-on-errored             # exit nonzero if any parse error / tool error — CI-friendly
agx corpus <dir> --trajectory-stats            # distributional stats (steps/toks/etc percentiles + branch/annot/error rates)
agx corpus <dir> --sample 20                   # keep only 20 most-recent sessions (composes with --filter)
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

For non-interactive use (scripts, CI, piping), use `--summary` mode. When the session carries token-usage data, agx also prints totals and a USD cost estimate (rates are from a hand-curated pricing table — see `crates/agx-core/src/pricing.rs` for the current entries and their `last_verified` dates):

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

Press `?` or `F1` inside the TUI for the full cheat sheet.

### Navigation

| Key | Action |
|---|---|
| `↓` / `j` | next step |
| `↑` / `k` | prev step |
| `PgDn` / `d` | jump 10 steps forward |
| `PgUp` / `u` | jump 10 steps back |
| `Home` / `g` | first step |
| `End` / `G` | last step |
| `:N` | jump to visible row N |
| `:@<duration>` | jump to first step ≥ offset from session start (1h30m, 5m, 90s) |
| `<N><motion>` | vim count prefix (3j, 5k, 2d, 42G, ...) |

### Filter & Search

| Key | Action |
|---|---|
| `f` | open filter prompt (hides non-matching rows) |
| `/` | open search prompt (highlights matches) |
| `//query` | semantic search (opt-in: `--features embedding-search`) |
| `n` / `N` | next / prev search match |

### Bookmarks & Annotations

| Key | Action |
|---|---|
| `m<char>` | set bookmark at current step |
| `'<char>` | jump to bookmark |
| `a` | add / edit / clear annotation on current step |
| `A` | list all annotations (Enter jumps to step) |

### Other

| Key | Action |
|---|---|
| `b` | list all fork roots (Claude Code edit/resume branches) |
| `y` | copy current step detail to clipboard |
| `h` | toggle heatmap mode (tool-call density) |
| `s` | toggle tool usage stats overlay |
| `Tab` | toggle 3-pane / 2-pane layout |
| `mouse click` | select row in timeline |
| `mouse scroll` | prev / next step |
| `?` / `F1` | toggle help |
| `q` / `Esc` | quit |

## Status

Everything below works end-to-end on real sessions across all supported formats, including sessions with thousands of entries. See `ROADMAP.md` for the full feature timeline.

### Working

**Formats**
- **Multi-format support**: Claude Code, Codex CLI, Gemini CLI, generic OpenAI-compatible, LangChain / LangSmith, Vercel AI SDK, OTel GenAI JSON, and OTel binary protobuf — all auto-detected (see Format support table)
- **Multi-session browser**: launch with no args to scan `~/.claude`, `~/.codex`, `~/.gemini` for recent sessions
- **Bidirectional tool pairing**: each tool_result shows both the originating call input and the response
- **`--debug-unknowns`**: per-format drift scanner reports unknown entry types / payload types / operation names to stderr with line-number samples — useful for diagnosing a new CLI release before it breaks parsing

**Observability & cost** (Phase 1, shipped 2026-04-15)
- **Per-step token usage**: `tokens_in`, `tokens_out`, `cache_read`, `cache_create` parsed from Claude Code, Codex, Gemini, generic OpenAI, and OTel GenAI sessions. Attached to the first step of each assistant message so corpus-level sums don't double-count.
- **USD cost estimates**: hand-curated pricing table in `crates/agx-core/src/pricing.rs` covers opus-4-6, sonnet-4-6, haiku-4-5, gpt-5, gpt-5-mini, gemini-2-5-pro, gemini-2-5-flash. Returns `None` on unknown models rather than fabricating cost.
- **`--summary`**: non-interactive total-tokens / total-cost / unique-models lines plus step counts and first 20 step labels
- **TUI cost rendering**: running session cost in the status bar, per-step tokens + cost in the detail pane, session totals in the stats overlay (`s`)
- **`--no-cost`**: suppresses cost estimates everywhere (summary, TUI, exports) while keeping token counts — for unpriced custom models or when cost is noise
- **`--export md | html | json | trajectory-openai`**: Markdown transcript, self-contained HTML (inline CSS, no JS, no external assets), stable-schema JSON (public programmatic interface), and OpenAI fine-tuning JSONL (one `{messages}` object per session)

**Navigation & UI**
- **Three-pane layout**: timeline / conversation view / detail pane (Tab toggles 2-pane fallback)
- **Alternating step colors** + **batch/fork markers** (`║` prefix for parallel tool dispatches)
- **Error detection**: heuristic-based tool error highlighting (red + bold) across all formats
- **Latency annotations**: per-step duration computed from timestamps, shown in detail pane
- **Filter** (`f`): case-insensitive substring match, hides non-matching rows
- **Search** (`/`): highlights matches with distinct bg, `n`/`N` to navigate hits
- **Semantic search** (`//query`, opt-in): ranks steps by meaning, not substring. Build with `cargo install agx --features embedding-search` (adds fastembed + ONNX Runtime; first use downloads a ~90MB MiniLM model to `~/.cache/fastembed/`). Without the feature, `//query` prints a rebuild hint and leaves existing search state intact.
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

**Tooling**
- **`agx doctor`** — health-check report: version, features, stepwise sibling availability (sift, rgx, agx-mcp)
- **`agx-mcp`** — MCP server for agent self-introspection mid-session (list steps, read annotations, export)
- **Experimental replay** (`R`, hidden flags) — re-execute a tool call from the TUI with triple-gate safety (flag + backend flag + per-invocation confirm)

**Quality bar**
- **369 tests** across the workspace (252 agx-core + 104 integration + 11 CLI + doc-tests), clippy-clean under strict and pedantic lint groups, `cargo audit` clean

## Why this exists

When an AI agent does something unexpected, today's debugging options are hosted dashboards (Langfuse, LangSmith, Helicone) or `cat session.jsonl | jq`. There is no terminal-native step-through debugger that lets you scrub through agent execution the way `gdb` lets you scrub through program execution, or the way rgx lets you step through regex matches.

agx is the rgx-style answer: deeply engineered, narrow scope, terminal-native. Eight formats auto-detected out of the box — Claude Code, Codex, Gemini, LangChain, Vercel AI SDK, OpenAI-compatible, and OpenTelemetry GenAI (JSON + binary).

## Architecture

```
crates/
├── agx-core/src/       # Pure parsers, timeline model, corpus, pricing, export, annotations, PII, semantic search
│   ├── session.rs      # Claude Code JSONL parser
│   ├── codex.rs        # Codex CLI JSONL parser
│   ├── gemini.rs       # Gemini CLI single-JSON parser
│   ├── generic.rs      # OpenAI-compatible conversation parser
│   ├── langchain.rs    # LangChain / LangSmith run-tree parser
│   ├── vercel_ai.rs    # Vercel AI SDK result parser
│   ├── otel_json.rs    # OpenTelemetry GenAI JSON parser
│   ├── otel_proto.rs   # Binary OTLP parser (feature-gated: otel-proto)
│   ├── format.rs       # Format auto-detection
│   ├── timeline.rs     # Shared Step / StepKind / Usage / SessionTotals
│   ├── pricing.rs      # Per-model USD rate table
│   ├── corpus.rs       # Parallel directory-walk aggregation
│   ├── export.rs       # Markdown / HTML / JSON / trajectory-openai writers
│   ├── annotations.rs  # Per-step notes (sidecar JSON under ~/.agx/notes/)
│   ├── pii.rs          # Heuristic credential / PII scanner
│   ├── semantic.rs     # Embedding-based search (feature-gated: embedding-search)
│   └── ...
├── agx-mcp/            # MCP server for agent self-introspection (standalone binary)
├── agx-py/             # Python bindings via PyO3
└── agx-wasm/           # WASM bindings via wasm-bindgen
src/
├── main.rs             # CLI entry (clap) + format dispatch + --summary / --export / --diff
├── lib.rs              # Re-exports agx-core + local TUI modules
├── tui.rs              # ratatui TUI: three-pane layout, event loop, overlays, TerminalGuard
├── corpus_tui.rs       # Interactive corpus browser (--tui)
├── diff_tui.rs         # Side-by-side session diff TUI (--diff-tui)
└── replay.rs           # Experimental tool-call replay (Phase 5.4, triple-gated)
```

Each parser produces `Vec<Step>` directly; `timeline::build()` is the Claude Code adapter that converts the format's native `Entry` enum into Steps. All parsers share the same step helpers (`user_text_step`, `assistant_text_step`, `tool_use_step`, `tool_result_step`) and the same `attach_usage_to_first` anchor for model/token data — so the TUI renders every format identically and corpus sums never double-count.

## Pairs well with

- **[rgx](https://github.com/brevity1swos/rgx)** — terminal regex
  debugger. When an agx timeline step shows a tool-call argument
  that contains a regex (a `Bash` grep, a sed expression, a routing
  pattern), `R` will open rgx for step-through inspection (proposed —
  see ROADMAP §8.5).
- **[sift](https://github.com/brevity1swos/sift)** — AI write review
  gate. Sift consumes agx's `--export json` output for format-aware
  session parsing, and launches agx from its review TUI to give
  timeline context to pending-write decisions via `agx --jump-to <N>
  <session>` (shipped Phase 5.5).

All three tools are independent — each earns its keep alone. Combined,
they form **[stepwise](https://github.com/brevity1swos/stepwise)**, the
terminal-native step-through debugger stack for the AI-development
workflow. Shared UX and integration contracts (keybindings, CLI
grammar, color palette, cross-tool file / subprocess contracts) live
in [docs/suite-conventions.md](docs/suite-conventions.md), maintained
verbatim across the three repos.

### Compatibility

Suite-level cross-tool compatibility, updated on each minor release
that changes a public CLI surface (per suite-conventions §7).

| agx   | works with sift | works with rgx |
|-------|-----------------|----------------|
| 0.1.x | —               | any            |
| 0.2.x | planned ≥ 0.3   | 0.11.x+        |

agx talks to its siblings at the subprocess boundary (`agx --export
json`, `agx --summary`, `agx --jump-to`) — no shared Rust
library, no coordinated release train. Missing siblings never block
agx's own flow.

## Credits

- [rgx](https://github.com/brevity1swos/rgx) — same-family terminal regex debugger; agx inherits its design philosophy of narrow scope + deep engineering + terminal-native quality
- [ratatui](https://github.com/ratatui/ratatui) — Rust TUI framework
- [Claude Code](https://github.com/anthropics/claude-code) — the session JSONL format agx consumes

## License

Dual-licensed under MIT OR Apache-2.0.
