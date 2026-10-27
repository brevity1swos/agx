# Changelog

All notable changes to this project will be documented in this file.

## [0.3.0] - 2026-10-27

### Bug Fixes

- *(corpus)* Attribute per-model usage per step, not per session
Multi-model sessions credited their full token and cost totals to every model, so per_model sums exceeded the corpus totals. ParsedSession now carries a per-model split computed from the steps that carry each model.
- *(corpus)* Stop descending into symlinked directories
path.is_dir() follows symlinks, so a link loop fanned out up to max_depth. DirEntry::file_type does not follow links; symlinked files are still collected.
- *(annotations)* Use a per-process temp file for atomic saves
Two agx instances saving the same session shared one .tmp file and could publish interleaved JSON, which load_for then treats as empty.
- *(slice)* Keep annotations and live reload aligned with sliced views
Annotations are keyed by original step index, but slicing renumbered steps first, so sliced exports and the tui showed notes on the wrong steps and saved new ones under the wrong index. slice_steps_indexed returns the original indices; exports and the tui re-key notes through it, and the tui makes notes read-only while sliced. Replay logs use the original index too.

  --live reloads now re-apply the slice instead of swapping in the full session, and --diff slices both sessions.
- *(slice)* Reject bare-integer durations that overflow u64
The bare-integer branch multiplied unchecked: a panic in debug and a silently wrong filter in release.
- *(export)* Redact every serialized field and fence md details safely
--redact only masked label and detail, so tool_name, model, tool_call_id and annotation text leaked into json and trajectory exports. Markdown now picks a fence longer than any backtick run in the detail so tool output can't close it early.
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

### Style

- *(pii)* Collapse nested jwt match into a let chain
Raising rust-version to 1.88 lets clippy's collapsible_if suggest let chains, which made -D warnings fail.

### Build

- Raise msrv to 1.88
The declared 1.85 was already false: ratatui 0.30 needs 1.86, and darling, time and ort (embedding-search) need 1.88. Users on older toolchains got dependency build errors instead of a clear rust-version message. The workspace checks clean on 1.88 with every feature.


## [0.2.1] - 2026-06-19

### Bug Fixes

- *(pricing)* Add claude-opus-4-8 rate
agx_session_summary reported cost_usd:null for the current flagship model
  because it was missing from the exact-match pricing table.

### Refactoring

- *(pii)* Inline single-use snippet_around helper
The helper was just text[start..end].to_string(); its doc promised
  caller-side wrapping/truncation that never materialized. Inline at both
  call sites and drop the indirection.
- *(corpus)* Extract rate() helper for trajectory stats
Consolidates three identical count-as-f64 / session_count blocks and their
  three separate cast_precision_loss allows into one private helper. Callers
  early-return on an empty corpus, so total is always >= 1.


## [0.1.0] - 2026-05-23

### Bug Fixes

- *(docs)* Repair broken intra-doc link in slice.rs
`parse_step_range`'s docstring referenced `StepRange::from_cli_range`,
  a method that doesn't exist on `StepRange` (only `is_identity` does).
  The reference predates the CI workflow this push added; `cargo doc`
  with `-D warnings` now surfaces it.

  The 1-based → 0-based conversion happens at the slice site, not
  through a struct method, so the reference was always stale. Rewrites
  the docstring to describe the actual behavior without a dangling
  link.

### Features

- *(phase7.1)* Workspace split — agx-core + agx crates
Repo is now a Cargo workspace with two crates:

  - `crates/agx-core/` — pure parsers, timeline model, cost tables,
    corpus aggregation, export writers, annotations, PII scanner,
    semantic search, notifications. Zero TUI deps (no ratatui,
    no crossterm, no arboard). Publishable to crates.io standalone.
  - `agx` (top-level) — the TUI / CLI binary. Re-exports agx-core's
    public surface via `pub use agx_core::*;` so every existing call
    site (bench, integration tests, TUI modules) keeps working
    unchanged.

  22 pure modules moved via `git mv` to `crates/agx-core/src/`:
  annotations, browser, codex, corpus, debug_unknowns, diff_align,
  export, format, gemini, generic, langchain, loader, notify,
  otel_json, otel_proto, pii, pricing, semantic, session, slice,
  timeline, vercel_ai. Git tracks these as renames.

  3 TUI modules stay in `src/`: tui, corpus_tui, diff_tui.

  Public-surface API preserved. The split is a publish-shape change,
  not a rebinding:

  - `agx::timeline::Step` still resolves (now via agx-core re-export).
  - `agx::loader::load_session` still works.
  - Bench + integration tests build / run unchanged.

  One API shape change: `corpus::run` gained a `tui_launcher: &TuiLauncher`
  parameter so agx-core can delegate `--tui` dispatch back to the bin
  crate without inverting the dependency. Library consumers without a
  TUI pass `corpus::no_tui` which errors if `--tui` is ever set.

  Visibility bumps: `pub(crate)` → `pub` for items the bin crate
  reaches across the boundary — `timeline::{format_duration_ms,
  truncate, user_text_step, assistant_text_step, tool_use_step,
  tool_result_step}` and `semantic::{rank, FEATURE_DISABLED_MESSAGE}`.
  All intra-lib items stay `pub(crate)`.

  Feature flags mirror across both crates — `agx/otel-proto` →
  `agx-core/otel-proto`, etc. Users flip either name and get the
  same build.

  352 tests pass across the workspace (249 in agx-core + 90 in
  agx-bin + 12 integration + 1 scaffold). Clippy + fmt clean on
  default features. Release binary unchanged at ~2.6MB.

  Ready to publish. `cargo publish -p agx-core` is a one-command
  next step (deferred on purpose — user should confirm the crates.io
  name).

  Unlocks Phase 7.2 (Python bindings via pyo3) and 7.3 (TypeScript /
  WASM bindings) — both now trivially depend on agx-core as a pure
  library.
