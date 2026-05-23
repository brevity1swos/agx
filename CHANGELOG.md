# Changelog

All notable changes to agx are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

Stability commitments — which fields, flags, and APIs will or won't change across versions — live in [`docs/stability.md`](docs/stability.md).

## [Unreleased]

## [0.2.0] - 2026-05-23

The substance release. Adds two more agent-trace formats (LangChain, Vercel AI SDK), full OpenTelemetry GenAI support, fork / branch detection, jump-to-step launch positioning, desktop notifications for live mode, trajectory export for RL training data, corpus-level distributional stats, PII / credential scanning, an experimental shell-replay subsystem with triple-gate safety, a workspace split with publish-ready Python (PyPI) / WASM (npm) bindings, formal stability commitments, an MCP server for agent self-introspection, and an `agx doctor` health-check subcommand.

The published crate is **`agx-tui`** — the `agx` name on crates.io was claimed by an unrelated project before this one existed. The installed binary remains `agx`:

```
cargo install agx-tui && agx --help
```

### Added — Format Support

- **OpenTelemetry GenAI (JSON)** — any OTLP-JSON traces export with `resourceSpans` + `gen_ai.*` attributes. Detection by content, no file-extension sniffing.
- **OpenTelemetry GenAI (binary protobuf)** — `.pb` / `.otlp` exports from `opentelemetry-collector` or OTLP/HTTP endpoints. Feature-gated behind `otel-proto` (`prost` adds ~500KB).
- **LangChain / LangSmith** — run-tree export from `LangSmith → Export run` or any LangChain tracer. Walks the `chain` / `chat_model` / `tool` tree and flattens by `start_time`.
- **Vercel AI SDK** — `generateText` / `streamText` result objects with camelCase fields (`toolCallId`, `toolName`, `args`) and per-step `usage` from `steps[]`.

### Added — Phase 5: Branch / Replay / MCP

- **Phase 5.1** — Claude Code branch / fork detection. `Step.is_fork_root` set on entries that share a `parentUuid`. New `b` TUI overlay; status-bar `[forks: N · b]` count when present. `fork_root_indices` / `fork_root_count` exposed for corpus consumers.
- **Phase 5.3** — `--notify-on-error` / `--notify-on-idle <DURATION>` flags for `--live` mode. Opt-in feature `notifications` (notify-rust). Default build unchanged at 2.6MB; OS notification failures never crash the TUI.
- **Phase 5.4 (experimental)** — `--experimental-replay` + `--allow-shell-replay` enable a shell-backend replay path on `tool_use` steps. Three independent gates must all pass before any execution: launch-flag intent announcement, tool-kind allow, and per-invocation `y` confirm. Every attempt appends one JSON line to `<session>.replay.log` next to the session; the original session is never touched. Bounded execution — 4 MiB per-stream output cap and 30 s wall-clock deadline — surfaced in the TUI status bar so a timed-out or truncated run is visually distinct from a normal completion.
- **Phase 5.5** — `--jump-to <STEP>` launches the TUI pre-positioned at a 0-indexed step. Clamps to the visible range. Public contract for sift's Timeline-jump integration per `docs/suite-conventions.md` §5.

### Added — Phase 6: Trajectory Export & Eval-Harness Integration

- **Phase 6.1** — `--export trajectory-openai` writes one JSONL line per session in OpenAI fine-tuning shape. `--redact <NEEDLE>` literal-substring mask applies uniformly to markdown / HTML / JSON / trajectory exports — redaction happens at the step layer, so every export format sees the same masked slice. `Step.tool_call_id` field added to the shared model for tool_use ↔ tool_result pairing without regex-extracting IDs.
- **Phase 6.2** — `agx corpus --trajectory-stats` emits min / p50 / p90 / p99 / max / mean / total distributional breakdowns plus branch / annotation / error rates. `--sample N` keeps the N most-recent sessions after filter. `ParsedSession.fork_root_count` added; surfaced in `--jsonl` output.
- **Phase 6.3** — [`docs/eval-integration.md`](docs/eval-integration.md) documents the stable JSON schema, anonymization checklist, and adapter recipes for inspect-ai, lm-eval-harness, and custom Python pipelines.
- **Phase 6.4** — `agx --scan-pii <session>` heuristic scanner. Catches AWS / Stripe / GitHub / OpenAI / Anthropic keys, JWT tokens, SSH private-key PEM headers, emails, IPv4. Read-only by design — pair with `--redact` to scrub.

