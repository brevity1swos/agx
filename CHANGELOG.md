# Changelog

All notable changes to agx are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

Stability commitments — which fields, flags, and APIs will or won't change across versions — live in [`docs/stability.md`](docs/stability.md).

## [Unreleased]

## [0.3.0] - 2026-10-27

### Bug Fixes

- *(corpus)* Attribute per-model usage per step, not per session
Multi-model sessions credited their full token and cost totals to every model, so per_model sums exceeded the corpus totals. ParsedSession now carries a per-model split computed from the steps that carry each model.
- *(corpus)* Stop descending into symlinked directories
path.is_dir() follows symlinks, so a link loop fanned out up to max_depth. DirEntry::file_type does not follow links; symlinked files are still collected.
- *(annotations)* Use a per-process temp file for atomic saves
Two agx instances saving the same session shared one .tmp file and could publish interleaved JSON, which load_for then treats as empty.
- *(cli)* Reject flag combinations that were silently ignored
--diff overrode --export/--summary, --scan-pii overrode --export, --live did nothing outside the tui, and --export overrode --summary. These now fail at parse time. Drops the --scan-pii doc claim of JSON output, which was never implemented.
- *(slice)* Keep annotations and live reload aligned with sliced views
Annotations are keyed by original step index, but slicing renumbered steps first, so sliced exports and the tui showed notes on the wrong steps and saved new ones under the wrong index. slice_steps_indexed returns the original indices; exports and the tui re-key notes through it, and the tui makes notes read-only while sliced. Replay logs use the original index too.

  --live reloads now re-apply the slice instead of swapping in the full session, and --diff slices both sessions.
- *(slice)* Reject bare-integer durations that overflow u64
The bare-integer branch multiplied unchecked: a panic in debug and a silently wrong filter in release.
- *(export)* Redact every serialized field and fence md details safely
--redact only masked label and detail, so tool_name, model, tool_call_id and annotation text leaked into json and trajectory exports. Markdown now picks a fence longer than any backtick run in the detail so tool output can't close it early.
- *(replay)* Detach child stdin from the tui terminal
- *(replay)* Stop waiting on pipes held by background grandchildren
After the shell exits, give the readers a short grace, then kill the
  process group and collect output with a bounded wait so a command
  like `sleep 30 &` no longer freezes the tui.
- *(tui)* Keep selection valid when live reload grows from or to empty
- *(tui)* Restore terminal when entering the alt screen fails
- *(mcp)* Recover from poisoned step cache mutex instead of panicking
A panic while holding the cache lock poisoned it, after which every
  tool call crashed the server via expect(). Recover the inner value at
  both lock sites.
- *(mcp)* Reply to null-id requests and reject malformed ones
serde collapsed an explicit "id": null into None, so such requests
  were treated as notifications and never answered. Inspect the raw
  object instead: only an absent id is a notification. Valid JSON that
  is not a well-formed request (non-object, missing method, wrong
  jsonrpc version, bad id type) now gets -32600 instead of -32700.
- *(mcp)* Answer ping with an empty result
MCP requires servers to respond to ping with {}; we returned -32601,
  which hosts can read as a dead server.
- *(mcp)* Follow mcp error shape for tool calls
Unknown tool names and invalid arguments are now -32602 and are
  checked before the session file is loaded. Failures while running a
  valid call are returned as a successful result with isError: true
  and the message as text content, instead of a -32603 RPC error the
  model never sees.
- *(session)* Dedupe usage across content-block entries sharing message id
Claude Code writes one JSONL line per content block and each line
  repeats the full message.usage, so per-entry attachment multiplied
  token and cost totals. Attach usage + model only on the first entry
  per message.id; entries without an id keep per-entry behavior.
- *(timeline)* Skip zero-count failed and null error fields in heuristic
" failed" matched test summaries like "5 passed; 0 failed", and the
  "error" key indicator fired on {"error": null} / false / "". Check the
  preceding count and the key's value before flagging a tool result.
- *(timeline)* Validate timestamp fields and apply utc offsets
Malformed dates (month 0/13, day 0, Feb 30, negative year) underflowed
  the u64 days_from_civil math and produced garbage timestamps. Validate
  each field, compute in i64, and return None for pre-epoch instants.
  Numeric offsets like +09:00 are now applied instead of ignored.
