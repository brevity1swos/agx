# Changelog

All notable changes to agx are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

Stability commitments — which fields, flags, and APIs will or won't change across versions — live in [`docs/stability.md`](docs/stability.md).

## [Unreleased]

### Added

- **Phase 5.1** — Claude Code branch / fork detection. `Step.is_fork_root` + `b` TUI overlay + status-bar `[forks: N · b]` count.
- **Phase 5.3** — `--notify-on-error` / `--notify-on-idle <DURATION>` flags for `--live` mode. Opt-in feature `notifications` (notify-rust). Default build unchanged at 2.6MB.
- **Phase 5.5** — `--jump-to <STEP>` CLI flag: launch TUI pre-positioned at a 0-indexed step. Clamps to the visible range. Public contract for sift's Timeline-jump integration per `docs/suite-conventions.md` §5.
- **Phase 6.1** — `--export trajectory-openai` (one JSONL line per session, OpenAI fine-tuning shape). `--redact <NEEDLE>` literal-substring mask applies to every export format. `Step.tool_call_id` field added to the shared model.
- **Phase 6.2** — `agx corpus --trajectory-stats` emits distributional (min/p50/p90/p99/max/mean/total) breakdowns + branch / annotation / error rates. `--sample N` keeps the N most-recent sessions after filter. `ParsedSession.fork_root_count` added; surfaced in `--jsonl` output.
- **Phase 6.3** — [`docs/eval-integration.md`](docs/eval-integration.md) documents the stable JSON schema, anonymization checklist, and adapter recipes for inspect-ai, lm-eval-harness, and custom Python pipelines.
- **Phase 6.4** — `agx --scan-pii <session>` heuristic scanner. Catches AWS / Stripe / GitHub / OpenAI / Anthropic keys, JWT tokens, SSH private-key PEM headers, emails, IPv4. Read-only — pair with `--redact` to scrub.
- **Phase 7.1** — Workspace split into `crates/agx-core/` (pure parsers, timeline, corpus, pricing, annotations, PII, semantic, notify, export) and top-level `agx` (CLI + TUI). agx-core is publish-ready on crates.io.
- **Phase 7.2** — `crates/agx-py/` PyO3 Python bindings scaffold. `agx.load(path)`, `agx.load_corpus(dir)`, `agx.scan_pii(text)`. Build via `maturin`; abi3-py310 for cross-version wheels.
- **Phase 7.3** — `crates/agx-wasm/` wasm-bindgen bindings scaffold. `load(filename, bytes)`, `scan_pii(text)`, `version()`. Build via `wasm-pack`; bytes-in API so browsers/Node/Deno own I/O.
- **Phase 7.4** — [`docs/stability.md`](docs/stability.md) formal SemVer and schema-stability commitments. `Format` and `StepKind` enums marked `#[non_exhaustive]` so external consumers handle future variant additions without breaking.

### Infrastructure

- `src/lib.rs` thin re-export shim — every `agx::X` from earlier versions still resolves after the workspace split.
- `corpus::run` gained a `TuiLauncher` callback parameter so agx-core stays TUI-free.
- `Step.is_fork_root` and `Step.tool_call_id` added to the shared model (serde-defaulted; non-Claude-Code parsers leave them false/None).
- `src/lib.rs` Criterion benches build against agx-core directly (Phase 3.2 pipeline still green after the split).

### Deferred

- CI matrix for wheel / WASM publishing (tracked as Phase 7.4b).
- Phase 5.2 MCP-aware rendering (ecosystem-gated).
- Phase 5.4 Replay (experimental-gate design pending).
- Phase 6.1 long-tail trajectory formats (trajectory-hermes, trajectory-dpo, trajectory-sft).

## [0.1.0] - 2026-04-12

First release. A step-through debugger for AI agent execution traces — *just another gdb, but for your agent.*

### Format Support
- Claude Code session JSONL parser with graceful unknown-type handling
- Codex CLI session JSONL parser (response_item + function_call pairing)
- Gemini CLI single-JSON parser (atomic toolCall splitting)
- Generic conversation JSON parser (OpenAI/Anthropic SDK/Vercel AI SDK/LangChain)
- Auto-detection of session format by content sniffing (no flags needed)
- Multi-session browser — launch with no args to scan ~/.claude, ~/.codex, ~/.gemini

### Debugger Features
- Three-pane TUI layout: timeline / conversation view / detail (Tab toggles 2-pane)
- Bidirectional tool_use ↔ tool_result pairing with originating call input visible
- Alternating step colors for visual clarity between adjacent tool calls
- Batch/fork visualization — ║ markers for parallel tool dispatches
- Tool error detection — heuristic-based red highlighting across all formats
- Heatmap mode (h) — color-coded tool-call density with 5-level gradient
- Time-travel scrubbing bar with position indicator
- Latency annotations — per-step duration computed from timestamps

### Navigation & Workflow
- Jump to step (:N command mode)
- Filter by tool name / step kind (f) — case-insensitive substring match
- Content search with match highlighting (/) and n/N navigation with wrap
- Bookmarks (m\<char\> / '\<char\>) — survive filter cycles, report hidden-by-filter
- Mouse support — click-to-select on timeline, scroll wheel navigation
- Vim-style count prefixes (3j, 5k, 42G, 2d, 7n, ...)
- Clipboard copy (y) — copies current step detail to system clipboard

### Analysis
- Tool usage statistics overlay (s) — per-tool counts, error rates, sorted by frequency
- Session comparison (--diff) — cross-format text summary of tool usage and errors

### Modes
- Live attach (--live) — watches session file and auto-refreshes on changes
- Non-interactive --summary mode for scripts and CI
- Help overlay (? / F1) with keybinding reference and color legend
- Shell completions (--completions bash/zsh/fish)

### Quality
- 116 unit tests
- Clippy strict + pedantic clean (zero warnings)
- cargo audit clean
- Panic-safe terminal cleanup via TerminalGuard Drop impl
- Synthetic sample fixtures for all 4 formats (zero personal data)