### Added — Phase 7: Library Mode

- **Phase 7.1** — Workspace split. `crates/agx-core/` is the pure, TUI-free library (parsers, timeline, corpus, pricing, annotations, PII, semantic, notify, export). Top-level `agx-tui` keeps the TUI + clap + arboard dependencies. `agx-core` is publishable to crates.io independently for Python / WASM / eval-harness consumers.
- **Phase 7.2** — `crates/agx-py/` PyO3 Python bindings scaffold. `agx.load(path)`, `agx.load_corpus(dir)`, `agx.scan_pii(text)`. Builds via `maturin`; abi3-py310 means one wheel per platform across all Python ≥ 3.10.
- **Phase 7.3** — `crates/agx-wasm/` wasm-bindgen bindings scaffold. `load(filename, bytes)`, `scan_pii(text)`, `version()`. Builds via `wasm-pack` for browsers / Node / Deno; bytes-in API so the JS side owns I/O.
- **Phase 7.4** — [`docs/stability.md`](docs/stability.md) formalizes the SemVer and schema-stability commitments. `Format` and `StepKind` enums marked `#[non_exhaustive]` so external consumers handle future variant additions without breaking.
- **Phase 7.4b** — Wheel / WASM publishing workflows (`.github/workflows/python-wheels.yml`, `wasm-packages.yml`) — tag-triggered.

### Added — Tooling & Suite Integration

- **`agx-mcp`** — Model Context Protocol server exposing `agx_load_session`, `agx_search_steps`, `agx_summarize`, and `agx_list_annotations` so AI agents can introspect their own running session. See [`docs/mcp-integration.md`](docs/mcp-integration.md) for the typed tool surface.
- **`agx doctor`** — stepwise-suite health check subcommand. Reports installed siblings (rgx, sift), their versions, and the agx side of the shared CLI grammar.
- **`docs/agent-guide.md`** — natural-language cookbook for AI coding assistants operating agx on a user's behalf.
- **`docs/suite-conventions.md`** — shared CLI grammar / TUI keybindings / color palette / integration contracts for the stepwise suite. Maintained verbatim against the copies in rgx and sift.

### Changed — Replay hardening (post-Phase 5.4)

- Cancel-on-non-confirm: pressing Esc or any non-`y` key after the `R` prompt now cancels cleanly instead of leaving a primed confirm.
- Empty-command refusal: the classifier returns `NotReplayable` for both extract-failed and extract-returned-empty paths, so a malformed step can't spawn `/bin/sh -c ""`.
- 4 MiB per-stream output cap with reader threads that keep draining past the cap (so a child that ignores SIGPIPE on a full pipe buffer can't deadlock).
- 30 s wall-clock deadline via `try_wait` polled every 50 ms; over-deadline children are killed and reaped with `timed_out` set.
- New `timed_out`, `stdout_truncated`, `stderr_truncated` flags on `ReplayOutput` and on each sidecar log entry — schema-additive so old consumers that ignore unknown fields keep working.

### Infrastructure

- Crate published to crates.io as `agx-tui`; the binary, the internal lib (`use agx::…`), and the brand all remain `agx`.
- `src/lib.rs` is a thin re-export shim — every `agx::X` from earlier versions resolves after the workspace split.
- `corpus::run` gained a `TuiLauncher` callback parameter so `agx-core` stays TUI-free.
- `Step.is_fork_root` and `Step.tool_call_id` added to the shared model (serde-defaulted; non-Claude-Code parsers leave them false / `None`).
- Criterion bench suite (`benches/agx_bench.rs`) builds against `agx-core` directly so the Phase 3.2 perf pipeline survives the workspace split.
- `release-plz` + `cliff` + a stable CI workflow wired up so subsequent releases follow conventional-commit-driven version bumps.

### Deferred

- Phase 5.2 — MCP-aware rendering. Ecosystem-gated; lands once MCP tool-call metadata stabilizes across agent CLIs.
- Phase 6.1 long-tail trajectory formats — `trajectory-hermes`, `trajectory-dpo`, `trajectory-sft`. `trajectory-openai` shipped; the rest pending demand signal.

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
