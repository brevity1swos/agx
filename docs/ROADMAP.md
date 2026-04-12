# agx Roadmap

Decided 2026-04-11. This roadmap is the binding scope for v1.0 pre-public-release. Anything not on this list is parked until after validation.

## Goal

Make agx powerful enough before the public release that cloning it is unambiguously harder than adopting it. The moat is depth, not breadth — match rgx's engineering density in the terminal regex space, applied to the agent trace debugging space.

## Strategic framing

- **rgx's moat came from depth + breadth**: 7,770 LOC, step-through debugger via PCRE2 FFI, 3 engines, codegen, vim mode, recipes, workspace, test suite, syntax highlighting, mouse, explanations. A weekend clone is impossible.
- **agx's current v0.1.0 state** is a parser + timeline + basic two-pane TUI with three format support (Claude Code, Codex CLI, Gemini CLI). Solid but clonable in a few days.
- **The decision** (from a three-option brainstorm — full v1 buildout / moderate cut / ship-fast minimum): **option 3 — ship fast with debugger depth + workflow muscle, defer analytics and format breadth to post-release**. Protects Road A's grant-forge queue and the broader SaaS capacity allocation while still producing a v1 that a rational competitor won't try to clone.
- **Capacity budget**: ~9 days of focused engineering work, ~2 months side-project calendar time (at ~1 day/week). After v1 ships and validation lands, Phase 3+4+5 become candidates based on real user signal.

## In scope for v1.0 (12 features, 2 phases)

### Phase 1 — Debugger depth (the rgx-family moat)

| # | Feature | Status | Notes |
|---|---|---|---|
| 1 | Alternating step colors | ✅ done | Commit `6b38d9f`. Distinct bg per tool call group. |
| 2 | Time-travel scrubbing bar | ✅ done | Commit `6b38d9f`. Bottom progress bar with Gauge widget. |
| 3 | Jump to step (`:42`) | ✅ done | Commit `6b38d9f`. Command mode via `:`, number jump. |
| 8 | Backtrack / error marker detection | ✅ done | Heuristic detection of tool errors in result content — red + bold in timeline. Retry pattern detection deferred to post-release. |
| 9 | Dual-cursor conversation panel | ✅ done | Three-pane layout (timeline 25% / conversation 40% / detail 35%), Tab toggles 2-pane fallback. Conversation pane shows text-only flow, cursor syncs to nearest preceding text step. |
| 10 | Branch / fork visualization | planned | Indent and tree display for parallel tool calls. Depends on feature 9. |

### Phase 2 — Workflow muscle (usability for long sessions)

| # | Feature | Status | Notes |
|---|---|---|---|
| 4 | Filter by tool name / step kind | ✅ done | Commit `76e2f94`. `f` opens filter prompt, case-insensitive substring match against step labels. |
| 5 | Content search (`/pattern`) | ✅ done | Commit `ef1dd11`. `/` to search label + detail, `n`/`N` to navigate matches with wrap, distinct highlight bg. |
| 6 | Bookmarks | ✅ done | Commit `95addb0`. `m<char>` sets, `'<char>` jumps. Stored by original step index, survives filter cycles, reports hidden-by-filter. |
| 7 | Mouse support | ✅ done | Click-to-select on timeline rows, scroll-wheel prev/next. |
| 11 | Vim mode (`--vim`) | planned | Normal/Insert, hjkl/w/b/e, `:` commands, `/` search |
| 12 | Multi-session browser | planned | No args → interactive picker across Claude / Codex / Gemini recent sessions |

**Progress: 9/12 features done** (~5 days of work). Remaining: 3 features, ~3.5 days.

## Build order (dependency-sorted)

Commit sequence. Each unit is independently shippable, rollbackable, and tested.