- *(langchain)* Pair tool runs with pending tool_use steps
Tool runs only checked steps.last() for a same-name tool_use, so
  parallel calls duplicated tool_use steps, and the tool_result used the
  run id instead of the LLM tool_calls[].id. Track unclosed chat_model
  tool_uses, match by recorded tool_call_id or FIFO by name, and reuse
  the matched use's id on the result.
- *(langchain)* Accept null for string and list run fields
LangSmith exports write explicit null for empty child_runs / names,
  which #[serde(default)] alone rejects. Add a shared null_as_default
  deserialize helper and apply it to Run's String / Vec fields.
- *(generic)* Accept null tool_calls and object-valued arguments
OpenAI SDK dumps write "tool_calls": null on plain replies, and some
  OpenAI-compatible exporters store function.arguments as a parsed object
  instead of a JSON string. Both previously failed the whole parse.
- *(otel)* Accept numeric or null startTimeUnixNano
OTLP-JSON specifies nano timestamps as strings, but several exporters
  emit plain JSON numbers, which failed the whole parse. Accept string,
  number, or null, and tolerate null span / attribute lists.
- *(vercel-ai)* Read v5 input and output tool fields
AI SDK v5 renamed toolCalls[].args to input and toolResults[].result
  to output, so v5 traces rendered empty tool inputs and results. Fall
  back to the v5 names when the v4 ones are absent.
- *(otel)* Skip prompt history replayed in later chat spans
Instrumentation that records the full conversation in gen_ai.prompt.N
  re-emitted every earlier turn on each chat span. Skip the leading
  prompts that match the text turns already on the timeline, deriving the
  history from emitted steps so the binary OTLP path benefits too.
- *(format)* Sniff utf-8 on a prefix before routing to otel proto
A single invalid byte anywhere in a text session sent the whole file to
  the protobuf parser. Decide text vs binary on the first 8 KiB, tolerating
  a multibyte char cut by the window, then detect on a lossy view.
- *(session)* Decode jsonl lines lossily instead of failing on bad utf-8
- *(ci)* Release python and wasm bindings on agx-core tags
Both workflows triggered on v* tags, but release-plz pushes per-crate tags (agx-core-v0.2.1), so neither ever ran and nothing reached PyPI or npm.

  - trigger on agx-core-v* and stamp the bindings version from that tag, so agx-py and agx-wasm version in lockstep with agx-core
  - publish to PyPI as agx-py (the bare agx name belongs to an unrelated project); the import name stays agx
  - switch PyPI publishing to trusted publishing and npm to --provenance
  - install wasm-pack with taiki-e/install-action instead of curl | sh
  - collapse the four wheel jobs into one matrix and pin every action
  - bump agx-py and agx-wasm to 0.2.1 to match agx-core
- *(cost)* Correct pricing and token counts for current sessions
Cost estimates were missing or wrong for nearly every real session:

  - pricing rows were unverified estimates; Opus 4.6/4.8 were priced at $15/$75 instead of $5/$25, and no current model (claude-opus-5-5, claude-sonnet-5-5, gpt-5.4, gemini-3-flash-preview, ...) was listed. Rows are now verified against the providers' pricing pages (2026-10-27), Gemini names use dots as sessions record them, and long-prompt tiers (Haiku 5.5, Gemini 2.5 Pro, GPT-5.4+) are priced via LongContextTier
  - Claude Code writes 1-hour-TTL cache entries, which bill at 2x input, not the 5-minute 1.25x. Step gains an optional cache_create_1h (the 1-hour share of cache_create) and pricing a 1-hour write rate. Reproduces Claude Code's own recorded cost for a real session to the cent
  - OpenAI and Gemini report prompt counts that include cached tokens, so cached input was billed twice. Usage::from_cache_inclusive stores uncached input, matching the Anthropic convention tokens_in already documented
  - real Codex rollouts carry the model on turn_context and usage on token_count events, neither of which was read, so every Codex session had no tokens or model. Usage is now taken from total_token_usage deltas (token_count repeats)
  - real Gemini CLI sessions write a tokens object instead of usageMetadata, so they had no tokens either; thoughts bill as output
- Point feature rebuild hints at the agx-tui crate
The hints said cargo install agx --features ..., but the crates.io crate named agx is an unrelated project; agx is published as agx-tui.
- *(debug-unknowns)* Stop reporting known metadata entries
Current Claude Code sessions write many metadata entries (ai-title, attachment, api-request, file-history-*, ...) that agx skips on purpose; reporting them buried real drift. Content items are now scanned only on user/assistant entries, since metadata entries embed whole request copies. Codex reasoning items and Gemini info messages are known too. Codex custom_tool_call / local_shell_call stay reported as real coverage gaps.

