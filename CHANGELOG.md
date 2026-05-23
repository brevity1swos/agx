# Changelog

All notable changes to agx are documented here. Format follows [Keep a Changelog](https://keepachangelog.com/).

Stability commitments — which fields, flags, and APIs will or won't change across versions — live in [`docs/stability.md`](docs/stability.md).

## [Unreleased]

## [0.1.1] - 2026-05-23

### Bug Fixes

- *(demo)* Prepend target/release to PATH in demo.tape
The demo was failing with "command not found" because vhs runs each
  tape in a fresh shell with no knowledge of the repo's ./target/release
  build. Typing `agx ...` couldn't find the binary.

  Prepend a hidden `export PATH=$PWD/target/release:$PATH` at the top of
  the tape so the rest of the commands read as plain `agx …` in the
  recording. Ctrl+L clears the export line from screen before `Show`.
  Regenerated demo.gif with the fix — size grew from 50KB (blank /
  error-only frames) to 828KB (actual animated demo).

  Also added a prerequisites comment at the top of the tape so future
  contributors know they need `cargo build --release` before running
  `vhs assets/demo.tape`.
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
- *(docs)* Repair broken intra-doc link in slice.rs
`parse_step_range`'s docstring referenced `StepRange::from_cli_range`,
  a method that doesn't exist on `StepRange` (only `is_identity` does).
  The reference predates the CI workflow this push added; `cargo doc`
  with `-D warnings` now surfaces it.

  The 1-based → 0-based conversion happens at the slice site, not
  through a struct method, so the reference was always stale. Rewrites
  the docstring to describe the actual behavior without a dangling
  link.

### Documentation

- Add comprehensive README and CLAUDE.md
- Add tagline, format support table, and explicit codex/gemini status
- Add tagline "Step through and debug AI agent traces without leaving
    your terminal. Written in Rust." mirroring rgx's subtitle structure.
  - Add Format support section up front so readers aren't misled: only
    Claude Code session JSONL works on v0.1.0. Codex CLI parses without
    crash but produces 0 timeline steps (different schema: wrapped in
    {timestamp, type, payload}, uses response_item/event_msg/turn_context
    types and role: developer). Gemini CLI hard-errors on load (single
    JSON object wrapper, not JSONL).
  - Update "Not yet implemented" list to name Codex CLI and Gemini CLI
    explicitly and cross-reference the Format support table.

  Multi-format support is a planned v0.2.0+ expansion per CLAUDE.md's
  "support a new agent trace format" common task.
- Add v1.0 pre-release roadmap
Binding scope for the pre-public-release buildout. After a three-option
  brainstorm (full v1 / moderate cut / ship-fast minimum), option 3 was
  chosen to protect Road A's grant-forge SaaS queue while still producing
  a v1 with enough depth that cloning is harder than adopting.
- Mark roadmap features 1-4 as done
- Alternating step colors (feature 1)
  - Time-travel scrubbing bar (feature 2)
  - Jump to step :N (feature 3)
  - Filter by tool name / step kind (feature 4)
- Mark roadmap feature 5 (content search) as done
- Mark roadmap feature 6 (bookmarks) as done
- Update README — move shipped features from Not Yet to Working
15 features shipped since the original v0.1.0 README: multi-format
  parsers, three-pane layout, filter, search, bookmarks, mouse, vim
  counts, error detection, latency, batch markers, stats overlay,
  session diff, multi-session browser, scrubbing bar, jump-to-step.

  Not Yet list trimmed from 9 items to 4 (heatmap, non-CLI formats,
  live attach, clipboard copy). Working section now reflects the full
  v1.0 + Phase 4 feature set with 112 tests.
- All features shipped — remove Not Yet Implemented section
Every item from the original Not Yet list has been implemented:
  heatmap, clipboard, live attach, generic conversation format.
  README Working section now lists all 23 features.
- Add demo GIF generated with vhs
- Add demo GIF, install section, and expanded usage examples
- Demo GIF embedded at the top of README (generated via vhs)
  - Install section with cargo install --path and shell completions
  - Try-it section now covers all 4 fixture formats, no-args browser,
    --live mode, --diff comparison, and --summary mode
- Sync architecture + status with shipped features; MSRV → 1.85
Catches the docs up to what the code actually does after Phases 0, 1,
  and 2.1. Also fixes a pre-existing MSRV inconsistency: Cargo.toml is
  `edition = "2024"` which requires rustc ≥ 1.85 (stabilized Feb 2025),
  but several docs still claimed MSRV 1.74 — wrong since the edition
  bump. No code change.
- Note  annotation keybinding in README try-it block
Missed in the Phase 4.3 commit because the README had been reformatted
  by  between my Read and Edit, so the anchor string didn't
  match. Tiny patch. No code change.
- Align agx with stepwise suite conventions and sift roadmap
Reviewed the stepwise philosophy across sift and rgx (sift/ROADMAP.md,
  sift/docs/suite-conventions.md, regex_101/README.md) and updated agx's
  own roadmap + docs to reflect the shared framing.

  What's now explicit in agx's docs:

  - **Stepwise positioning.** ROADMAP's executive summary names the
    three-tool suite (rgx / agx / sift) and states the shared thesis —
    "can humans keep control over an automated agentic workflow without
    paying the efficiency of that workflow for it?" — that every agx
    feature has to pass before shipping.
  - **Suite conventions checked in.** docs/suite-conventions.md is now
    present in agx, maintained verbatim against the rgx and sift copies
    (divergence is a smell, fix forward, no CI automation — maintainer
    discipline).
  - **Two new guiding principles** (adopted from sift): composition
    over feature bloat, and "human judgment is the point; automation
    of recognition is a regression." Also a new principle 9 pinning
    `--export json`, `--summary`, and future `--jump-to` as versioned
    public CLI contracts that sift depends on.
  - **Phase 5.5 added: `--jump-to <session>:<step>`.** Sift's `t`-keybind
    Timeline-jump integration (docs/suite-conventions.md §1 + §5)
    targets this flag. Lands with Phase 5's replay / branch work since
    the event-loop entry point is shared.
  - **Phase 8.5 added: stepwise suite retrofits.** `agx doctor`
    subcommand, cross-tool compatibility table in README, Pairs well
    with section from agx's perspective, optional `R` → rgx "Regex
    lens" keybind (only if dogfood demand surfaces).
  - **Sustainability kill criteria.** Explicit triggers for
    archive / deprecate (maintainer hasn't used the tool in 3 months,
    major CLI ships native replay, issue-response regresses).
    Rechecked at every Phase transition.
  - **CLAUDE.md "Stepwise suite context"** — short internal orientation
    for future maintainers: role split, the two load-bearing public
    contracts (`--export json`, future `--jump-to`), and the subprocess-
    boundary / one-way-coupling integration rules from conventions §6.
  - **Four new "Not to Do" items** covering: convention-doc drift,
    reaching into sift's `.sift/` directory, duplicating sift's
    review-gate surface, duplicating rgx's regex-authoring surface.
  - **README "Pairs well with"** refreshed with an explicit
    compatibility table per conventions §7 and a pointer to the
    conventions doc.

  No code changes; docs-only. 303 tests pass, fmt clean.

  Philosophy summary: agx must stand alone against browser dashboards.
  Suite compounding is a first-class design concern but never a rescue
  for a tool nobody uses on its own merits.
- *(phase6.3)* Eval-integration guide — JSON schema, anonymization, adapter recipes
Closes Phase 6.3. Docs-only — no code changes.

  New file: docs/eval-integration.md. Three things an integrator
  actually needs, in one place:

  1. **Stable JSON schema reference.** Every field of
     `--export json` and `--export trajectory-openai` documented with
     type, nullability, and semantics. Stability commitments pinned
     to the public-CLI-contract list in docs/suite-conventions.md §5
     — breaks require minor-version bump + README compat-table entry.

  2. **7-step anonymization checklist.** Chains the Phase 6.4 scanner
     into the Phase 6.1 redactor end-to-end: `--scan-pii` → review
     matches → `--redact` → re-scan until clean. Plus user/host/path
     sweep, corpus `--trajectory-stats` sanity check, agx-annotations
     awareness, third-party-tool-output license reminder.

  3. **Adapter recipes for inspect-ai, lm-eval-harness, and a hand-
     rolled Python pipeline.** All use subprocess over the CLI (no
     shared Rust crate leak), demonstrating the stepwise suite's
     "compose at the process boundary" pattern from
     suite-conventions §6.

  Also includes a schema-drift reporting section telling external
  integrators exactly what to include when filing a schema break.

  README's Try-it block links the new doc. ROADMAP 6.3 marked shipped.

  Phase 6 is now fully shipped: 6.1 trajectory-openai + redact ✅,
  6.2 trajectory-stats + sample ✅, 6.3 eval integration docs ✅,
  6.4 --scan-pii ✅.
- Agent-guide.md — natural-language cookbook for AI agents operating agx
Closes the gap flagged in the sift-docs review — sift has
  `docs/agent-guide.md` for the same audience; agx didn't. Now it
  does, and the two guides cross-reference each other.
- *(roadmap)* Status-at-a-glance table at the top
Closes the last open item from the sift-docs review — both agx
  and sift ROADMAPs ran past 1000 lines with status information
  buried inside per-subplan bullet lists. Reviewers opening the
  doc for the first time now see the current state immediately.
- *(phase8.1)* Contributor guide for new format parsers
Pivots Phase 8.1 from "ship Aider / Windsurf / Zed parsers"
  (blocked on real-world sample files) to "ship the path for
  contributors to land these parsers." The 8 existing parsers all
  follow the same pattern; formalizing it into a doc unblocks
  community contributions in a way I can't unblock alone by
  guessing at formats.

  New file: docs/adding-a-parser.md (~200 lines). Covers:

  - **Pre-flight** — confirm the format writes locally, get 3
    real-world samples, confirm a deterministic detection signal,
    check upstream stability.
  - **12-step checklist** — module creation → Format variant →
    detect wiring → loader wiring → attach_usage_to_first convention
    → drift scanner → synthetic fixture → unit tests → docs →
    verify. Every step points at the existing parser that
    exemplifies it.
  - **Known formats waiting for a parser** — catalogs Aider (prefer
    `.aider.llm.history` JSONL over `.aider.chat.history.md`
    Markdown), Windsurf (hosted-by-default, schema unstable), Zed
    Assistant (SQLite, needs feature-gated rusqlite), plus Cline /
    Continue / Cursor / OpenClaw / Hermes. Each entry has storage
    location + on-disk shape + challenges + references.
  - **When NOT to add a parser** — upstream CLIs that don't write
    locally, formats that change weekly without versioning,
    proprietary SQLite with no schema guarantee, fewer than 3
    samples.

  Roadmap 8.1 updated — the parsers themselves are now "ship as
  contributors step up" rather than "I ship them." Candidate list
  is preserved for discoverability.

  Why this pivot: without access to real sample files from each
  upstream CLI, a parser I wrote would be guessing at schema
  shape. The 8 existing parsers were all built from real samples;
  shipping a speculative parser would be first-in-class tech
  debt. The contributor guide routes PR energy to where real
  samples exist (the contributor's own machine).

  Docs-only change. No code impact. Tests + clippy + fmt
  unaffected.
- *(changelog)* Draft v0.2.0 entry covering Phases 2 follow-on through 7.4b
Hand-write the v0.2.0 entry before release-plz takes over so the
  first-substance release has phase-narrative prose, not a flat
  cliff-generated bullet list. release-plz / cliff pick up from
  v0.2.1 onward with conventional-commit-driven entries.

  Covers everything since v0.1.0:

    - 4 new format parsers (LangChain, Vercel AI SDK, OTel GenAI
      JSON + binary protobuf)
    - Phase 5: fork detection, notifications, experimental replay
      (5.4 + iter 1 + iter 2 hardening), --jump-to
    - Phase 6: trajectory-openai export, --redact, --scan-pii,
      --trajectory-stats / --sample
    - Phase 7: workspace split, agx-py + agx-wasm scaffolds,
      stability doc + #[non_exhaustive] enums, wheel/WASM CI
    - Tooling: agx-mcp MCP server, agx doctor, agent-guide,
      mcp-integration, suite-conventions
    - Crate rename to `agx-cli` for crates.io publishing

### Features

- Scaffold agx — step-through debugger for AI agent execution traces
A terminal TUI for stepping through AI agent execution traces, inspired
  by rgx. Visualizes user/assistant turns, tool calls, and tool results
  in a two-pane layout with color-coded steps and bidirectional tool_use ↔
  tool_result pairing.

  v0 scope (Claude Code session JSONL only):
  - Parser with serde + graceful unknown-type handling
  - Timeline builder that flattens entries into navigable steps and pairs
    each tool_result with its originating tool_use's name and input
  - ratatui TUI with two-pane layout, color-coded labels, panic-safe
    terminal cleanup, and a centered help overlay
  - 17 unit tests covering parser variants, timeline construction, tool
    pairing, fallback, truncate edge cases, and short_id boundaries
  - clippy clean under both default and pedantic lint groups
- Add synthetic sample session fixture and usage example
assets/sample_session.jsonl is a synthetic Claude Code session that
  exercises every entry type the parser and renderer handle: user text,
  assistant text, multi-content assistant turns (text + tool_use),
  tool_result with paired tool name, and a permission-mode header.

  UUIDs and tool_use IDs are all-zero / toolu_synthetic_* to make it
  obviously synthetic. The conversation is a short "write a Fibonacci
  function in Python" exchange with Write, Bash, and Edit tool calls —
  realistic enough for demos and parser tests, with zero personal
  information from any real session.

  README now points at the fixture for an immediate try-it experience
  and documents the TUI keybindings.
- Multi-format support — Codex CLI and Gemini CLI parsers
agx now auto-detects and parses session files from all three major
  agent CLIs: Claude Code, Codex CLI (OpenAI), and Gemini CLI (Google).
  Format is detected by content sniffing — no file extension or flag
  required.
- Alternating step colors, scrubbing bar, and jump-to-step command mode
Bundles v1.0 roadmap features 1+2+3 since they share outer-layout and
  command-mode infrastructure. First down payment on Phase 1 debugger depth.

  Alternating step colors:
  - Each tool_use and tool_result step gets an alternating dark background
    (Color::Indexed(236)) so adjacent tool calls visually separate at a
    glance. Text steps (user/assistant) keep default background.
  - compute_bg_flags() runs once at App::new and stores a parallel Vec<bool>
    keyed by tool-use-parity and tool-result-parity separately. Codex-style
    batched tool calls (tool1 tool2 tool3 result1 result2 result3) still
    alternate correctly because each parity counter is independent.

  Time-travel scrubbing bar:
  - Outer layout now reserves a 1-row status bar at the bottom of the frame.
  - Default mode: ratatui Gauge renders a progress bar keyed to current/
    total step position with the "N/M" label. Instant visual feedback of
    where you are in the session.
  - Alternate states: command input line when command mode is active, or a
    red error/status message when execute_command rejects input.

  Jump to step (:N):
  - Press `:` to enter command mode. Cursor shows as a blinking block after
    the typed digits on the status bar.
  - Digits accumulate, Backspace edits, Enter executes, Esc cancels.
  - execute_command parses the input as usize. Valid number -> goto_step(n)
    clamps to the last step index. Zero -> error message. Unparseable ->
    "unknown command" error message. Empty -> no-op.
  - 1-indexed from the user's perspective (matches the --summary output).

  Help overlay updated to document :N and the new alternating bg behavior.
- Filter by tool name / step kind
v1.0 roadmap feature 4. Press `f` to open a filter prompt; type a
  substring (case-insensitive) that matches against step labels, then
  Enter to apply. Empty input clears the filter. Esc cancels without
  applying.

  Because step labels embed both the kind prefix (`[tool]`, `[user]`,
  etc.) and the tool name, a single substring query naturally handles
  every filter use case:
  - `Read` → all Read tool_use and tool_result steps
  - `[tool]` → only tool_use steps
  - `[result]` → only tool_result steps
  - `[user]` → only user-text steps
  - `fib` → any step whose label or content preview mentions fib
- Content search with match highlighting and n/N navigation
v1.0 roadmap feature 5. Press / to open a search prompt; type a
  substring (case-insensitive) that matches against step labels OR step
  content, then Enter to apply. Press n for next match, N for previous.
  Matches wrap at the ends. Empty input clears search. Esc cancels
  input without applying.

  Difference vs filter (feature 4):
  - Filter HIDES non-matching rows (reduces list).
  - Search KEEPS all rows visible, HIGHLIGHTS matches with a distinct
    dark-yellow bg, and lets you jump between matches with n/N.
  - The two compose: searching within a filter only searches the
    currently-visible filtered view.
- Bookmarks with m<char> set and '<char> jump
v1.0 roadmap feature 6. Standard vi-style two-key sequences:
  - m<char>  sets a bookmark at the currently-selected step, keyed by char
  - '<char>  jumps to the bookmark under char

  Bookmarks persist for the session (not across invocations) and are stored
  by ORIGINAL step index, so they survive filter changes — set a bookmark,
  apply a filter, clear it, the bookmark still points to the right step.
  If the bookmarked original step is hidden by an active filter, jumping
  reports "bookmark '<ch>' points to step N (hidden by filter)" instead
  of silently failing or clearing the filter.
- Mouse support — click-to-select and scroll wheel navigation
v1.0 roadmap feature 7. Enables mouse capture at terminal init, handles
  scroll-up/down as prev/next step, and translates left-click inside the
  timeline list into a row selection.

  Click translation: mouse row relative to list_area.y (minus border)
  plus list_state.offset() gives the view index. click_to_select()
  clamps to filtered_view bounds and updates list_state. Clicks outside
  the list bounds are ignored.
- Auto-detect and highlight tool error results
v1.0 roadmap feature 8 (core — retry pattern detection deferred).
  Walks each ToolResult step and heuristically detects error outputs by
  scanning the Result: section for substring indicators common across
  Claude Code, Codex, and Gemini error formats:

  - "\"error\"" (JSON error field)
  - "error:"
  - "failed" (word-anchored)
  - "traceback"
  - "panic!"
  - "exception:"
  - "no such file"
  - "permission denied"
  - "command failed"
  - "exit code 1" through "exit code 9"
  - "process exited with code 1" / "...code 2"

  Matching is case-insensitive and scoped to the Result: section only
  (via split on "\nResult:\n") so tool inputs mentioning error words
  don't produce false positives.
- Three-pane layout with cursor-synced conversation panel
v1.0 roadmap feature 9. The biggest structural change of Phase 1.
  Default layout is now 3 columns: timeline list (25%) / conversation
  view (40%) / detail pane (35%). Tab toggles back to the 2-pane layout
  (40/60) for narrow terminals or user preference.

  The conversation panel is a second List widget that shows only user
  and assistant text steps (tool_use and tool_result are hidden —
  they're still fully visible in the timeline). Its cursor is
  auto-synchronized with the timeline cursor: when you navigate the
  timeline to any step, the conversation pane jumps to the nearest
  PRECEDING text step, which is typically the message that owns the
  tool call you're inspecting.

  This is the "dual cursor" concept borrowed from rgx (pattern cursor
  + text cursor), applied to agent debugging: one cursor on the step
  timeline, one cursor on the flowing conversation, synchronized so
  you can inspect both at once.
- Branch / fork visualization for batched parallel tool calls
v1.0 roadmap feature 10. Detects runs of 2+ consecutive tool_use or
  tool_result steps in the timeline and marks them with a dark-gray
  ║ prefix so parallel tool dispatches are visually distinct from
  sequential single-tool calls.

  Detection (compute_batch_flags): walks the step list and finds runs
  of consecutive same-kind ToolUse or ToolResult steps of length >= 2.
  Each step in such a run gets its flag set. Runs are bounded by steps
  of a different kind (text, or the other tool-step kind).

  This catches the real-world batched-parallel patterns:
  - Claude Code parallel Agent dispatches (3+ Agent tool_uses back-to-back)
  - Codex function_call batches (the rollout format sends all calls
    before their outputs arrive, naturally producing runs)
- Vim-style count prefixes for navigation (3j, 5k, 42G, ...)
v1.0 roadmap feature 11. agx's keybindings (hjkl, g/G, d/u, /, n/N, m,
  ', PgUp/PgDn, arrows) were already vim-compatible; the missing piece
  was count prefixes. Adding them completes the vim navigation feel
  without introducing a separate mode — there's no Insert vs Normal
  distinction in a read-only debugger, so no `--vim` flag is needed.

  Count accumulation:
  - Digits 1-9 always start a count.
  - 0 joins an existing count but is otherwise an unbound key (so you
    can't accidentally start a count with 0).
  - count_buffer is a String capped at 6 digits. take_count() parses
    and clears, returning max(parsed, 1) so 0 can't produce a no-op.
  - Count buffer is displayed as "×N" in the bottom status gauge label
    so the user sees their typed digits.

  Supported count actions:
  - `3j` / `3↓`     — next step, 3 times
  - `3k` / `3↑`     — prev step, 3 times
  - `2d` / `2PgDn`  — page down 2× (20 steps)
  - `5u` / `5PgUp`  — page up 5× (50 steps)
  - `7n` / `7N`     — next/prev search match, 7 times
  - `42G` / `42End` — jump to step 42 (standard vim `<N>G` semantics);
                      plain `G`/`End` still goes to last step
  - `5:` — count is cleared before entering command mode (count doesn't
            apply to input-mode entries)

  Count is cleared on: q/Esc, ?/F1, :, f, /, m, ', Tab, g/Home, and
  any unhandled key. Count persists only across digit presses until the
  next navigation action consumes it.

  Help overlay documents `<N><motion>` in the navigation section.
- Multi-session browser when launched without args
v1.0 roadmap feature 12 — the final feature in the pre-release scope.
  Running `agx` with no session path argument now scans the three known
  session storage locations, presents a numbered list of the most recent
  files across all formats, and prompts the user to pick one by number.

  Discovery locations:
  - Claude Code: ~/.claude/projects/<encoded-project>/<uuid>.jsonl
  - Codex CLI:   ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl
  - Gemini CLI:  ~/.gemini/tmp/<project>/chats/session-*.json

  For each discovered file, browser::discover_all() collects the path,
  format tag, and mtime (via std::fs::read_dir + metadata). Results are
  sorted by mtime descending so the most recent sessions float to the top.

  Presentation is a simple numbered stdin prompt rather than a second
  full TUI — keeps complexity low, avoids alt-screen coordination issues
  between the browser and the main debugger TUI, and produces a format
  that can be piped/logged. Each row shows:
    N. [Claude] 2h ago  -Users-.../7205f9a5.jsonl
  with the path rewritten to use ~/ prefix when it's under $HOME. Up to
  30 rows are shown; the rest are counted as "... (N more, not shown)".

  format_relative_time() formats the mtime delta as "just now", "Nm ago",
  "Nh ago", "Nd ago", or "Nmo ago" — same style as git / standard CLI tools.

  Also cleaned up pedantic clippy warnings introduced by feature 12:
  - browser.rs: use Path::extension() + eq_ignore_ascii_case instead of
    case-sensitive ends_with for jsonl/json suffix checks.
  - browser.rs, timeline.rs: moved const declarations to the top of the
    enclosing fn for clippy::items_after_statements cleanliness.
  - main.rs: if-let instead of single-arm match for the session dispatch.
- Tool usage statistics overlay (Phase 4)
Press `s` to open a centered stats popup showing per-tool aggregates:
  use count, result count, error count, and error rate percentage. Tools
  are sorted by use_count descending. Error-heavy tools show in red.
- Latency annotations with per-step duration (Phase 4)
Adds timestamp parsing and sequential duration computation across all
  three formats. Each step now shows "[Nms since previous step]" in its
  detail pane, giving instant visibility into how long each tool call
  or response took.
- Session comparison with --diff flag (Phase 4)
agx session1.jsonl --diff session2.jsonl prints a side-by-side text
  summary comparing two sessions across any combination of formats:
  - Step count breakdown by kind
  - Per-tool usage comparison (which tools appear in both, only in A/B,
    with delta counts)
  - Error count comparison

  Works cross-format: Claude Code vs Codex, Gemini vs Claude, etc.
  load_session() helper extracted from main() for reuse.
- Clipboard copy with y key
Press y to copy the current step's full detail text to the system
  clipboard. Uses the arboard crate (same as rgx). Status bar shows
  confirmation with step number and char count, or a red error if the
  clipboard is unavailable.
- Heatmap mode showing tool-call density
Press h to toggle heatmap mode. When active, each step's background
  color reflects how many tool_use/tool_result steps are within ±5 steps
  of it. Dense tool-call regions glow warm (orange→red), sparse regions
  are cool (blue→green), and text-only regions show no bg.

  5-level color gradient via 256-color indexed palette:
    0 calls  → no bg
    1-2      → dark blue (17)
    3-4      → dark green (22)
    5-7      → brown (130)
    8-10     → orange (208)
    11+      → red (196)

  Heatmap replaces alternating bg when active; search highlight still
  takes priority over both. Precomputed at App::new via sliding-window
  density calculation, O(n·w) where w=5.
- Live attach mode with --live flag
Pass --live to watch the session file for changes and auto-refresh the
  TUI. The event loop uses poll-based reading (500ms timeout) instead of
  blocking, so it can periodically re-load the file and update all
  derived state (steps, bg_flags, batch_flags, heatmap, conversation
  indices, tool stats) while preserving the cursor position.

  Reload is triggered when the step count changes (cheap proxy for file
  modification). Filter, search, and bookmarks are cleared on reload
  since the underlying step indices may have shifted.
- Generic conversation JSON format (OpenAI/Anthropic/Vercel/LangChain)
Adds a Generic conversation parser that handles the standard OpenAI
  chat completion message format: {messages: [{role, content, tool_calls,
  tool_call_id}]}. This covers:
  - OpenAI Assistants API logs
  - Anthropic SDK conversation exports
  - Vercel AI SDK chat histories
  - LangChain/LangSmith trace exports
  - Any tool that logs conversations in OpenAI-compatible JSON

  Format auto-detection: single JSON object with "messages" array but
  NO "sessionId" (which distinguishes it from Gemini) → Generic.

  Parser handles: user/assistant/tool/system roles. tool_calls with
  function.name + function.arguments (JSON string). tool role messages
  paired by tool_call_id. System messages skipped. Content supports
  both string and array-of-{type,text} formats.
- Shell completions, CHANGELOG, and demo tape (Phase 5 polish)
- Shell completions: --completions bash/zsh/fish via clap_complete.
    Prints completion script to stdout for eval/sourcing.
  - CHANGELOG.md: comprehensive v0.1.0 changelog organized by category
    (format support, debugger features, navigation, analysis, modes,
    quality).
  - Demo tape: assets/demo.tape for vhs — 20-second screencast showing
    launch, navigation, filter, search, heatmap, stats, layout toggle,
    help overlay.
- Phase 0 stabilization, positioning refresh, long-term roadmap
Ship Phase 0 (stabilization) from the roadmap — v0.1.x fixes that close
  gaps from the initial code review and set up regression guards for format
  drift.

  - Add --debug-unknowns flag (src/debug_unknowns.rs) that scans a session
    for entry types and fields the parser doesn't recognize and reports
    them to stderr with line-number samples. Zero deps, zero cost when off.
    Verified: flags permission-mode (Claude Code) and reasoning (Codex) as
    expected on existing fixtures.
  - Add integration tests: tests/summary_test.rs (6 tests for --summary
    output across all four formats) and tests/corpus_test.rs (walks
    tests/corpus/ and asserts every fixture parses; no-ops when empty).
  - Scaffold tests/corpus/ with layout docs; fixtures accumulate later via
    community contribution.
  - Add CONTRIBUTING.md with the parser-adding recipe and anonymization
    guide.
  - Add .github/ISSUE_TEMPLATE/ with format_drift, bug_report, and
    feature_request templates.

  Sharpen product positioning — agx is the terminal-native sibling of
  browser-based agent dashboards, analogous to rgx/regex101. Not a
  competitor to Langfuse/LangSmith. README tagline and intro rewritten to
  reflect this, CLAUDE.md and Cargo.toml descriptions updated.

  Add ROADMAP.md with 9 phases through v1.0. Structural shifts from prior
  drafts: OTel GenAI moves from v1.0 to Phase 2, corpus analytics + perf
  from v1.0 to Phase 3. New phases for RL/eval export (Phase 6) and library
  mode with agx-core + Python/TS bindings (Phase 7). CLI long-tail
  (Aider, Cline, Cursor, OpenClaw, Hermes) deferred to v1.0.
- *(phase1)* Token usage parsing, pricing table, --summary cost output
Phase 1.1 — per-step token usage:
  - Extend timeline::Step with model, tokens_in, tokens_out, cache_read,
    cache_create (all Option<u64>/String).
  - Parse usage+model from all four formats. Claude Code reads the Anthropic
    shape (input_tokens / output_tokens / cache_*_input_tokens). Codex reads
    OpenAI-style snake_case with legacy camelCase fallback. Gemini reads
    usageMetadata (promptTokenCount / candidatesTokenCount / cachedContent-
    TokenCount). Generic reads OpenAI per-message usage (prompt_tokens /
    completion_tokens / cached_tokens).
  - Convention: usage + model attach to the FIRST step emitted from each
    assistant message, so a corpus sum doesn't double-count when an assistant
    message produces text + tool_use steps.
  - Normalized crate-internal Usage struct + attach_usage_to_first helper
    keep the attachment point identical across all four parsers.

  Phase 1.2 — cost tables:
  - New src/pricing.rs with per-model USD-per-1M-token rates for opus-4-6,
    sonnet-4-6, haiku-4-5, gpt-5, gpt-5-mini, gemini-2-5-pro, gemini-2-5-
    flash. Each entry carries a last_verified field; a test asserts it's
    non-empty on every row. Cache rates follow provider-specific discounts
    where the provider publishes them; fall back to input rate otherwise.
  - Step::cost_usd() method delegates to pricing::cost_usd. Returns None on
    unknown model or zero tokens — agx doesn't fabricate cost numbers.

  Phase 1.3 (partial) — summary integration:
  - --summary now prints total-tokens, unique models, and estimated cost
    when the fixture carries usage data. Gracefully omits the lines when no
    step had usage (existing fixtures not enriched yet).
  - compute_session_totals in timeline aggregates across steps.
  - Enriched assets/sample_session.jsonl with realistic usage on all 4
    assistant turns (simulating cache-creation on turn 1, cache-reads on
    subsequent turns) so the pipeline is exercised end-to-end.
  - tests/summary_test.rs guards against regression on the enriched
    output.

  Remaining for Phase 1.3: stats overlay cost column, status bar running
  cost, per-step detail pane tokens+cost. Phase 1.4 (--export md/html/json)
  not started.

  Test state: 156 unit + 1 corpus + 7 integration = 164 tests passing.
  cargo clippy --all-targets -- -D warnings clean. cargo fmt --check clean.
- *(phase1)* --no-cost flag, TUI cost integration, export md/html/json
Completes Phase 1 of the roadmap.

  Phase 1.3 — TUI cost rendering:
  - --no-cost flag suppresses cost estimates in summary, status bar, detail
    pane, and stats overlay. Token counts are still shown — useful for
    unpriced custom models or when cost is noise.
  - Status bar appends running session cost alongside the position gauge
    when cost is available and --no-cost is off.
  - Detail pane prepends a meta block showing duration, model, tokens,
    and cost for each selected step. Block is skipped when none of those
    are known (tool_result steps without timing).
  - Stats overlay (`s`) adds a session-totals section above the per-tool
    table: tokens breakdown, unique models, estimated cost. Falls back to
    "(no pricing entry for model)" when unknown.
  - Step::cost_usd() is now a public method on Step; no more dead_code
    allows in pricing.rs.
  - tui::run signature gained a no_cost bool; all App::new call sites
    updated.

  Phase 1.4 — Export (md/html/json):
  - New src/export.rs module with three writers. Each takes (steps,
    totals, no_cost) and returns a String — no I/O, caller prints.
  - JSON export: stable-schema `{totals, steps}` pretty-printed. Step,
    StepKind, and SessionTotals now derive serde::Serialize. StepKind
    serializes as snake_case ("user_text", "assistant_text", "tool_use",
    "tool_result"). This JSON is the reserved public programmatic
    interface per Phase 7 library mode plan.
  - Markdown export: one H2 section per step with ASCII-only kind
    prefixes ([user] / [asst] / [tool] / [result]) — no emoji per the
    project's terminal-native principle. Code-fenced detail, meta line
    with duration/model/tokens/cost. Totals header at the top.
  - HTML export: self-contained single file with inline CSS. No JS, no
    external assets, no <link> or <script>. Color-coded by step kind
    matching the TUI palette (cyan/green/yellow/magenta). Step details
    are HTML-escaped to prevent injection.
  - --export md|html|json CLI flag (clap ValueEnum) writes to stdout;
    mutually exclusive with TUI launch, runs before --summary branch.
  - 8 unit tests cover JSON round-trip, MD structure, HTML
    self-containment, HTML injection prevention, and cost suppression.

  Test state: 164 unit + 1 corpus + 8 integration = 173 tests passing.
  cargo clippy --all-targets -- -D warnings clean. cargo fmt --check clean.

  ROADMAP.md Phase 1 marked ✅ shipped 2026-04-15.
- *(phase2.1)* OpenTelemetry GenAI JSON parser
Adds agx's fifth format. OTel GenAI is the emerging cross-framework
  instrumentation standard — supporting it unlocks LangChain, LlamaIndex,
  Vercel AI SDK, Pydantic AI, and anything else that exports OTel traces
  in a single parser.

  Implementation (src/otel_json.rs):
  - Parses OTLP-JSON envelope: resourceSpans → scopeSpans → spans.
  - Flattens AnyValue attribute shape (stringValue / intValue /
    boolValue / doubleValue) to a plain HashMap per span. intValue
    strings are re-parsed back to u64 (OTLP encodes int64 as a string
    to preserve precision over JSON).
  - Span classification via gen_ai.operation.name: `chat` /
    `text_completion` / `generate_content` → LLM span, `execute_tool` →
    tool span. Other operations ignored gracefully.
  - LLM spans: walks gen_ai.prompt.{N}.role/.content (user → UserText,
    system → dropped) then gen_ai.completion.{N}.role/.content
    (assistant → AssistantText).
  - Tool spans: emits paired tool_use + tool_result from
    gen_ai.tool.name / .call.id / .call.arguments / .call.result.
  - Usage + model attach to the FIRST step emitted from each span, same
    convention as the other parsers — no double-counting in corpus sums.
  - Chronological across ResourceSpans / ScopeSpans boundaries: all
    spans sorted by startTimeUnixNano before emission.
  - Non-GenAI spans (generic HTTP / DB) are silently ignored so agx can
    coexist with mixed traces.
- *(phase2.2)* Binary OTLP (.pb / .otlp) parser behind otel-proto feature
Adds agx's sixth supported format. OTLP-protobuf is what
  `opentelemetry-collector` and OTLP/HTTP endpoints emit natively;
  previously users had to convert to JSON first. Phase 2.1 covered the
  JSON side; 2.2 closes the binary side.
- *(phase2.3)* LangChain / LangSmith run-tree export parser
Adds agx's seventh supported format. LangSmith's "Export run" button
  produces a single-JSON run tree that users commonly ship in bug
  reports; LangChain's tracer emits the same shape when dumping a full
  session. Parsing it natively unlocks the mass of framework-level
  LangChain workloads that don't route through OTel.

  Implementation (src/langchain.rs):
  - `Run` struct with serde defaults on every field so the fluid
    LangSmith schema doesn't error on missing values. Runs form a tree
    via `child_runs`; collect_runs flattens it, then we sort by
    start_time for chronological emission (child_runs order is not
    reliably chronological when a parent awaits multiple children).
  - Root user turn extracted once from `inputs.input` / `.question` /
    `.query` / `.prompt` with fallback to first human message in
    `inputs.messages[0]`. Inner chat_models carry the same user content
    alongside prior tool turns — emitting from there would duplicate.
  - `chat_model` / `llm` runs → assistant text from
    `outputs.generations[0][0].message.data.content` plus tool_use
    steps from the same message's `tool_calls[]` (modern LangChain
    tool-calling shape).
  - `tool` runs → paired `tool_result` only when the immediately prior
    step is a matching tool_use (same tool_name); otherwise both
    tool_use + tool_result so standalone tool runs stay visible.
    Prevents double-counting in the normal agent flow.
  - Token usage from `outputs.llm_output.token_usage` with
    `prompt_tokens`/`input_tokens` + `completion_tokens`/`output_tokens`
    fallback pairs. Model from `outputs.llm_output.model_name` with
    `extra.invocation_params.model_name|model` fallback. Usage attaches
    to the FIRST step from each chat_model run — same anchor
    convention every other agx parser uses.
  - `chain` / `retriever` / `parser` inner runs skipped (no render);
    agx walks into their children without emitting wrapper steps.
- *(phase2.4)* Vercel AI SDK generateText/streamText result parser
Adds agx's eighth supported format. The AI SDK has OTel telemetry
  (covered by otel_json / otel_proto), but most users just serialize
  the raw GenerateTextResult / StreamTextResult. That shape is
  idiosyncratic enough — camelCase tool fields, nested steps[],
  finishReason at top — that the Generic OpenAI-compatible parser
  loses fidelity on it.

  Implementation (src/vercel_ai.rs):
  - Walks `steps[]` array when present (multi-step agent loops), per-step
    usage + model attach to each step's first emitted timeline row.
    Absent `steps` → treat root as single implicit step (covers the
    plain single-turn generateText shape).
  - User-prompt extraction: `prompt` string → `messages[0]` with
    role=user → content-as-string / content-array-parts / message-level
    `parts` (v5 UI idiom).
  - Tool calls: `toolCallId` + `toolName` + `args` as a JSON object (not
    a serialized string the way OpenAI does it). Tool results: same
    fields plus `result` (string or object, pretty-printed when object).
  - Usage handles both v4 (`promptTokens` / `completionTokens`) and v5+
    (`inputTokens` / `outputTokens`) naming plus cache counters
    (`cachedInputTokens`, `cacheCreationInputTokens`). All-zero usage
    blocks are treated as "no LLM call on this step" — AI SDK emits
    them on tool-result-only steps and attaching would sprout
    misleading zero-token rows.
  - Model from `response.modelId` per step with root-level fallback for
    single-step cases. Usage has NO root-level fallback — root usage is
    an aggregate of all steps and would double-count at the corpus
    level. This was the one bug I hit writing the tests and it's now
    encoded as the "step-level ONLY for usage" rule.

  Detection (src/format.rs):
  - New `is_vercel_ai` helper checks three independent signals:
    `finishReason` at top level, `steps[0].stepType` present, or
    camelCase `toolCalls[0].toolCallId` + `toolName`. Any one triggers.
  - Probed before Generic so Vercel wins on its specific markers while
    plain `{messages: [{role, content}]}` still falls through to Generic.
  - 4 new detection unit tests covering all three signals plus the
    fall-through case.

  Integration (src/main.rs, src/browser.rs, src/debug_unknowns.rs):
  - Format::VercelAi variant, Display label "Vercel AI SDK".
  - main.rs dispatches to vercel_ai::load.
  - browser tag [Vercel].
  - Drift scanner reports unknown `steps[].stepType` values (known:
    initial, continue, tool-result).
  - summary_test.rs: integration test for the format label + fixture.

  Fixture (assets/sample_vercel_ai_session.json):
  - Three-step agent: chat with tool_call → tool-result-only step with
    zero usage → continue step with final answer. 5 timeline steps end
    to end, $0.0053 estimated cost. Exercises every code path: user
    extraction from messages, multi-step walking, zero-usage
    suppression, per-step usage anchor, tool-call/tool-result pairing.
- *(phase2.5,2.6)* Close out Phase 2 — LlamaIndex/Pydantic AI deferred, detection order documented
Phase 2.5 — LlamaIndex + Pydantic AI:
  No new parser. Inventory pass confirmed every mainstream
  LlamaIndex and Pydantic AI instrumentation exports OTel GenAI by
  default (openinference callbacks, arize-phoenix, Traceloop /
  OpenLLMetry, logfire). agx's existing otel_json / otel_proto paths
  cover them without a native parser. Native modules deferred to
  Phase 8 long-tail if a user contributes a non-OTel fixture. Decision
  written into the roadmap so we don't re-litigate it without evidence.

  Phase 2.6 — Detection order:
  - Full probe sequence documented as a docstring on format::detect.
    The "most specific first" rule is called out with Vercel (which
    also has `messages[]`) as the illustrative case.
  - Three new unit tests fill the last detection-coverage gaps:
      * detects_otel_json_by_resource_spans_key
      * detects_generic_by_bare_messages_only
      * langchain_requires_inputs_or_outputs_alongside_run_type
    Previously missing: there was no direct coverage that a bare
    `{messages: [...]}` (no Vercel markers, no LangChain markers) falls
    through cleanly to Generic, and no test that `run_type` alone (no
    inputs/outputs) doesn't misroute to Langchain.
  - 15 format-detection tests now cover every disambiguator across all
    8 formats.

  Doc linting: the original ASCII-art indented list with aligned
  arrows triggered clippy's `doc_overindented_list_items` — continuation
  whitespace confused the parser. Switched to a flat bullet list. No
  information loss.

  Test state: 200 unit + 1 corpus + 11 integration = 212 passing
  (feature off). cargo clippy --all-targets -- -D warnings clean.
  cargo fmt clean.

  Marks Phase 2 (v0.3 OTel + Framework Traces) complete. Next up is
  Phase 3 — Corpus Analysis + Performance.
- *(phase3.1)* Agx corpus <dir> subcommand for cross-session analytics
First piece of Phase 3. Adds a subcommand that walks a directory
  tree, loads every session file in parallel, and aggregates stats
  across them — totals, per-model/tool/format breakdowns, and a
  filter pipeline. The existing `agx <file>` flow is untouched; clap's
  optional subcommand handles both shapes.

  Implementation (src/corpus.rs + src/loader.rs + src/main.rs):
  - `load_session` factored out of main.rs into a new `src/loader.rs`
    module so single-session and corpus flows share one format-dispatch
    entry point.
  - `agx corpus <dir>` is a new `clap::Subcommand` variant, not a
    top-level flag. `agx corpus <dir> --filter model=X --filter tool=Y
    --json --no-cost --max-depth 8` reads naturally in docs.
  - Recursive directory walk is stdlib-only (`std::fs::read_dir` +
    depth limit); no `walkdir` dep. Default depth 8 reaches every
    format's canonical storage layout.
  - Parallel parse via `rayon` (new always-on dep — three small
    pure-Rust crates: rayon-core + two crossbeam helpers). Per the
    roadmap's "earn your place" rule: corpus is the flagship use case
    and rayon's weight is modest. `AGX_CORPUS_SERIAL=1` env var forces
    serial loading for debugging.
  - Non-session files are silently skipped via a sentinel-error
    pattern (detect failure → `AGX_SKIP: ...`; corpus matches that
    prefix and drops the result). Detection-succeeds-but-load-fails
    still surfaces as a real parse error so format drift is visible.
  - Special case: binary files routed to OtelProto at detection time
    when the `otel-proto` feature is off are also silently skipped —
    they're almost certainly unrelated binaries (images, PDFs,
    archives) rather than real OTLP protobuf exports the user forgot
    to compile support for.

  Aggregation (CorpusStats):
  - File count, parse success / error / filtered counts, total steps,
    tokens (in/out/cache_read/cache_create), total cost.
  - Per-model bucket: session count, tokens, cost — sorted by
    session count desc, alphabetic tie-break.
  - Per-tool bucket: use count, error count — same ordering.
  - Per-format bucket: session count — same ordering.
  - All orderings are stable and reproducible.

  Filter pipeline:
  - `Filter::parse` accepts `model=<name>`, `tool=<name>`, or the bare
    keyword `errored`. Whitespace-tolerant. Rejects unknown keys with
    a clear message.
  - Multiple `--filter` flags AND-combine. Applied after per-session
    parse so predicates can look at observed content (tool_stats,
    unique_models, error_count).
- *(phase3.2)* JSONL line-streaming + --bench timing flag
First slice of the Phase 3 performance pass. No behavior change —
  same outputs, same tests — but peak memory during JSONL parsing
  drops from O(file size) to O(longest line).

  Line-streaming (src/session.rs, src/codex.rs):
  - Both JSONL parsers replaced `std::fs::read_to_string(path)` + a
    string-backed `.lines()` walk with `BufReader::lines()` over a
    `File`. The old path materialized the full file as a single
    `String` just to iterate over it — pure waste on multi-MB sessions.
    A real Claude Code session can be 50MB+ of JSONL; Codex rollouts
    regularly exceed 10MB. The new path keeps working set bounded by
    the longest single line (typically a few KB).
  - Line-number context preserved in error messages. `BufReader::lines`
    returns `Result<String, IoError>` per line; we keep the
    `.enumerate()` index and plumb it into both the IO-error and
    serde-error `with_context` calls so format-drift reports still
    say "parsing line N of codex session".
  - Gemini / Generic / LangChain / Vercel / OTel-JSON parsers
    untouched — those formats are single-JSON-object files where
    streaming gains nothing (and `serde_json::from_str` wants a
    complete string).

  Bench flag (src/main.rs, src/corpus.rs):
  - New hidden `--bench` flag on both the top-level CLI and `agx
    corpus`. Writes timing breakdowns to stderr so stdout stays
    pipeable (important for --summary / --export / --json).
  - Single-session: `[bench] load: 1.09ms (11 steps)` — measures the
    full `load_session` call.
  - Corpus: `[bench] walk: 0.11ms (9 files)  load: 1.22ms (7 parsed,
    0 errored)  aggregate: 0.01ms  total: 1.34ms` — three-phase
    breakdown so users can file specific regression reports.
  - Flag is `hide = true` in clap; diagnostic tool, not a user-facing
    feature.
- *(phase3.3)* Interactive corpus TUI — agx corpus --tui <dir>
Builds on Phase 3.1's corpus aggregation and adds an interactive
  two-pane browser on top. Session list (left) + selected-session
  summary (right) + cyan header bar with corpus totals + gray footer
  with key hints. Enter drills into the existing per-session
  step-through TUI; Esc/q returns.

  Implementation (src/corpus_tui.rs):
  - New module with its own `App` struct, event loop, and
    `TerminalGuard`. Raw mode is owned per-TUI (corpus or per-session)
    because it's process-global and not stackable — the outer loop in
    `run()` tears the corpus TUI down before calling `tui::run`, then
    re-enters the corpus view after the per-session TUI exits.
  - Sort cycle via `s`: mtime ↓ → cost ↓ → errors ↓ → tokens ↓ →
    format/name → (wrap). Current mode labelled in the header. Selected
    session's identity is preserved across re-sorts — the cursor tracks
    the session object, not the row index. Less jarring than
    "selection snaps to row 0 every time you press s".
  - Keybindings mirror the per-session TUI: j/k/g/G/Home/End/PgUp/PgDn
    navigation, ?/F1 help, q/Esc quit. Plus two corpus-specific
    additions: Enter (drill-in) and s (cycle sort).
  - Detail pane shows path, format, relative mtime, step count, token
    breakdown (including cache read/create when present), cost,
    unique models, and top 8 tools by use count (with error badges).

  Integration (src/corpus.rs, src/main.rs):
  - `CorpusArgs` gets a `tui: bool`. Clap-level `conflicts_with =
    "json"` prevents the nonsensical `--tui --json` combo (TUI owns
    the terminal, JSON needs stdout clean).
  - `corpus::run` dispatches to `corpus_tui::run` when `tui` is set,
    otherwise keeps the text / JSON branches intact.
  - `ParsedSession` gains `mtime_secs: Option<u64>`, populated from
    `fs::metadata` during parallel load. Used for the default
    mtime-desc sort. Also makes the future per-session drill-down
    display more informative ("2h ago" instead of a bare UUID).

  Drive-by fix (src/browser.rs):
  - `files.sort_by(|a, b| b.x.cmp(&a.x))` → `sort_by_key(|f|
    Reverse(f.mtime_secs))`. New clippy (1.95.0) flagged the old
    pattern as `unnecessary_sort_by`; swapping to `Reverse` is the
    recommended idiom and behaviorally identical.
- *(phase3.4)* --jsonl streaming + --fail-on-errored CI gate
Closes Phase 3's core subplans. Two flags that make `agx corpus`
  pipeable into eval / CI infrastructure.

  --jsonl (src/corpus.rs):
  - Emits one line of compact JSON per successfully-parsed session to
    stdout. Parse errors go to stderr so `agx corpus --jsonl foo/ | jq`
    sees a clean line-delimited stream.
  - Dedicated `SessionLine` serialization struct with a flat stable
    schema: path, format, step_count, tokens_{in,out}, cache_read,
    cache_create, cost_usd, models, tool_counts (array of {name,
    use_count, error_count}), error_count, mtime_secs. No transitive
    Serialize propagation through ParsedSession / Format / ToolStats
    — isolated so the wire format doesn't drift when those types
    evolve.
  - `--jsonl` conflicts_with `--json` at the clap level (and both
    conflict with `--tui`) so nonsensical combos are rejected at
    parse time.
  - Chosen `--jsonl` over the roadmap's original `--json-lines` to
    match the file-extension convention and for terser `jq` recipes.

  --fail-on-errored (src/corpus.rs):
  - Exits nonzero when any parse error OR any is_error_result
    tool_result is present across the corpus.
  - Orthogonal to rendering: combines cleanly with --json / --jsonl /
    --tui / default text. Evaluated before the rendering branch so
    the TUI path (which takes `parsed` by value) doesn't block it.
  - Exit code 1 via anyhow's normal error path. Considered a
    dedicated code 2 to distinguish from other failure modes but the
    user-facing benefit didn't justify bypassing anyhow's reporting.

  Verified end-to-end on `assets/`:
    $ ./target/release/agx corpus --jsonl assets/ | head -1 | jq -c
    {"path":"assets/sample_session.jsonl","format":"Claude Code",
     "step_count":11,"tokens_in":740,"tokens_out":345,
     "cache_read":6810,"cache_create":1500,"cost_usd":0.075315,
     "models":["claude-opus-4-6"],"tool_counts":[...],
     "error_count":0,"mtime_secs":1776266231}
    $ ./target/release/agx corpus --fail-on-errored assets/
    ... (normal summary)
    exit: 0

  Test state: 223 unit + 1 corpus + 11 integration = 235 tests
  passing. cargo clippy --all-targets -- -D warnings clean. cargo
  fmt clean.

  Phase 3 status: core subplans (3.1 corpus command, 3.3 corpus TUI,
  3.4 eval integration) all shipped. Phase 3.2's remaining perf items
  (criterion benches behind a src/lib.rs shim, tool-name interning,
  lazy detail expansion, memory-ceiling regression test) stay on the
  roadmap for dedicated perf-focused commits.

  Deferred within 3.4 (tracked in ROADMAP): true streaming during
  parallel parse via channel + print thread, so `tail -f` on --jsonl
  output actually shows lines as parses complete. Current impl
  collect-then-print is fine for small-to-medium corpora; upgrade if
  users ask.
- *(phase4.1)* LCS-based session alignment (part 1 of interactive diff)
Ships the pure-algorithm half of Phase 4.1 as a standalone slice so
  the TUI work in the follow-up commit has a well-tested alignment
  kernel to sit on. No user-visible feature yet — `src/diff_align.rs`
  is registered in main.rs but gated with `#[allow(dead_code)]` until
  the TUI that consumes it lands.

  Implementation (src/diff_align.rs):
  - Structural signature per step: `(kind, tool_name)`. Ignores
    content on purpose — real agent sessions doing "the same thing"
    typically paraphrase assistant text and tweak tool inputs, so
    matching on content makes alignment brittle. Structure is the
    signal worth surfacing.
  - Standard LCS DP (O(N·M) time and space) over the signature
    sequences, followed by a backtrack that emits aligned index pairs
    in monotonically increasing order on both sides.
  - A `weave` pass interleaves the LCS pairs with one-sided rows so
    the output is a linear Vec<AlignRow> the TUI can render as a
    two-pane list. Trailing unmatched rows on either side get emitted
    as LeftOnly / RightOnly.
  - AlignKind::{Match, Differ, LeftOnly, RightOnly}. Match vs Differ
    is decided per aligned pair by comparing detail strings — so two
    assistant messages with the same structure but different wording
    show up as Differ and the TUI can color them yellow.
- *(phase4.1)* --diff-tui two-pane TUI (part 2 of 2)
Closes Phase 4.1. Consumes the alignment kernel from the previous
  commit and renders it as an interactive two-pane TUI.

  TUI (src/diff_tui.rs):
  - Two ratatui `List` panes sharing a single `ListState` across both
    `render_stateful_widget` calls. Because the panes are the same
    height (horizontal split of a single vertical slot), ratatui's
    "keep selected visible" offset math produces identical offsets on
    both sides — the panes scroll in lockstep for free with no manual
    top/height bookkeeping. Simple and robust.
  - Row prefixes are ASCII only (terminal-native principle): `= `
    match (green), `~ ` differ (yellow), `- ` only-A (red on left,
    gray "(absent)" on right), `+ ` only-B (green on right, gray
    "(absent)" on left).
  - Header: both file labels with format + token totals + cost,
    plus alignment counts (N match · N differ · N only-A · N only-B).
    Cyan bg for visibility.
  - Footer: compact key-hints strip.
  - Navigation mirrors per-session + corpus TUIs: j/k/g/G/Home/End/
    PgUp/PgDn. ?/F1 opens help overlay with color legend. q/Esc quits.
  - Raw-mode lifecycle via a module-local `TerminalGuard` — same
    pattern as tui.rs and corpus_tui.rs. Each TUI owns its raw mode
    because raw mode is process-global, not stackable.

  CLI wiring (src/main.rs):
  - New `--diff-tui` bool flag on top-level Cli. `requires = "diff"`
    enforces that `--diff <path>` must be set. `conflicts_with_all =
    ["summary", "export"]` because those flags own stdout and can't
    coexist with an alt-screen TUI.
  - Dispatch: when `--diff-tui` is set alongside `--diff`, we detect
    formats for both sides (for the header labels), then call
    `diff_tui::run(&steps_a, &steps_b, ...)` instead of printing the
    text summary. All existing `--diff` behavior unchanged when
    `--diff-tui` is absent.
  - The `#[allow(dead_code)]` on `mod diff_align` from the previous
    commit is removed — the TUI consumes the module now.
- *(phase4.2)* Slicing flags + :@duration jump command
Closes Phase 4.2. Adds a shared slice module (`src/slice.rs`) used
  by both the CLI (`--after`/`--before`/`--after-step`/`--before-step`/
  `--range`) and the TUI (`:@<duration>` jump command), so the two
  paths speak the same grammar.

  Module (src/slice.rs):
  - `parse_duration_ms` accepts `30s` / `5m` / `2h` / `1d`, compound
    forms `1h30m` / `90m30s`, long-form units (`minutes`, `hours`),
    case-insensitive, and a bare integer as seconds (`300` = 5m).
    Rejects empty input, unknown units, floats, and overflow.
  - `parse_step_range` parses `start..end` (exclusive end, mirrors
    Rust's `Range<usize>`) with open-ended forms (`..500`, `100..`,
    `..`). Rejects reversed ranges and non-range input at parse time.
  - `StepRange::contains` and `slice_steps` apply both index and time
    filters in a single filter pass. Time bounds are offsets in
    milliseconds from the session's first-step timestamp.
  - `warn_if_time_filter_ignored` keeps the core pure — stderr warning
    when time filters were requested but the session has no
    timestamps.
  - 16 unit tests cover all of the above (5 duration edge cases, 6
    range forms, 5 slice scenarios including empty / identity /
    no-timestamps no-op paths).

  Semantics decision — `--after 2h` / `--before 10m` are relative to
  the session's *first step*, not wall-clock `now()`. This is
  unambiguous for archived sessions where "now" is meaningless, and
  matches the intuitive read of "give me what happened in the first
  10 minutes of the session". Documented at the top of slice.rs.

  CLI wiring (src/main.rs):
  - Top-level `--after`, `--before`, `--after-step`, `--before-step`,
    `--range` flags. Clap-level `conflicts_with = "range"` on the step
    scalars prevents confusing combinations (range shorthand subsumes
    the two scalars). Applied after `load_session` and before the
    render-branch (summary / TUI / export / diff).
  - `--bench` reports `slice: N → M steps` when slicing changed the
    count, giving users a timing-adjacent signal for how much the
    filter trimmed.
  - `warn_if_time_filter_ignored` fires before slicing so the warning
    is visible next to the rest of the stderr diagnostics.

  TUI extension (src/tui.rs):
  - `execute_command` now handles `:@<duration>` in addition to `:N`.
    Uses `slice::parse_duration_ms` so the grammar is identical to
    the CLI `--after` / `--before` flags.
  - `goto_time_offset` finds the first step whose timestamp is at
    least `offset_ms` past the session start. Reports clearly when
    there are no step timestamps, when no step matches the offset,
    and when the matched step is hidden by the active filter.
  - Help overlay gets a new line documenting `:@<duration>` with
    examples.
  - 4 new unit tests cover the command-mode parsing, the
    no-timestamps path, the past-end path, and malformed-duration
    rejection.

  End-to-end verified on `assets/sample_session.jsonl`:
    $ agx --summary --range 2..6 ...       # 4 steps
    $ agx --summary --after 3s ...         # 7 steps
    $ agx --summary --after 10h ...        # 0 steps (past end)

  Test state: 261 unit + 1 corpus + 11 integration = 273 tests
  passing. cargo clippy --all-targets -- -D warnings clean.
  cargo fmt clean.

  Deferred (tracked in ROADMAP): absolute-time `:@HH:MM:SS` jump
  (ambiguous across days; needs either a date prefix or a
  day-of-session heuristic), `..=` inclusive-end range syntax
  (trivial when asked for).
- *(phase4.3)* Per-step annotations with persistent storage
First persistent write-back feature. The `a` keybinding in the TUI
  opens an annotation prompt for the selected step; Enter saves to
  disk, empty text deletes, Esc discards. Annotated rows get a
  magenta `*` prefix in the list; the detail pane prepends a
  `[note: ...]` meta line.

  Module (src/annotations.rs):
  - Storage: `~/.agx/notes/<session-stem>-<fnv1a-hash8>.json`, keyed
    by FNV-1a of the canonical session path. Hand-rolled FNV keeps
    hashes deterministic across agx invocations (std's hashmap hasher
    has a random seed) and adds zero deps.
  - File format v1: `{version, path, notes: {step_idx: {text,
    created_at_ms, updated_at_ms}}}`. String keys because JSON
    requires it; numeric sort in `iter()` (BTreeMap iterates strings
    lexicographically, which would put "12" before "2").
  - Atomic writes: serialize to `<dest>.json.tmp` then rename. POSIX
    rename is atomic on the same filesystem; partial writes never
    corrupt an existing notes file.
  - Reads are fault-tolerant. Missing file → empty `Annotations`,
    not an error — the common case for sessions the user hasn't
    annotated yet. Malformed file → stderr warning + empty set, so
    one bad file doesn't block the TUI.
  - `set(idx, text)` trims whitespace, treats empty text as delete,
    refreshes `updated_at_ms` on edit, returns true on change.
  - Deliberately chose a single storage location over the
    roadmap's sibling `.agx/` + home-dir-fallback scheme. Simpler
    retrieval; portable across workstations where session dirs are
    read-only or cross-mounted. Override via `AGX_HOME` (used by
    tests).
  - Keyed by canonical path rather than session-file UUID since UUID
    extraction varies per format and isn't available for all of
    them. Renames start fresh — acceptable trade-off for MVP.

  TUI integration (src/tui.rs):
  - New `InputMode::Annotation { step_idx, buffer }` variant. The
    existing input-mode rendering / Enter / Backspace / Char
    handlers extend to cover it.
  - `a` in normal mode calls `enter_annotation_mode`, which prefills
    the buffer with the existing note (edit-in-place) or opens
    blank.
  - Enter in Annotation mode calls `save_annotation` which upserts
    via the module and persists to disk. Errors surface in the
    status bar rather than panicking.
  - List prefix is now a 2-char slot that prioritizes `*` (annotated)
    over `║` (batched). Annotations are more user-signal than derived
    structure, so they win.
  - Detail pane prepends `[note: ...]` to the meta block when the
    selected step has one.
  - Help overlay gets the `a` line and a color-legend entry.

  Wiring (src/main.rs, src/corpus_tui.rs):
  - `tui::run` gains a `session_path: Option<&Path>` argument. When
    set, annotations are loaded on entry and the App knows where to
    save. `None` disables persistence (for tests).
  - main.rs passes the session path through; corpus_tui drill-in
    passes the drilled path.
- *(phase4.3)* Annotation A-overlay, export integration, corpus filter
Completes the Phase 4.3 follow-ups. Annotations now participate in every
  read path agx exposes:

  TUI — `A` opens a list overlay of every note (step label + preview),
  with j/k to navigate and Enter to jump the main cursor. Filter-hidden
  targets report a status message instead of moving somewhere unexpected.

  Exports — `--export md|html|json` all surface notes when present.
  Markdown gets a `> **note**: …` blockquote per step (multi-line notes
  keep the `> ` prefix per line). HTML renders a magenta-bordered
  `<div class="note">` below the meta row with full HTML-escaping of
  the text. JSON adds an optional top-level `annotations` array of
  `{step_index, text, created_at_ms, updated_at_ms}`. The field is
  omitted entirely when the session has no notes to keep the common-case
  output small.

  Corpus — `--filter annotated` keeps sessions with ≥1 note.
  `ParsedSession.annotation_count` is loaded eagerly during the parallel
  scan (one small disk read per session via `Annotations::load_for`) and
  appears on `--jsonl` per-session output for downstream tooling.

  296 tests pass; clippy + fmt clean. No new dependencies.
- *(phase4.4)* Semantic search via //query prefix (opt-in feature)
Closes the last Phase 4 subplan. When built with --features
  embedding-search, a leading `//` in the TUI search prompt routes the
  query through fastembed (MiniLM-L6-v2) → cosine similarity against
  each step's label+detail → top 30 matches above threshold 0.25 flow
  into the existing `search_matches` vec so highlight + `n` / `N`
  navigation work unchanged.

  Without the feature, `//query` shows a status-bar message telling
  users exactly how to rebuild agx. No fastembed deps are compiled in
  the default build — verified binary size 2.6MB (budget <5MB).
- *(phase3.2)* Src/lib.rs shim + criterion bench suite
Closes two of three Phase 3.2 remnants. agx now has a proper library
  layer that external consumers (benches, future agx-core split, any
  integration test or tool) can import directly, plus a criterion
  bench file covering the three hot paths users care about.

  Library shim (src/lib.rs):
  - All 23 modules moved to `pub mod X` in lib.rs. main.rs is now a
    thin `use agx::{...}` shell — no more `mod X` declarations. Two
    in-place references to `crate::timeline::ToolStats` rewritten as
    `agx::timeline::ToolStats`.
  - Stability note: the library surface is NOT yet a public API;
    expect breaking changes between 0.x. Phase 7 formalizes a stable
    subset when the library split happens.

  Criterion bench (benches/agx_bench.rs):
  - `bench_load`: per-format parse throughput in MiB/s across all
    seven fixtures (claude_code / codex / gemini / generic /
    langchain / otel_json / vercel_ai). Measured 25-77 MiB/s on
    macOS-arm64 at ship time.
  - `bench_aggregate`: `compute_session_totals` and
    `compute_tool_stats` at N=100 / 1k / 10k steps to surface any
    O(N^2) regression in things like `unique_models` dedup.
  - `bench_corpus`: end-to-end `agx corpus assets/` covering file
    discovery + parallel parse + aggregate.
  - Missing fixtures warn on stderr rather than aborting the suite,
    so partial-repo checkouts still run usable benches.

  Workflow (added to CLAUDE.md quick reference):
    cargo bench --bench agx_bench
    cargo bench --bench agx_bench -- --save-baseline main
    cargo bench --bench agx_bench -- --baseline main

  Deferred from 3.2:
  - Tool-name interning + lazy Step.detail expansion — crosses the
    TUI/parser boundary, scheduled for its own commit. The bench
    baseline now in place will quantify the gain.
  - Cross-platform memory-ceiling regression test — RSS measurement
    is fragile across macOS / Linux / Windows CI; revisit after the
    interning + lazy-detail changes ship.
- *(phase5.5)* --jump-to <N> for stepwise Timeline-jump integration
Ships the CLI surface sift's `t`-keybind Timeline-jump targets per
  docs/suite-conventions.md §5. Sift now has a first-class public
  contract to drop into the turn context an agent's write came from
  without asking the user to type `:N` manually.
- *(phase5.3)* --notify-on-error / --notify-on-idle for live mode
Closes another Phase 5 subplan. The audience is eval engineers and
  long-running unattended agent runs — people who start a session, walk
  away, and want a desktop notification when something interesting
  happens without having to keep agx in view.

  Flags (both `requires = "live"` at clap level — meaningless without
  the reload loop):

  - `--notify-on-error`: fires one OS notification per newly-arrived
    `tool_result` that matches the existing `is_error_result` heuristic.
    Error-scan snapshots newly-added steps *before* `reload_steps()`
    moves the vec, so the diff stays accurate.
  - `--notify-on-idle <DURATION>`: fires when the session hasn't grown
    for the given duration. Duration grammar reuses
    `slice::parse_duration_ms` (`30s` / `5m` / `1h`, compounds, bare-
    int seconds). Latched — fires at most once per idle interval,
    resets on growth.
- *(phase5.1)* Claude Code branch / fork detection + TUI overlay
Closes the last of the Phase 5 subplans that doesn't require ecosystem
  adoption (5.2 MCP) or experimental-flag design (5.4 replay). Useful
  for Claude Code users who do edit/resume flows — the resulting branch
  structure is now visible as a first-class concept rather than having
  to guess from the conversation shape.
- *(phase6.1)* --export trajectory-openai + --redact flag
Ships the most load-bearing trajectory format (OpenAI fine-tuning
  JSONL) and the dataset-prep flag (`--redact`) from Phase 6.1. Serves
  the RL / eval-engineer audience that Phase 6 exists for: take a
  Claude Code / Codex / Gemini / OTel / Vercel / LangChain session,
  strip any secrets that crept into tool outputs, export it as JSONL
  that OpenAI's fine-tuning / batch endpoints accept verbatim.

  New CLI:
  - `--export trajectory-openai` — one JSONL line per session:
    `{messages: [{role, content, tool_calls?, tool_call_id?}]}`.
    UserText → `user`, AssistantText → `assistant`, ToolUse →
    `assistant` with `tool_calls[]`, ToolResult → `tool` with a
    matching `tool_call_id`.
  - `--redact <NEEDLE>` — repeatable literal-substring mask. Every
    occurrence across step label and detail becomes `[REDACTED]`.
    Applies to every export format (md / html / json /
    trajectory-openai). Empty needles are skipped (prevents
    `--redact ''` from nuking the output).

  Model change:
  - `Step.tool_call_id: Option<String>` added. `tool_use_step` and
    `tool_result_step` set it from their `id` argument. Every format
    parser already passes an `id` through, so this is a zero-code-
    change benefit for all 8 parsers. `#[serde(skip_serializing_if = "Option::is_none")]`
    keeps the existing JSON export lean for non-tool steps.
- *(phase6.2)* --trajectory-stats + --sample for corpus-level inspection
Ships the dataset-prep pair from Phase 6.2. Researchers running the
  corpus subcommand over a dataset directory now get distributional
  signal (percentiles + rates) in one shot rather than having to pipe
  --jsonl into jq + manually compute percentiles.

  New CLI on `agx corpus`:
  - `--trajectory-stats`: replaces the default aggregate text with a
    distributional breakdown. Per-session min/p50/p90/p99/max/mean/total
    for steps, tool-calls, tokens-in, tokens-out. Branched / annotated /
    errored *rates* (fraction of sessions). Nearest-rank percentiles —
    matches numpy's "lower" interpolation on integer inputs.
  - `--sample N`: keeps the N most-recent-by-mtime sessions after filter
    application. Deterministic, reproducible. `--filter model=X --sample
    20` gives the 20 newest X-model sessions — the concrete spot-check
    workflow.

  Mode composition:
  - `--trajectory-stats --json` emits the stats blob as pretty JSON.
  - `--trajectory-stats --jsonl` writes per-session JSONL to stdout
    (unchanged) and the stats blob to stderr so a pipeline can tee
    both streams.
- *(phase6.4)* --scan-pii credential / PII heuristic scanner
Ships the dataset-prep safety scanner from Phase 6.4. The workflow
  pairs with the Phase 6.1 `--redact` flag:

      agx --scan-pii session.jsonl   # find what's there
      agx --export trajectory-openai --redact '<needle>' session.jsonl   # scrub
      agx --scan-pii session.jsonl   # re-check after redaction

  Coverage (all via prefix-based byte scans — no `regex` dep):
  - AWS access keys: AKIA* / ASIA*
  - Stripe: sk_live_ / sk_test_ / pk_live_ / pk_test_
  - GitHub tokens: ghp_ / gho_ / ghu_ / ghs_ / ghr_
  - OpenAI: sk- (excluding sk-ant- via disambiguation)
  - Anthropic: sk-ant-
  - SSH private-key PEM headers: OPENSSH / RSA / DSA / EC / PRIVATE
  - JWT: eyJ + 3 base64url groups of ≥16 chars joined by `.`
  - Emails: local@domain.tld with a real TLD (heuristic — rejects
    `@handle` without a domain dot)
  - IPv4: 4 dotted octets with per-octet 0-255 range validation
    (rejects 999.1.1.1 and trailing-digit artifacts like 12.34.56.789)
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
- *(phase7.2)* Agx-py Python bindings (pyo3 + maturin scaffold)
Ships the Python-bindings scaffold from Phase 7.2. Builds on the
  Phase 7.1 workspace split — agx-py depends on agx-core, pulls no
  TUI deps, targets abi3-py310 for cross-version wheel compat.

  New crate: `crates/agx-py/`.

  Python surface:
  - `agx.load(path) -> list[dict]` — one dict per step, field names
    mirror the stable JSON schema from docs/eval-integration.md.
  - `agx.load_corpus(dir) -> list[dict]` — per-session aggregate
    dicts (format, step_count, totals, tool_stats, fork_root_count,
    annotation_count, mtime_secs). Parse errors come back at the end
    as `{path, error}` dicts so downstream code distinguishes "not
    a session" from "bad session."
  - `agx.scan_pii(text) -> list[dict]` — Phase 6.4 scanner over
    arbitrary strings, yielding `{category, step_index, snippet}`.
  - `agx.__version__` — tracks Cargo.toml version.

  Build shape:
  - `crate-type = ["cdylib"]`, module name `agx` → `import agx`.
  - `pyo3 = { features = ["extension-module", "abi3-py310"] }` — one
    wheel for Python ≥ 3.10 across linux / macos / windows.
  - maturin config in `pyproject.toml`. One-command wheel:
    `cd crates/agx-py && maturin build --release`.
  - `publish = false` on Cargo.toml — ships via PyPI, not crates.io.

  Workspace integration:
  - Added to `[workspace].members` but excluded from `default-members`,
    so `cargo build` / `cargo test` at repo root don't compile pyo3.
    The dev loop stays fast and Python-toolchain-free for contributors
    who only touch the bin crate. Use `cargo check -p agx-py` or
    `maturin develop` to opt in.

  Implementation choices:
  - JSON-bridge conversion (`json_to_py`): agx-core types that are
    `Serialize` (Step, etc.) cross into Python via `serde_json::to_value`
    + a hand-rolled `Value` → `PyAny` walker. Keeps the bridge schema
    honest — whatever `--export json` shows, Python sees the same keys.
  - `ParsedSession` / `ToolStats` / `Format` aren't Serialize
    (didn't need to be in the CLI path), so `load_corpus` hand-builds
    their dicts field by field. A future commit could derive
    Serialize on them for symmetry; leaving it be for now so the
    agx-core public surface doesn't grow ahead of demand.
- *(phase7.3)* Agx-wasm bindings (wasm-bindgen scaffold)
Ships the WebAssembly / TypeScript bindings scaffold from Phase 7.3.
  Mirrors the Phase 7.2 agx-py surface one-to-one so the same stable
  JSON schema flows through three distribution channels:

      Rust CLI  →  agx --export json
      Python    →  agx.load(path)        -> list[dict]
      JS/WASM   →  load(filename, bytes) -> Step[]

  All three return the same field shape documented in
  docs/eval-integration.md. The contract is now cross-language.

  New crate: `crates/agx-wasm/`.

  JS surface (identical shape to agx-py):
  - `init()` — `#[wasm_bindgen(start)]`, installs a panic hook routing
    Rust panics to the browser console. One-time; no-op on repeat.
  - `load(filename, bytes) -> Step[]` — bytes-in to match browser /
    Node / Deno I/O shapes (no wasi filesystem shim).
  - `scan_pii(text) -> Match[]` — Phase 6.4 scanner over free text,
    returns `{category, step_index, snippet}` objects.
  - `version() -> string` — package version.

  Build (wasm-pack):
    cd crates/agx-wasm
    wasm-pack build --target web
    wasm-pack build --target nodejs
    wasm-pack build --target bundler

  `crate-type = ["cdylib", "rlib"]` means `cargo check -p agx-wasm` works
  on native targets (useful for local hacking / CI lint), and the
  browser / Node artifacts come from the cdylib on
  `wasm32-unknown-unknown`.

  Workspace integration:
  - Added to `[workspace].members` but excluded from `default-members`,
    same as agx-py. Root-level `cargo build` / `cargo test` skip it.
    Explicit `-p agx-wasm` or `wasm-pack build` to opt in.

  Implementation choices:
  - **Bytes, not paths.** WASM has no default filesystem. The host
    (browser File API / Node fs / Deno / fetch) owns I/O and passes
    `Uint8Array` in. Keeps the binding small and portable.
  - **serde-wasm-bindgen** for the Rust→JS bridge. agx-core's Step
    type already derives Serialize, so objects cross with identical
    field names to the JSON export.
  - **Panic hook**: `console_error_panic_hook` default-on; Rust
    panics show up as readable browser-console messages instead of
    opaque "unreachable executed" traps.
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
- Agx doctor subcommand — stepwise suite health check
Rounds out suite-conventions §2 — all three tools (rgx, agx,
  sift) now expose a `doctor` subcommand that reports which
  siblings are on PATH. Matches the shape already documented in
  sift's agent-guide and in agx's docs/stability.md.
- *(agx-mcp)* Agx_list_annotations tool — human → agent messaging
Opens a new channel in the agx-mcp surface: users leave notes via
  `a` in the TUI, agents read them on the next invocation to pick up
  guidance across turns / sessions without the user having to repeat
  themselves in the chat prompt.
- *(phase7.4b)* CI wheel + WASM publishing matrix
Ships the deferred CI matrix from Phase 7.4b — actual PyPI / npm
  publication paths are now one tag-push away. Closes the last
  infrastructure gap in the library-mode track.

  New workflows:

  **.github/workflows/python-wheels.yml** — agx-py wheels via
  maturin-action across 4 platforms (linux-x86_64, linux-aarch64,
  macos-arm64, windows-x86_64) + an sdist job. abi3-py310 on our
  end means one wheel per platform, not per Python minor version —
  matrix stays compact. Each build uploads its artifact; a final
  publish job gates on `refs/tags/v*` and uses MATURIN_PYPI_TOKEN.

  **.github/workflows/wasm-packages.yml** — agx-wasm built for
  web / nodejs / bundler via wasm-pack. Bundler variant is the
  canonical npm publish; the other two ship as artifacts. npm
  publish gated on tag push with NODE_AUTH_TOKEN.

  Both workflows:
  - Trigger on `refs/tags/v*` push AND `workflow_dispatch` so the
    maintainer can spot-check the matrix without tagging.
  - Use `environment: pypi` / `environment: npm` so the actual
    token access goes through GitHub's environment protection
    rules (approval gating, branch restrictions).
  - Restrict `permissions:` to `contents: read` at the workflow
    level — no write access by default.

  Security review (matches the in-file header comments):
  - No untrusted `github.event.*` consumption in `run:` commands.
  - `matrix.target` / `matrix.python` are static strings from the
    workflow file, not user input.
  - Secrets threaded via `env:` only — the safe pattern per
    https://github.blog/security/vulnerability-research/how-to-catch-github-actions-workflow-injections-before-attackers-do/

  Secrets required before first tagged release:
  - `PYPI_API_TOKEN` scoped to the `agx` project on pypi.org
  - `NPM_TOKEN` automation token for the `@brevity1swos` scope

  ROADMAP 7.4b marked shipped. Next actual release can be cut as
  a tag once `cargo publish -p agx-core` runs on crates.io.
- *(phase5.4)* Experimental shell-replay MVP with triple-gate safety
Phase 5.4 — v1 ships the shell-backend replay path with three independent
  gates layered from launch through per-invocation confirm. MCP + API backends
  are deferred; their classifier arms land in follow-ups.

  Three gates that must all pass before a replay runs:

    1. `--experimental-replay` at launch — intent announcement; stock-build
       users cannot trigger replay at all.
    2. `--allow-shell-replay` at launch — tool-kind gate. A future MCP or API
       backend will have its own `--allow-*-replay` flag.
    3. Per-invocation `y` confirm in TUI — even with both flags, every `R`
       press asks before executing.

  The flow:

    - `R` on a tool_use step → `replay::classify()` returns one of
      `NeedsConfirm { input }` / `NotReplayable { reason }` / `FlagMissing
      { hint }`. Pure, no side effects, unit-testable without a terminal.
    - On `NeedsConfirm`, the status bar prompts "Replay? y/n" with the
      extracted command. `y` spawns `/bin/sh -c <input>` via
      `replay::execute_shell()` capturing stdout / stderr / exit code /
      wall-clock ms. `n` aborts silently.
    - Every attempt appends one JSON line to `<session>.replay.log` next to
      the session. Schema: ts_ms, step_index, tool_name, tool_call_id,
      input, exit_code, duration_ms, stdout, stderr. Original session file
      is NEVER touched — agx-core's read-only posture is absolute.

  Input extraction tolerates both `command` (Claude Code / Codex shape) and
  `cmd` (Gemini shape), mirroring the duck-typing already in the tool-use
  parsers.
- *(replay)* Output cap + wall-clock timeout
Phase 5.4 iter 2 — bound the worst-case memory and time cost of an
  experimental shell replay so a runaway `yes`/`cat /dev/zero` or a
  hung `sleep 999999`/`nc -l` can't freeze or OOM the TUI.

    - MAX_CAPTURE_BYTES = 4 MiB per stream. Reader threads drain past
      the cap into a scratch buffer so the child's pipe doesn't block
      on a full kernel buffer when the child ignores SIGPIPE.
    - DEFAULT_TIMEOUT_SECS = 30. `try_wait` polled every 50ms; on
      deadline the child is killed and reaped, `timed_out` flag set.
    - New flags `timed_out`, `stdout_truncated`, `stderr_truncated`
      on ReplayOutput and on each sidecar log entry. Schema-additive —
      old log consumers that ignore unknown fields keep working.
    - TUI status bar surfaces the markers so a 30s timeout-kill or
      a 4 MiB-capped buffer isn't visually indistinguishable from a
      normal completion.

  Tests use `awk 'BEGIN{for(i=0;i<2000;i++)printf "x"}'` instead of
  bash brace expansion (`{1..2000}`); the latter isn't expanded by
  dash, which is `/bin/sh` on Debian / Ubuntu CI runners.

  `execute_shell_with_limits` is `pub(crate)` so tests run with 1s
  deadline / 64-byte caps instead of waiting 30s per test.

### Miscellaneous

- Disable ratatui default features to drop unused termwiz backend
agx uses the crossterm backend only (via ratatui-crossterm). Ratatui's
  default features also pull in ratatui-termwiz → termwiz → phf → rand,
  which triggered RUSTSEC-2026-0097 (unsound rand::rng() with custom
  logger) via a build-time code path that agx does not use.

  Disabling default features and opting into just ["crossterm"]:
  - Eliminates RUSTSEC-2026-0097 (cargo audit now clean)
  - Drops the dependency graph from 197 to 98 crates
  - Reduces cold build time
  - Shrinks the supply chain attack surface
- Resolve all pedantic clippy warnings (audit pass)
10-iteration audit-fix loop. Convergence at iteration 2:
  - timeline.rs: #[allow(cast_precision_loss)] on format_duration_ms,
    #[allow(many_single_char_names, cast_sign_loss, cast_possible_wrap)]
    on parse_iso_ms — justified by the Howard Hinnant date algorithm
    requiring mixed-sign arithmetic and the short variable names being
    standard for date math (y, h, d, mo, mi, se).
  - codex.rs: renamed Entry.entry_type → Entry.kind to satisfy
    clippy::struct_field_names (field name starting with struct name).

  Iterations 3-10: zero issues found. 112 tests pass on every iteration.
  Clippy strict and pedantic both clean across all 10 passes.
- Resolve pedantic clippy warnings (final audit pass)
10-iteration audit. Convergence at iteration 1:
  - generic.rs: merged system + catch-all match arms (identical bodies)
  - tui.rs: #[allow(struct_excessive_bools)] on App (4 independent
    overlay/layout toggles, justified)
  - tui.rs: run() takes Option<&dyn Fn()> instead of Option<Box<dyn Fn()>>
    (pedantic: don't pass by value what you don't consume)
  - tui.rs: run_loop() takes Option<&dyn Fn()> instead of &Option<Box<>>
    (pedantic: Option<&T> over &Option<T>)
  - tui.rs: collapsed 3-deep nested if into let-chain for live reload
  - tui.rs: removed unused SystemTime import
  - main.rs: uses .as_deref() to convert Option<Box<dyn Fn>> to Option<&dyn Fn>

  116 tests pass across all 10 iterations. Strict + pedantic both clean.
- Rename published crate to agx-cli (binary stays agx)
The unqualified `agx` name on crates.io was published 2026-03-19
  by an unrelated project ("Cross-platform AI agent terminal
  multiplexer", 21 downloads, no overlap with this org). To unblock
  publishing we go out as `agx-cli`; the binary that users invoke
  is still `agx`, so the install UX is:

    cargo install agx-cli && agx --help

    - package.name: agx → agx-cli
    - [[bin]] name = "agx", path = "src/main.rs" preserves the binary
    - [lib] name = "agx", path = "src/lib.rs" preserves the internal
      `use agx::…` paths used by main.rs and the bench
    - description, keywords, categories, documentation, readme
      added so the crates.io listing is rich on first publish
    - README install section moved crates.io to the primary path,
      `cargo install --path .` kept as the source-build fallback

### Refactoring

- /techdebt + /simplify + /security-scan sweep (10 iters)
Ran 10 iterations of agent-led review + direct edits. Converged at iter 4
  (agent reported "no significant tech debt remaining"); iters 5–10 are
  verification passes that kept the full function test suite green. Final
  state: 297 tests pass, cargo clippy clean (both feature configs),
  cargo audit clean, fmt clean.

  Changes (9 behavior-relevant edits, 1 duplicate sum fold from the
  simplifier agent):

  SECURITY (MED)
  - timeline::short_id: char-based truncation replaces `&id[..11]` so a
    crafted tool_use_id with a 4-byte emoji straddling byte-index 11 no
    longer panics. Added regression test proving the previous slice
    would have panicked on "abcdefghijk😀xyz".

  TECH DEBT (MED)
  - timeline::Step + StepKind now derive Default; the four step
    constructors use `..Step::default()` instead of repeating eight
    `None` field initializers each (saved 32 lines of boilerplate,
    and every future Step field adds zero friction).
  - corpus::load_one returns a dedicated LoadOutcome { Ok, Skip, Err }
    enum instead of smuggling "skip" through an `anyhow` error with
    the magic substring "AGX_SKIP". A future refactor reshuffling
    error messages can no longer silently misclassify.
  - corpus: OtelProto-feature-off skip predicate now compile-time
    `cfg!(not(feature = "otel-proto"))` instead of substring-matching
    the stub error message. Strictly stronger: triggers on any load
    failure of the gated format.
  - session.rs: struct-level `#[allow(dead_code)]` narrowed to
    per-field on the actually-unused ones (uuid, parent_uuid, role).
    timestamp + model + message + content + usage are actively read.
    Preserves useful compiler warnings for future additions.
  - otel_json.rs: same treatment for Span.name.

  TECH DEBT (LOW)
  - slice.rs: `session_start_ms.unwrap()` eliminated via
    `.then().flatten()` + `if let Some(start) = time_anchor`. One less
    fragile point where guard/consumer drift could wedge the filter.
  - main::print_diff: two `.find(…).unwrap()` linear-scan lookups
    replaced with HashMap::get(…).zip(…). Faster (O(both) instead of
    O(both · |stats|)) and cannot panic if stats_a / stats_b grow an
    inconsistency.

  VERIFIED BUT LEFT ALONE
  - `unique_models` O(n·m) dedup in timeline::compute_session_totals —
    m is 1-3 in practice; HashSet would be over-engineering.
  - TUI annotations overlay, codex/langchain/vercel_ai parsers —
    iter-4 agent confirmed clean.
  - ANSI escape passthrough in `--summary` — acceptable trust boundary
    for a local-only CLI on user-generated files.

  No new dependencies.
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
- *(replay)* Iter 1 — cancel-on-key, empty-cmd refusal, simplifications
/loop iter 1 of /techdebt + /simplify + /security-scan sweep. Convergent
  findings from three parallel agent reviews applied; output cap and
  timeout land in a later iter.

  **Cancel-on-non-confirm (security + UX fix)**
  In tui.rs, `pending_replay` used to be cleared only by `y`. Pressing any
  other key — including `Esc` — left the confirm primed, so a later `y`
  pressed for an unrelated purpose would fire the stale command. Now:

    - `Esc` with a pending replay cancels cleanly and shows "replay
      cancelled" without quitting the TUI.
    - Any non-`y`/`R` key (arrows, `j`, `k`, `:`, `/`, etc.) clears the
      pending state before its normal handler runs, so navigation
      reliably defuses the primed confirm.

  **Empty-command refusal (correctness)**
  `extract_shell_command` can return `None` on malformed JSON, and the old
  `.unwrap_or_default()` turned that into `NeedsConfirm { input: "" }`.
  Accepting the confirm would spawn `/bin/sh -c ""` (a no-op) and log a
  misleading `exit=0` sidecar entry. The classifier now surfaces
  `NotReplayable { reason: "could not extract shell command from step" }`
  for both extract-failed *and* extract-returned-empty-string paths.

  **Idiomatic cleanups**
    - `with_context(|| "literal")` → `context("literal")` in `execute_shell`
      (no closure for a free literal; clippy-idiomatic).
    - `map_or_else(|| "signal".to_string(), …)` → `map_or("signal".into(), …)`
      in the TUI exit-code display; the default branch is trivial enough
      that lazy evaluation saves nothing.

  **New tests**
    - `classify_refuses_malformed_input` — step detail with non-JSON input
      must surface NotReplayable, not an empty-string NeedsConfirm.
    - `classify_refuses_empty_command_string` — well-formed JSON with
      `"command":""` must also refuse, defensive against pathological
      agent output.

  **Test count**: 363 → 365 (+2). Workspace-wide cargo test, clippy -D
  warnings, and fmt all clean.

  **Deferred to iter 2+**:
    - Unbounded stdout/stderr capture (OOM via malicious session).
    - Synchronous Command::output blocks TUI (no timeout, no cancel).
    - Both need restructured shell spawn with piped I/O + wait_timeout.

### Testing

- *(replay)* Gate /bin/sh tests on cfg(unix) for windows CI
The four `execute_shell*` tests spawn `/bin/sh` directly, which
  Windows runners don't ship. The replay feature itself is Unix-only
  by design — the classifier accepts Bash-shaped tool_use only, and
  the shell-backend assumes POSIX semantics — so the tests should
  mirror that posture rather than try to run cross-platform.

  Adds `#[cfg(unix)]` to:
    - execute_shell_captures_exit_and_stdout
    - execute_shell_caps_stdout_at_the_limit
    - execute_shell_caps_stderr_at_the_limit
    - execute_shell_times_out_on_long_running

  Non-shell-spawning tests (`classify_*`, `log_replay_*`) still run
  on Windows. 100 of 104 tests run on Windows; 104 of 104 on Unix.

### Ci

- Add test + clippy + fmt + docs workflow
First real CI for the repo — only `python-wheels.yml` and
  `wasm-packages.yml` existed, both tag-triggered and never fired.
- Wire up release-plz + cliff for automated releases
Adopts the same release flow rgx uses (most recently v0.12.3
  shipped via release-plz PR #71). On push to main, release-plz
  inspects new conventional commits, opens a "chore: release" PR
  that bumps versions and appends to CHANGELOG. Merging the PR
  tags, publishes agx-core + agx-cli to crates.io, and creates the
  GitHub release.

    - .github/workflows/release-plz.yml — runs MarcoIeni/release-plz-action@v0.5
      on push to main; needs CARGO_REGISTRY_TOKEN + RELEASE_PLZ_TOKEN
      secrets.
    - release-plz.toml — workspace config; declares agx-core + agx-cli
      as publishable. agx-py / agx-wasm stay publish=false (they
      ship to PyPI / npm via their dedicated workflows). agx-mcp is
      the local MCP-server bin, unpublished.
    - cliff.toml — changelog template, repo URL pointed at agx,
      same commit-parser groups as rgx for cross-project consistency.

  CHANGELOG.md already has a hand-written v0.2.0 section
  (docs(changelog) commit aa3d26a); on first release-plz run cliff
  will prepend its commit-derived v0.2.0 above. Clean up by
  removing the cliff-generated copy when reviewing the release-plz
  PR — the prose version is the one to keep.


## [0.2.0] - 2026-05-23

The substance release. Adds two more agent-trace formats (LangChain, Vercel AI SDK), full OpenTelemetry GenAI support, fork / branch detection, jump-to-step launch positioning, desktop notifications for live mode, trajectory export for RL training data, corpus-level distributional stats, PII / credential scanning, an experimental shell-replay subsystem with triple-gate safety, a workspace split with publish-ready Python (PyPI) / WASM (npm) bindings, formal stability commitments, an MCP server for agent self-introspection, and an `agx doctor` health-check subcommand.

The published crate is **`agx-cli`** — the `agx` name on crates.io was claimed by an unrelated project before this one existed. The installed binary remains `agx`:

```
cargo install agx-cli && agx --help
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

- **Phase 7.1** — Workspace split. `crates/agx-core/` is the pure, TUI-free library (parsers, timeline, corpus, pricing, annotations, PII, semantic, notify, export). Top-level `agx-cli` keeps the TUI + clap + arboard dependencies. `agx-core` is publishable to crates.io independently for Python / WASM / eval-harness consumers.
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

- Crate published to crates.io as `agx-cli`; the binary, the internal lib (`use agx::…`), and the brand all remain `agx`.
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
