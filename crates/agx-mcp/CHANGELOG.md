# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-05-23

### Bug Fixes

- *(ci)* Unblock v0.2.0 launch
Three independent issues blocked the first push's CI / release-plz:

  1. `replay::tests::execute_shell_times_out_on_long_running` failed on
     Linux because `/bin/sh -c "sleep 60"` orphans the sleep when we
     kill only the shell; the orphan inherits our stdout/stderr pipe
     FDs and the reader threads block on `read()` until the orphan
     exits naturally (60s in the test → CI hang). macOS passed by
     luck. Fix: spawn the shell as its own process-group leader
     (`CommandExt::process_group(0)`) and `libc::kill(-pgid, SIGKILL)`
     the whole group on deadline. Pipe closes immediately, reader
     threads return, timeout enforced. Adds `libc = "0.2"` under
     `[target.'cfg(unix)'.dependencies]` — already in the transitive
     dep tree via ratatui / crossterm / arboard.

  2. `cargo doc --workspace` failed with "document output filename
     collision" because agx-py's `[lib] name = "agx"` (intentional —
     Python imports it as `import agx`) collided with agx-cli's
     `[lib] name = "agx"` (added during the agx-cli rename so internal
     `use agx::…` paths kept resolving). Fix: `doc = false` on
     agx-py's `[lib]`. It's a Python cdylib, not a Rust API surface,
     so the rustdoc output was always cosmetic.

  3. `cargo publish` would have failed for agx-cli because its path
     dep on agx-core didn't carry a version field. crates.io requires
     path + version on workspace path-deps so the crate resolves both
     during workspace builds (via path) and from the registry (via
     version). Fix: `version = "0.1.0"` alongside `path` on all four
     agx-core consumers (agx-cli, agx-mcp, agx-py, agx-wasm).
     release-plz bumps them in lockstep on v0.2.0.

### Features

- Agx-mcp — MCP server for agent self-introspection
Ships a new workspace crate that exposes agx's read-only session
  tools over the Model Context Protocol. AI agents running under
  Claude Code / Cline / Gemini CLI can now query their own trace
  mid-session to self-budget, detect retry loops, and redact PII
  before persisting.

  New crate: `crates/agx-mcp/` — plain-Rust stdio binary
  (`agx-mcp --session <path>`). JSON-RPC 2.0 over stdio, MCP protocol
  version 2025-03-26, ~280 LOC, zero new deps beyond what agx-core
  already needs.

  Tool surface (v1, read-only):

  | Tool                    | Use case                                                 |
  |-------------------------|----------------------------------------------------------|
  | agx_session_summary     | Self-budget: cost / error thresholds                     |
  | agx_recent_errors       | Loop detection: same-tool failing repeatedly             |
  | agx_tool_distribution   | Stuck detection: 47 calls to Read                        |
  | agx_scan_pii            | Pre-commit guardrail using the Phase 6.4 scanner         |
  | agx_search              | Memory: "did I already look at this?"                    |

  Every tool returns JSON strings matching the stable schema from
  docs/eval-integration.md. Schemas versioned per docs/stability.md.

  Composition with sift (also documented in docs/mcp-integration.md):

  - Pre-write (agent-side): agent calls agx_scan_pii → redacts →
    writes. Sift never sees the leaked content.
  - At write time: sift runs its usual PreToolUse hook, independent
    of agx-mcp.
  - At review: `sift review` `t` keybind spawns agx on the session
    (Phase 5.5 --jump-to). Both tools see the same file.
  - Training-data loop (future): sift's accept/revert ledger +
    agx-mcp's eventual agx_annotate_step (Phase 8+) flow into
    --export trajectory-dpo.

  The three tools compose at the workflow level, not the API level —
  agx-mcp stays agx-focused, sift ships its own MCP when ready.

  Read-only by design for v1. Write tools (agx_annotate_step for
  self-reflection, agx_note_to_user for human-facing notes) have
  real coordination concerns across multiple agents sharing the
  annotations file, plus the note-channel needs its own design
  pass. Deferred to a follow-up phase.

  Wiring (Claude Code .mcp.json):

      {"mcpServers": {"agx": {
        "command": "agx-mcp",
        "args": ["--session", "${CLAUDE_SESSION_FILE}"]
      }}}

  Smoke-tested end-to-end: initialize → tools/list → tools/call on
  agx_session_summary all produce valid MCP responses. Workspace
  member, included in default-members alongside agx-core — plain
  Rust, no Python / WASM / system headers needed, so `cargo build`
  at repo root picks it up for free.

  New docs:
  - docs/mcp-integration.md — full wiring guide + sift composition
    story + stability commitments cross-reference
  - crates/agx-mcp/README.md — per-crate overview for crates.io

  352 tests pass across the workspace (unchanged — agx-mcp is
  scaffold + server, no new unit tests yet). Clippy + fmt clean.
- *(agx-mcp)* Agx_list_annotations tool — human → agent messaging
Opens a new channel in the agx-mcp surface: users leave notes via
  `a` in the TUI, agents read them on the next invocation to pick up
  guidance across turns / sessions without the user having to repeat
  themselves in the chat prompt.

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