1. Alternating step colors + time-travel scrubbing bar + jump-to-step (features 1+2+3, bundled — shared command mode infrastructure)
2. Filter by tool name / step kind (feature 4)
3. Content search with match highlighting (feature 5)
4. Bookmarks (feature 6)
5. Mouse support (feature 7)
6. Detect and mark tool errors and retries (feature 8)
7. Three-pane layout with dual-cursor conversation view (feature 9 — structural)
8. Branch / fork visualization for parallel tool calls (feature 10, depends on 9)
9. Vim mode (feature 11, depends on command mode from 1, filter from 4, search from 5)
10. Multi-session browser when launched without args (feature 12)
11. Final pre-release polish: README + CLAUDE.md updates + demo GIF (pre-public-release only)

## Deliberately out of scope for v1.0

These are all legitimate features that are **parked**, not rejected. They become candidates after v1.0 ships and real user signal arrives.

### Phase 3 (format breadth) — parked
- Anthropic Agent SDK trace format
- Vercel AI SDK trace format
- LangSmith / LangChain export
- OTEL (OpenTelemetry) agent traces
- Cline / Continue.dev / Aider session formats

### Phase 4 (agent-domain analytics) — parked
- Cost + latency annotations from timestamps + token counts
- Tool usage statistics overlay (histogram, timing, failure rates)
- Session comparison / structural diff mode

### Phase 5 (polish + distribution) — parked (do just-in-time before public flip)
- Config file at `~/.config/agx/config.toml`
- Syntax highlighting in result pane (syntect)
- Export to markdown
- Export to HTML
- Clipboard copy (step / input / result)
- Shell completions (bash / zsh / fish / powershell)
- cargo-dist shell + powershell installers
- Homebrew tap entry
- Demo GIF (vhs or asciinema)
- Criterion benchmarks suite
- CHANGELOG.md
- CONTRIBUTING.md

## Validation gate (between v1.0 and public release)

v1.0 does NOT auto-ship to public. After all 12 features land, the validation step kicks in per the HANDOFF.md discipline:

1. Launch the v1 TUI against real sessions from all three CLIs
2. Show the prototype to 5-10 Claude Code / Codex / Gemini users in r/ClaudeAI, GitHub issues, awesome-claude-code, X/Twitter
3. Measure signal: are reactions "wait, what is this?" (ship public) or flat ("meh, another tool")?
4. If signal is strong, do Phase 5 polish pass and flip the repo public
5. If signal is weak, diagnose which part of the tool-hypothesis didn't transfer and decide whether to iterate or kill

## Anti-scope

Things that will tempt us during the build and should be rejected:

- **Do not add a hosted component** (web viewer, cloud sync, telemetry). agx is terminal-native. Adding a hosted tier contradicts the positioning and pulls the project into Langfuse/LangSmith territory where incumbents have massive leads.
- **Do not add AI features** that use an LLM to explain or summarize sessions. The whole point is to *be* the debugger, not call one. Explanations via LLM would mean needing API keys, runtime cost, and latency — none acceptable for a terminal tool.
- **Do not add format-specific shortcuts** that only work for one CLI. Keybindings must work identically across Claude Code, Codex, and Gemini sessions.
- **Do not let Phase 5 polish bleed into Phases 1+2.** Distribution work is explicitly final-pass only.
- **Do not promote parked features into v1.0 mid-build without a conscious decision.** Every "while I'm in here..." is Road A dying by a thousand cuts.

## Success criteria

v1.0 is ready for the validation gate when:

- All 12 features land as separate commits
- `cargo fmt --check` clean
- `cargo build --release` clean
- `cargo test` passes (target: 60+ tests — roughly double the current 33)
- `cargo clippy --all-targets -- -D warnings` clean
- `cargo clippy -- -W clippy::pedantic` zero warnings (maintain the current standard)
- `cargo audit` clean
- All three sample fixtures still parse correctly
- Real-session smoke tests work for Claude Code, Codex, and Gemini
- README reflects the v1 feature set
- CLAUDE.md reflects the v1 architecture

## How to use this roadmap

Each session resuming cold should:
1. Read this file
2. Check which features are marked "done" vs "planned"
3. Pick the next "planned" feature from the build-order list
4. Implement + test + commit
5. Mark it done here

Future Claude sessions: treat this as the binding v1 scope. Do not drift into parked phases without explicit user authorization.