- *(phase7.4)* Formal stability doc + non_exhaustive enums + CHANGELOG
Closes the last Phase 7 subplan except the CI wheel / WASM matrix
  (deferred to 7.4b — that's publishing-workflow shape).

  New doc: `docs/stability.md`. Formalizes what agx promises to hold
  across versions:

  - SemVer rules for the 4 public surfaces (CLI flags, export JSON
    schema, agx-core Rust API, Python / WASM bindings).
  - Cross-tool compat table semantics — sift is the downstream
    consumer of `agx --export json` + `--jump-to`; agx CHANGELOG
    flags breaks and sift's `doctor` subcommand reports them.
  - JSON schema rules: field names / types / enum values are stable.
    Additions are MINOR; removes / renames are MAJOR.
  - Feature-flag stability: can't remove a flag without MAJOR bump.
  - Deprecation policy: flag or API item stays functional for ≥1
    MINOR past the CHANGELOG note, with a stderr warning where
    possible, before removal in the next MAJOR.

  Code change: `Format` and `StepKind` enums marked
  `#[non_exhaustive]`. External callers now need a wildcard arm —
  internal matches stay exhaustive because `non_exhaustive` only
  affects consumers outside the defining crate. This buys us
  ecosystem-friendly forward compat: shipping a new `Format` variant
  (LlamaIndex, Pydantic AI…) or `StepKind` variant (McpResourceRead
  from Phase 5.2) no longer requires a MAJOR bump for external
  consumers who match.

  Two external matches in `src/tui.rs` (`kind_color`, detail-title)
  needed wildcard arms. Added with comments explaining the
  forward-compat default behavior.

  CHANGELOG.md gets an `[Unreleased]` section summarizing every
  Phase 5 / 6 / 7 commit with pointers to the relevant subplans in
  ROADMAP.md. Top-of-file header now links to docs/stability.md so
  release managers always see the contract.

### Refactoring

- /techdebt + /simplify + /security-scan sweep (10 iters)
Ran a 10-iteration agent-led sweep on the post-Phase-7 workspace.
  Iters 1-2 applied all 7 concrete findings; iters 3-10 were
  verification passes that kept the full function-test suite green.
  Final: 264 tests pass (+3 new regression tests for the exit-code
  parser), clippy clean across default + otel-proto features +
  agx-py + agx-wasm (including wasm32 target checks), cargo audit
  reports 2 unmaintained-dep warnings (both transitive via
  `fastembed` under the `embedding-search` feature gate — not
  reachable in the default build), fmt clean.

  Concrete fixes (1 HIGH + 6 MED):

  **HIGH — agx-wasm broken on its primary target (wasm32).**
  The hand-rolled `tempfile_like` wrote to `std::env::temp_dir()`
  which doesn't exist on `wasm32-unknown-unknown`. Replaced the
  native path with `tempfile::NamedTempFile` (O_EXCL + random name
  + Drop-based cleanup, target-gated as a non-wasm32 dep); wasm32
  now returns a clear actionable error instead of panicking inside
  `fs::File::create`. Deleted two `#[allow(dead_code)]` "referenced"
  shim fns that existed only to suppress unused-import warnings.

  **MED — agx-mcp re-parsed the session on every tool call.**
  Added a mutex-guarded `(mtime, Vec<Step>)` cache at module scope.
  Cache hit on unchanged mtime, miss (+ reparse) on growth. The
  common MCP pattern (agent fires 4-6 tool calls in quick
  succession) now parses once instead of N times.

  **MED — agx-py `load_corpus` hand-rolled dict construction
  duplicated field names and had already silently omitted
  `result_count` from `tool_stats`.** Derived `Serialize` on
  `ParsedSession` + `ToolStats` + `Format` so the whole struct
  crosses into Python via `serialize_to_py` (serde_json → PyAny),
  same bridge shape `agx.load(path)` already used. 22 lines of
  hand-rolled dict construction removed; schema drift risk now
  lives on one type, not both sides of the FFI.

  **MED — agx-wasm temp-file symlink race + leak** (two related
  items): fixed by `NamedTempFile` above — random filename avoids
  prediction, O_EXCL rejects symlink hijack, Drop removes the file
  on return.

  **MED — `is_error_result` exit-code substring matched across
  boundaries.** "exit code 1" matched "exit code 127" (accidentally
  correct) but also "exit code 10" (false positive on valid
  non-error codes like batch-completion). Replaced the 18 hardcoded
  "exit code N" / "process exited with code N" substrings with
  `haystack_has_nonzero_exit_code`, which parses the integer
  cleanly and rejects 0. Added 3 regression tests: clean 127 is
  detected, 0 is rejected, 10 is detected (but via integer parse
  not prefix coincidence), and an embedded-after-verbose-output
  case.

  Non-fixes (deliberate):
  - agx-wasm's wasm32 path returns an error rather than supporting
    bytes-first parsing. Real fix is `agx_core::loader::load_bytes`
    with bytes-first parser entry points across 8 parsers —
    documented in CHANGELOG under Deferred as a separate follow-up
    commit.
  - `cargo audit` unmaintained warnings (`number_prefix`, `paste`)
    live only in `fastembed`'s transitive tree. Unreachable in the
    default build; revisit if `fastembed` moves to maintained
    alternatives upstream.

  Simplifier agent found nothing worth simplifying — the codebase
  post-split was already idiomatic per its review.