### Documentation

- Sync claude.md architecture tree with the workspace layout
Parsers and core modules moved to crates/agx-core/src; the tree still listed them under src/ and omitted agx-mcp, agx-py, agx-wasm and replay.rs. Also documents prefix-based UTF-8 sniffing in format detection.
- Refresh jump-to contract, token convention and test count
--jump-to shipped as <step> <session>, not the planned <session>:<step>. Documents the uncached-input token convention and the pricing-row fields.

### Miscellaneous

- Add launch/ promotion folder
Promotion plan + per-channel drafts (Show HN, Lobste.rs, Terminal Trove, MCP
  registries, Twitter/X) mirroring the rxray launch template, tailored to agx:
  zero-instrumentation multi-CLI parsing + the agx-mcp self-introspection angle.
  awesome-ratatui already merged; awesome-rust deferred until the >50star/>2000dl bar.
- Add launch preview.png and mark pre-launch items done
1400x800 still cropped from the demo (3-pane timeline + token/cost detail) for
  the Terminal Trove image field. Repo homepage set to docs.rs/agx-core. Launch
  docs updated to reflect both done.

### Style

- *(pii)* Collapse nested jwt match into a let chain
Raising rust-version to 1.88 lets clippy's collapsible_if suggest let chains, which made -D warnings fail.

### Build

- *(deps)* Bump deps with open rustsec advisories
Updates crossbeam-epoch, h2, rustls, rustls-webpki, webpki-roots, webpki-root-certs and notify-rust (drops quick-xml 0.37) within semver, and pyo3 0.26 -> 0.29 for agx-py. cargo audit now reports no vulnerabilities.
- Raise msrv to 1.88
The declared 1.85 was already false: ratatui 0.30 needs 1.86, and darling, time and ort (embedding-search) need 1.88. Users on older toolchains got dependency build errors instead of a clear rust-version message. The workspace checks clean on 1.88 with every feature.

### Ci

- Add msrv, wasm32 and embedding-search jobs
msrv checks the workspace on 1.88 with every feature. wasm checks agx-wasm for wasm32-unknown-unknown, since --workspace only builds it for the host. embedding-search was never compiled in ci; it runs clippy only, because the semantic tests would download a ~90MB model each run.

  Also pins every action to a commit sha, sets a read-only token, drops checkout credentials, and cancels superseded pr runs.
- Run cargo audit on lockfile changes and weekly
Advisories accumulated unnoticed because nothing ran cargo audit. The weekly schedule catches advisories published against an unchanged lockfile.
- *(release)* Serialize release-plz runs and pin actions
Back-to-back pushes to main could run release-plz concurrently and race on the release pr or tags.


## [0.2.1] - 2026-06-19

### Bug Fixes

- *(pricing)* Add claude-opus-4-8 rate
agx_session_summary reported cost_usd:null for the current flagship model
  because it was missing from the exact-match pricing table.

### Documentation

- Sync docs with shipped workspace layout and 8-format support
Correct stale references left over from the workspace split and the
  format-parser expansion:

  - src/ paths → crates/agx-core/src/ (parsers now live in agx-core)
  - format count 5 → 8 (adds LangChain/LangSmith, Vercel AI SDK, and
    OTel binary protobuf)
  - corrected agx-mcp tool names in CHANGELOG (agx_session_summary,
    agx_recent_errors, agx_tool_distribution, agx_scan_pii, agx_search)
  - expanded keybinding reference and dep-discipline wording

  No code changes. Plain docs: prefix keeps release-plz's release_commits
  filter from firing.

### Refactoring

- *(pii)* Inline single-use snippet_around helper
The helper was just text[start..end].to_string(); its doc promised
  caller-side wrapping/truncation that never materialized. Inline at both
  call sites and drop the indirection.
- *(corpus)* Extract rate() helper for trajectory stats
Consolidates three identical count-as-f64 / session_count blocks and their
  three separate cast_precision_loss allows into one private helper. Callers
  early-return on an empty corpus, so total is always >= 1.
- *(mcp)* Use const-initialized Mutex for the step cache


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

- **`agx-mcp`** — Model Context Protocol server exposing `agx_session_summary`, `agx_recent_errors`, `agx_tool_distribution`, `agx_scan_pii`, `agx_search`, and `agx_list_annotations` so AI agents can introspect their own running session. See [`docs/mcp-integration.md`](docs/mcp-integration.md) for the typed tool surface.
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
