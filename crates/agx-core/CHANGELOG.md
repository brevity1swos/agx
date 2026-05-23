# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2026-05-23

### Miscellaneous

- Release ([#1](https://github.com/brevity1swos/agx/pull/1))
* chore: release

  * chore(release): bump workspace to 0.2.0 and restore hand-written CHANGELOG

  Two overrides to what release-plz proposed:

    1. Version: 0.1.0 → 0.2.0 across all 5 crates, not the 0.1.1
       semver-checks suggested. The bump reflects the substance of
       v0.2.0 — Phase 2 follow-on (LangChain, Vercel AI SDK, OTel
       GenAI JSON + binary protobuf), Phase 5 (fork detection,
       notifications, --jump-to, experimental shell replay with
       triple-gate safety and bounded resource use), Phase 6
       (trajectory-openai export, --redact, --scan-pii,
       --trajectory-stats / --sample), Phase 7 (workspace split,
       agx-py + agx-wasm scaffolds, formal stability + non_exhaustive
       enums), and the agx-mcp / agx doctor tooling tracks. Minor
       across the suite is the correct shape; semver-checks reported
       "API compatible changes" because the new public surfaces are
       additive rather than breaking.

    2. CHANGELOG: restore the hand-written v0.2.0 prose authored in
       `docs(changelog) aa3d26a`. Cliff prepended ~2000 lines
       covering every commit since the project started, because no
       v0.1.0 git tag exists for it to anchor against. The
       hand-written entry is denser and reads better; cliff takes
       over for v0.2.1 onward when there will be an anchor tag.

  Path-deps on agx-core also bumped to 0.2.0 so `cargo publish`
  resolves the registry version correctly.


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

