# agx Roadmap

Long-term development plan for agx, organized into phases. Each phase is a
minor version (v0.2, v0.3, …). Phases are ordered by dependency, not by
calendar — ship a phase when it's ready, not on a schedule. Work inside a
phase can be parallelized across contributors.

## Executive summary

**What changed in this revision.** The prior roadmap implicitly treated agx
as a tool for "users of specific agent CLIs" (Claude Code, Codex, Gemini,
Aider, Cline) and leaned toward competing with hosted dashboards. Both
framings were too narrow. The sharp positioning is now:

> **rgx : regex101.com :: agx : Langfuse / LangSmith / internal agent dashboards**

agx is the **terminal-native sibling** of browser-based agent trace
dashboards, not a replacement. Langfuse, LangSmith, Helicone, and your
team's internal dashboard still own team sharing, retention, alerting, and
cross-team analytics. agx is what you reach for when you're already in the
terminal and want to scrub through a session with vim bindings — the `gdb`
of agent execution.

**Who it serves.** Every developer who builds agentic AI services — as broad
as "developers who use regex." That is language-agnostic, framework-agnostic,
vendor-agnostic: LangChain / Vercel AI SDK / LlamaIndex / Pydantic AI
builders, AutoGen / CrewAI / DSPy / LangGraph framework users, eval
engineers, T&S investigators, RL / RLHF researchers working over agent
trajectories, customer-support engineers reading user bug reports, and
agent-CLI teams dogfooding their own output. The unifying value prop: a
step-through debugger for agentic flow.

**Concrete consequences for the roadmap** (each is rationalized in the
relevant phase):

1. **OTel GenAI semconv moves from "v1.0 afterthought" to Phase 2.** If OTel
   becomes the dominant instrumentation standard, supporting it unlocks
   every framework that exports OTel in a single move. Waiting until v1.0
   loses multiple years of framework-level coverage.
2. **Framework-level traces (LangChain, Vercel AI SDK, LlamaIndex, Pydantic
   AI) get their own phase** rather than being buried inside "generic
   parser" polish. They probably outnumber CLI users by an order of magnitude.
3. **Multi-session corpus analysis moves from v1.0 → Phase 3.** RL
   researchers and eval engineers need cross-trajectory stats on day one,
   not at v1.0.
4. **A dedicated performance phase is added.** A researcher scanning 10k
   trajectories needs <1s load on a large session, not 30s. Speed is a
   feature of the target audience's workflow, not a polish item.
5. **Library mode (`agx-core` crate + Python / TS bindings) becomes a
   phase.** Reuse of parsers from custom eval harnesses is how rgx-style
   tools become ecosystem primitives rather than end-user-only apps.
6. **RL / eval export is promoted** — trajectory dataset inspection and
   export to training-data formats (JSONL with role tags, Hermes-style) is
   a strictly larger market than debug-my-CLI-agent.
7. **MCP (Model Context Protocol)** gets explicit treatment once tool-call
   metadata richens; not a separate phase but a subplan inside the format
   and replay phases.

**Guiding principles** (kept in sync with CLAUDE.md "Not to Do"):

1. Narrow scope, deep engineering, terminal-native. No hosted components,
   no telemetry, no team-sharing features.
2. Read-only by default. Write-back (annotations, replay) is opt-in and gated.
3. Format drift must degrade gracefully — `#[serde(other)]` everywhere,
   never panic on unknown fields.
4. Every new parser reuses `timeline::Step` helpers so the TUI renders
   every format identically. No unifying `Entry` trait across formats.
5. Keep the dep baseline lean. Anything that pulls in heavy crates (SQLite,
   ONNX, protobuf, tokio-full) goes behind a feature flag.
6. MSRV locked at Rust 1.85 (required by edition 2024) until a Phase bumps it with an explicit note.

---

## Phase 0 — v0.1.x Stabilization ✅ (shipped 2026-04-14)

**Goal:** Close gaps surfaced in the v0.1.0 code review before adding
features. Everything here is low-risk, small-diff, no architectural change.

**Duration:** one or two patch releases (v0.1.1, v0.1.2).

### Subplans

**0.1 — Doc & metadata drift fixes** ✅
- [x] README: correct dependency count (6 → 8; ratatui, crossterm, serde,
      serde_json, anyhow, clap, clap_complete, arboard)
- [x] README: update `--summary` example output to match current format
      (`Loaded <fmt> session from …`, no `other` count)
- [x] CONTRIBUTING.md: copy the parser-adding recipe from CLAUDE.md →
      CONTRIBUTING.md for external visibility
- [x] Issue templates (`.github/ISSUE_TEMPLATE/format_drift.md`,
      `bug_report.md`, `feature_request.md`) with a "Paste the first 10
      lines of your session file" field in the drift template

**0.2 — Corpus fixture system** ✅ (scaffold; fixtures accumulate later)
- [x] `tests/corpus/<format>/` directory layout documented in
      `tests/corpus/README.md`
- [x] `tests/corpus_test.rs` integration test: for every file in
      `tests/corpus/`, runs `agx --summary` and asserts exit code 0;
      no-ops when the directory is empty (v0.1 baseline)
- [x] CONTRIBUTING.md documents anonymization + contribution flow

**0.3 — `--debug-unknowns` flag** ✅
- [x] `--debug-unknowns` CLI flag (`src/debug_unknowns.rs`)
- [x] Per-format scanners report unknown top-level types, payload types,
      and content-item types to stderr, with line-number samples
- [x] Zero new deps, zero runtime cost when the flag is off
- [x] Verified against all four fixtures: flags `permission-mode`
      (Claude Code) and `reasoning` (Codex) as expected; clean on Gemini
      and Generic

**0.4 — Integration test for `--summary`** ✅
- [x] `tests/summary_test.rs` — 6 tests covering format-label assertions
      for all four fixtures, missing-file exit code, and stderr split
      with `--debug-unknowns`
- [x] Uses stdlib `Command` rather than `assert_cmd` to keep the dev-dep
      baseline at 1 crate (tempfile)

**Shipped:** `cargo test` = 125 unit + 1 corpus + 6 integration = 132 tests
passing. `cargo clippy --all-targets -- -D warnings` clean. README matches
actual `--summary` output. Contribution on-ramp is documented.

---

## Phase 1 — v0.2: Observability & Cost ✅ (shipped 2026-04-15)

**Goal:** Answer the first question every user asks after a session:
*"how much did that cost and where did the time go?"* Pure deepening of the
existing timeline model — no new formats, no new architecture.

**Why this first:** biggest value jump with zero architectural risk, and
tokens + cost are prerequisites for the corpus analytics in Phase 3.

### Subplans

**1.1 — Per-step token usage** ✅
- [x] Extend `timeline::Step` with `tokens_in: Option<u64>`,
      `tokens_out: Option<u64>`, `cache_read: Option<u64>`,
      `cache_create: Option<u64>`, `model: Option<String>`
- [x] Claude Code: parse `message.usage` from assistant entries in `session.rs`
- [x] Codex: parse `usage` on `response_item` message payloads in `codex.rs`
      (handles snake_case and legacy camelCase)
- [x] Gemini: parse `usageMetadata` from message objects
      (`promptTokenCount` / `candidatesTokenCount` / `cachedContentTokenCount`)
- [x] Generic: usage from OpenAI-compatible per-message `usage` field
- [x] Unit tests per format with fixture entries carrying realistic usage
- [x] Convention documented: usage + model attach to the FIRST step emitted
      from each assistant message (avoids double-counting in corpus sums)

**1.2 — Cost tables** ✅
- [x] `src/pricing.rs` with hardcoded per-model USD-per-1M-token prices
      for opus-4-6, sonnet-4-6, haiku-4-5, gpt-5, gpt-5-mini,
      gemini-2-5-pro, gemini-2-5-flash
- [x] `Step::cost_usd()` computed from tokens × model rate; returns `None`
      if model is unknown rather than guessing
- [x] Each pricing entry carries a `last_verified` field; a test asserts
      the field is non-empty on every row
- [x] `--no-cost` flag to suppress cost columns in summary, TUI status bar,
      detail pane, and stats overlay

**1.3 — Summary + TUI rendering** ✅
- [x] `--summary` mode adds total-cost, total-tokens, and model-list lines;
      falls back to "(unknown — no pricing entry for model)" when the model
      isn't in the pricing table
- [x] Integration tests guard the summary against regression and verify
      `--no-cost` suppresses cost while keeping tokens
- [x] Claude Code fixture enriched with realistic usage so the pipeline is
      exercised end-to-end (4 assistant turns with cache-hit pattern)
- [x] Stats overlay (`s`) adds session totals (tokens + cost + model list)
- [x] Status bar shows running cost of session alongside position gauge
- [x] Per-step detail pane shows duration, model, tokens, and cost as a
      meta block above the detail text

**1.4 — Export** ✅
- [x] `--export md` — Markdown transcript, ASCII-only kind prefixes
      ([user] / [asst] / [tool] / [result] per terminal-native principle),
      code-fenced tool I/O, totals header
- [x] `--export html` — self-contained HTML with inline CSS, no JS, no
      external assets. Color-coded by step kind. Escapes detail to prevent
      injection
- [x] `--export json` — stable-schema JSON dump (`{totals, steps}`) as the
      first public programmatic interface; serde_json round-trips through
      `serde_json::Value`
- [x] Unit tests cover JSON round-trip, MD section count, HTML
      self-containment, HTML injection prevention, and cost suppression
      paths

**1.4 — Export**
- [ ] `--export md` — Markdown transcript, one section per step, code-fenced
      tool I/O, optional front-matter with totals
- [ ] `--export html` — self-contained HTML, inline CSS, color-coded kinds,
      no JS, no external links
- [ ] `--export json` — stable-schema JSON dump (`Vec<Step>` + totals) as
      the first public programmatic interface (tees up Phase 7 library mode)

**Acceptance:** on a real session with 50+ tool calls,
`agx --summary` shows total cost, `s` in TUI shows per-tool cost breakdown,
`agx --export md > session.md` produces a readable transcript, and the JSON
export round-trips (`jq . | agx --import` path reserved for Phase 7).

**Depends on:** Phase 0.
**Feeds:** Phase 3 (corpus analytics need cost), Phase 7 (JSON schema is
the basis for library mode).

**Rationale vs prior roadmap:** unchanged in scope but explicitly promoted
to "prerequisite for corpus analytics." Added `--no-cost` for the portion
of the audience (privacy-sensitive researchers, internal eval teams on
unpriced custom models) who don't want cost estimation at all.

---

## Phase 2 — v0.3: OpenTelemetry GenAI + Framework Traces (in progress)

**Goal:** Capture the framework-level audience in one move. Support OTel
GenAI semconv as a first-class format and ship parsers for the three or
four framework formats that don't emit OTel yet.

**Why this second (moved up four phases):** if the target audience is
"every developer who builds agentic AI services," the mass of that
audience is on LangChain / Vercel AI SDK / LlamaIndex / Pydantic AI,
**not** on any specific agent CLI. OTel GenAI is converging as the
cross-framework instrumentation standard — supporting it is the single
biggest leverage point in the roadmap.

### Subplans

**2.1 — OTel GenAI (JSON export)** ✅
- [x] `src/otel_json.rs` parser for OpenTelemetry `traces.json` exports
      (OTLP-JSON envelope: `resourceSpans` → `scopeSpans` → `spans`)
- [x] Map GenAI semconv attributes → `Step`: `gen_ai.request.model` →
      model, `gen_ai.usage.input_tokens` / `.output_tokens` /
      `.cache_read_tokens` / `.cache_creation_tokens` → tokens,
      `gen_ai.tool.name` / `.call.id` / `.call.arguments` / `.call.result`
      → tool_use + paired tool_result, `gen_ai.operation.name` drives
      span classification
- [x] Chronological ordering: spans sorted by `startTimeUnixNano` across
      ResourceSpans / ScopeSpans boundaries
- [x] Non-GenAI spans (generic HTTP / DB) ignored so agx coexists cleanly
      with mixed traces
- [x] Detection: file contains `resourceSpans` top-level key →
      `Format::OtelJson`, probed before Gemini/Generic in `format::detect`
- [x] Synthetic fixture `assets/sample_otel_json_traces.json` covers
      chat → execute_tool → chat with usage + tool pairing
- [x] 7 unit tests cover minimal chat, usage attachment, system-role
      dropping, tool pairing, non-GenAI span filtering, cross-span
      chronological sorting, and the full fixture round-trip
- [x] `--debug-unknowns` scans OTel files and reports unknown
      `gen_ai.operation.name` values (known set: chat, text_completion,
      generate_content, execute_tool)
- [x] Browser label: `[OTel  ]`
- [ ] **Deferred**: OpenInference attributes (`llm.*` prefix). Will be
      added when a real LangChain/LlamaIndex fixture lands in
      `tests/corpus/otel_json/`

**2.2 — OTel GenAI (OTLP protobuf)** ✅
- [x] `--features otel-proto` compile flag (default off; gates `prost`
      behind a flag per our dep discipline — also skips the originally
      planned `opentelemetry-proto` dep by hand-writing a minimal
      prost schema, keeping the feature-on build lean)
- [x] `src/otel_proto.rs` decodes binary `.pb` / `.otlp` files — stub
      function when the feature is off, real prost-backed parser when
      on; both behind the same `load(path) -> Result<Vec<Step>>` API
- [x] When the feature is off, a non-UTF-8 file prints a helpful error
      (`rebuild with --features otel-proto` + the exact `cargo install`
      / `cargo build` commands) rather than a parse crash. Binary
      content is routed to `Format::OtelProto` at detection time so the
      failure message surfaces at dispatch, not deep in serde.
- [x] `format::detect` reads bytes first (previously UTF-8 string) so
      it can distinguish JSON/JSONL from binary protobuf cleanly
- [x] Reuses `otel_json::append_span` for span → Step conversion — only
      the wire decode differs between the JSON and protobuf paths
- [x] Unit tests build fixtures in-memory via prost `encode_to_vec` and
      round-trip through `load`: minimal chat, usage attachment,
      execute_tool pairing, cross-resource chronology, non-GenAI span
      filtering, invalid-protobuf error. Stub-path test asserts the
      helpful-error message when the feature is off.
- [x] Feature-on and feature-off builds both clippy-clean under strict
      lints; one `#[allow(clippy::enum_variant_names)]` on the
      `any_value::Value` enum since variant names mirror the OTLP
      `AnyValue` oneof field names (`string_value`/`bool_value`/etc.)
- [x] Tests: feature off = 173 unit + 1 corpus + 9 integration = 183;
      feature on = 178 unit (+5 for the protobuf path)

**2.3 — LangChain native `.jsonl` / LangSmith export**
- [ ] `src/langchain.rs` parser for LangChain's `.jsonl` trace export and
      LangSmith's export-run JSON
- [ ] Handle `tool_calls[].name` vs OpenAI's `function.name` split
- [ ] Detection: first line has `run_type` or `serialized.lc` keys

**2.4 — Vercel AI SDK traces**
- [ ] `src/vercel_ai.rs` parser for `streamText` / `generateText` saved
      traces (JSON arrays of `{ role, content, toolInvocations }`)
- [ ] Already partially hits `Format::Generic`; split out because the
      `toolInvocations` schema is idiosyncratic enough that generic
      treatment loses fidelity
- [ ] Verify against a trace captured from `@ai-sdk/openai` with tool calling

**2.5 — LlamaIndex + Pydantic AI quick wins**
- [ ] LlamaIndex: most LlamaIndex users export via OTel already (covered by
      2.1); add a targeted path only if a different native format shows up
      in fixture contributions
- [ ] Pydantic AI: parse the `agent.run_sync()` log shape if / when users
      contribute fixtures — otherwise punt to Phase 8 long-tail

**2.6 — Detection reshuffle**
- [ ] `format::detect` now probes in order: Gemini single-object →
      Codex → OtelJson → LangChain → VercelAI → ClaudeCode → Generic
- [ ] Content-based only; still no extension sniffing
- [ ] Add detection unit tests covering each new format's disambiguator

**Acceptance:** a LangChain-over-OTel trace from a user's local
`otel-desktop-viewer` dump loads in agx with the same TUI shape as a
Claude Code session. A Vercel AI SDK `streamText` trace shows tool calls
paired with their results. Core binary size unchanged (OTel-proto hidden
behind the feature flag).

**Depends on:** Phase 0 (fixture layout), Phase 1 (Step usage fields so
OTel `gen_ai.usage.*` attributes have a home).
**Feeds:** Phase 3 (OTel coverage multiplies what corpus analytics can scan).

**Rationale vs prior roadmap:** The prior roadmap put OTel in Phase 5.1
(v1.0) and framework traces inside Phase 2.4 as an afterthought. Under the
new "every agentic-AI developer" audience, this inverts: framework-level
coverage is the point, and OTel is the highest-leverage form of it. Moving
it up front-loads the biggest audience expansion of the roadmap.

---

## Phase 3 — v0.4: Corpus Analysis & Performance

**Goal:** Let a researcher or eval engineer point agx at 10,000 trajectories
and get answers in seconds. Two intertwined concerns — cross-session
analytics and the raw speed to make them tolerable — so they ship together.

**Why this third (moved up from v1.0):** The RL / eval audience cannot wait
for v1.0 to ask "across these 10k trajectories, which tools error most?"
Nous Research and similar trajectory-heavy users need this early or they'll
build around agx instead of with it.

### Subplans

**3.1 — `agx corpus` command**
- [ ] `agx corpus <dir>` subcommand: loads every session in a directory
      tree, format-auto-detected per file
- [ ] Parallel load via `rayon` (new feature-flagged dep; defaults on
      because corpus is the flagship use case)
- [ ] Cross-session aggregates: total cost, total tokens, per-tool error
      rate, per-tool latency p50/p95, per-model usage, sessions/day histogram
- [ ] Output modes: `--summary` (text), `--export json`, `--export csv`
      (new), dedicated corpus TUI (3.3)
- [ ] `--filter model=gpt-5` / `--filter tool=Bash` / `--filter errored`
      post-filter predicates

**3.2 — Performance pass**
- [ ] Benchmark baseline on a large real session (~50MB JSONL, ~2000
      steps) using `criterion`; target: <1s wall time for `--summary`
- [ ] Replace `serde_json::from_str(&entire_file)` with line-streaming
      `Deserializer::from_reader` where not already used
- [ ] Avoid cloning `Step.detail` strings when rendering; intern repeated
      tool names via a small string-table in `App`
- [ ] Lazy detail expansion: timeline list holds only label + kind +
      offsets; detail pane reads from backing buffer on select
- [ ] Memory ceiling: document the target (~3x file size resident for
      Claude Code JSONL) and regression-test it
- [ ] `--bench` hidden flag prints load + parse timings for diagnostics

**3.3 — Corpus TUI view**
- [ ] `agx corpus --tui <dir>` launches an overview TUI: session list
      (left) sorted by mtime or cost or error count; selected-session
      summary (right); Enter to drill into the normal per-session TUI,
      Esc returns to the corpus view
- [ ] Per-tool heatmap across sessions (reuse Phase 0 heatmap machinery)
- [ ] Keybindings consistent with the session TUI (j/k, /, f, :N)

**3.4 — Eval-loop integration**
- [ ] `agx corpus <dir> --json-lines` streams one JSON per session as they
      parse (eval pipelines can `tail -f` this)
- [ ] Exit code reflects "any session errored" when `--fail-on-errored` is
      set — lets CI pipelines gate on agent health

**Acceptance:** `agx corpus ~/.claude/projects/` on a 30-day corpus of a
few hundred sessions returns a summary in under 5 seconds, the corpus TUI
opens instantly, and a single large session (~2000 steps) loads under 1s.

**Depends on:** Phase 1 (cost for aggregation), Phase 2 (OTel + framework
coverage so the corpus isn't just Claude Code).
**Feeds:** Phase 4 (diff depth), Phase 6 (RL export reuses corpus filters).

**Rationale vs prior roadmap:** Corpus was a v1.0 subplan (5.2). Promoted
because the audience shift makes it a day-one need; performance sibling
added because corpus on slow parsers is unusable.

---

## Phase 4 — v0.5: Diff, Search Depth, Annotations

**Goal:** Turn agx from a viewer into an analysis tool. Real side-by-side
diff, deeper search, and notes that survive session edits.

### Subplans

**4.1 — Interactive side-by-side diff**
- [ ] `--diff-tui` mode: two timelines in parallel panes, synchronized
      scrolling, single cursor walking aligned pairs
- [ ] Alignment: match by `(tool_name, normalized_input)` first, fall back
      to position; show gray gutters where only one side has a step
- [ ] Color-code: green = match, yellow = input differs, red = outcome
      differs, gray = only-in-one-side
- [ ] `j`/`k` walk aligned pairs, `Tab` jumps to next unaligned-only step,
      `d` toggles inline diff of detail pane
- [ ] Unit test: synthetic pair where both sessions do "write fib.py"
      with minor input variance aligns correctly

**4.2 — Jump-to-time + trim**
- [ ] `:@HH:MM:SS` / `:@12:34` jumps to first step at-or-after that time
- [ ] `--after <duration>` / `--before <duration>` CLI filters (e.g.
      `--after 2h`)
- [ ] `--after-step <N>` / `--before-step <N>` step-index slices
- [ ] `--range <a..b>` as sugar (e.g. `--range 100..500`)

**4.3 — Annotations**
- [ ] `a` in TUI opens annotation prompt for current step
- [ ] Stored in `.agx/<session-id>.notes.json` sibling to session file;
      falls back to `~/.agx/notes/<session-id>.json` if sibling write fails
- [ ] Rendered as a marginal indicator in the timeline (ASCII-only `*`
      prefix — no emoji per "terminal-native" principle)
- [ ] `A` opens annotation list overlay for current session
- [ ] Exports carry annotations (Phase 1.4 md/html/json)
- [ ] `agx corpus` aggregate: "sessions with annotations" filter

**4.4 — Semantic search (opt-in feature flag)**
- [ ] `--features embedding-search` compile flag, default off
- [ ] `//query` prefix in search triggers embedding-based lookup
- [ ] Use `fastembed-rs` (pure-Rust ONNX); model downloaded once to
      `~/.cache/agx/` on first use, no network calls afterward, no API
      calls ever
- [ ] Without the feature: `//query` prints "semantic search not compiled in"

**Acceptance:** user can diff two sessions side-by-side in TUI with
inline-highlighted input drift, add notes to specific steps that survive
across re-runs, and slice the timeline by time or step range. Core binary
under 5MB without `embedding-search`.

**Depends on:** Phase 0.
**Feeds:** Phase 6 (RL export includes annotations as training signal).

**Rationale vs prior roadmap:** essentially the old Phase 3, shifted one
slot later because OTel + corpus now precede it. Annotations + corpus
integration is a new bullet (3.3 last item) since the corpus view benefits
from "show me sessions I've annotated."

---

## Phase 5 — v0.6: Branch, Replay, and MCP-Aware Tool Calls

**Goal:** The "gdb `p x = 5`" moment — read-write features, gated behind
explicit flags because this is where we leave safe-viewer territory. Also
lean into MCP as the tool-call metadata layer matures.

### Subplans

**5.1 — Branch / fork visualization**
- [ ] Walk `parentUuid` in Claude Code entries to build a conversation
      tree in `timeline::build_tree()`; most sessions are linear but
      edit/resume creates branches
- [ ] TUI overlay: ASCII tree of branches, `b` lists, Enter switches view
- [ ] Codex and Gemini: implement only if their formats carry branch
      pointers; otherwise this is Claude-Code-only and documented as such

**5.2 — MCP-aware tool call rendering**
- [ ] When a tool call carries MCP metadata (server name, resource URI,
      prompt ID), render them in the detail pane
- [ ] Pair MCP tool calls with their corresponding resource reads in the
      timeline (new `StepKind::McpResourceRead` variant if warranted)
- [ ] Works across any format whose tool call fields carry MCP-shaped
      metadata — not a new parser, a render pass
- [ ] Depends on ecosystem: ship progressively as MCP metadata surfaces
      in real sessions

**5.3 — `--live` + desktop notifications**
- [ ] Extend existing `--live` with `--notify-on-error`: when a new
      `tool_result` arrives and `is_error_result` is true, send a native
      OS notification via `notify-rust` (lightweight, cross-platform)
- [ ] `--notify-on-idle <duration>`: fire when the session hasn't grown
      for N seconds — useful for agents that hang
- [ ] Notifications are opt-in per-flag; no background daemons

**5.4 — Replay a tool call** — `--experimental-replay` gate
- [ ] `R` in TUI on a `tool_use` opens replay mode; detail pane becomes
      editable, input JSON editable inline
- [ ] Pluggable backends:
  - **MCP backend**: if a running MCP server supports the tool, dispatch
    through it (safest, declarative permissions)
  - **Shell backend**: for Bash-like tools, gated behind
    `--allow-shell-replay` AND confirmed per-invocation
  - **API backend**: Anthropic / OpenAI / Google SDK dispatch, requires
    env-var auth, gated behind `--allow-api-replay`
- [ ] Output appended to a side `replay.log`; original session file is
      **never** modified
- [ ] Ships behind `agx --experimental-replay` for at least two releases
      before graduation

**Acceptance:** user can browse branches in a Claude Code session that has
them, replay a single tool call via MCP in an isolated backend, and get a
desktop notification when a long-running live session errors. Experimental
flag gate is documented in README.

**Depends on:** Phase 0 (event loop), Phase 4 (annotations, since replay
results attach to steps).
**Feeds:** Phase 6 (RL data export uses branches as the alt-trajectory axis).

**Rationale vs prior roadmap:** Scope similar to old Phase 4. Added MCP
render pass as subplan 5.2; MCP is specifically called out in the revised
framing and deserves explicit treatment rather than "and also MCP" in a
later note. Notification subplan expanded with `--notify-on-idle` because
eval harness users hit this exact failure mode.

---

## Phase 6 — v0.7: RL Export and Eval-Harness Integrations

**Goal:** Make agx a first-class citizen in the RL / RLHF / eval ecosystem.
Nous Research, alignment research groups, and T&S teams generate or
analyze millions of trajectories — agx should be their inspector AND their
data-prep tool.

**Why this phase exists (new vs prior roadmap):** "Agent trajectory
dataset inspector + exporter" is a strictly larger market than "debug my
CLI agent." Prior roadmap had nothing for it.

### Subplans

**6.1 — Trajectory export formats**
- [ ] `--export trajectory-openai` — OpenAI fine-tuning JSONL (`{messages:
      [{role, content, tool_calls}]}`)
- [ ] `--export trajectory-hermes` — Hermes / ShareGPT-style role+content
      with tool segments as dedicated messages
- [ ] `--export trajectory-dpo` — pairs of (chosen, rejected) trajectories
      when agx can infer them from branches (Phase 5.1) or annotations
- [ ] `--export trajectory-sft` — supervised fine-tuning-ready: strip
      system prompts or keep, include tool I/O verbatim or summarize
- [ ] All exports take a `--redact` flag with a regex list that masks
      matches in tool outputs (redacting secrets before dataset release)

**6.2 — Dataset-level inspection**
- [ ] `agx corpus <dir> --trajectory-stats`: tokens per trajectory
      distribution, tool-call counts, branch-rate, annotation counts —
      the numbers a researcher needs before publishing a dataset
- [ ] `agx corpus <dir> --sample <N>` — random-sample N sessions into the
      TUI viewer for manual spot-check

**6.3 — Eval-framework adapter helpers**
- [ ] Document the exact JSON schema used by Phase 1.4's `--export json`
      and guarantee its stability (feeds Phase 7 library mode)
- [ ] Ship small adapter examples (docs only, not shipped crates): how to
      wire `agx --export json` → `inspect-ai` / `lm-eval-harness` /
      custom pipeline
- [ ] Include anonymization checklist for dataset release

**6.4 — Privacy & safety for dataset use**
- [ ] `--scan-pii`: heuristic scan for emails, phone numbers, API-key
      shapes, SSH keys in tool outputs; reports counts, doesn't mutate
- [ ] `--anonymize-uuids`: rewrite UUIDs, absolute paths, and user/project
      names in exports to stable pseudonyms
- [ ] Both are **opt-in**, documented as "best effort, not a substitute
      for human review before dataset release"

**Acceptance:** a researcher can point agx at a directory of 1000 Claude
Code sessions, redact common secret patterns, and emit a clean Hermes-style
JSONL dataset in one command. Dataset-level stats surface distributional
issues (trajectory length, tool imbalance) before release.

**Depends on:** Phase 1 (export JSON schema), Phase 3 (corpus infra),
Phase 4 (annotations as DPO signal), Phase 5 (branches as chosen/rejected pairs).

**Rationale vs prior roadmap:** entirely new phase. The RL/alignment
audience is large, explicitly called out in the framing update, and has a
concrete workflow — inspect a trajectory corpus, filter and redact, export
in their trainer's format — that none of the existing phases addressed.

---

## Phase 7 — v0.8: Library Mode (`agx-core` + bindings)

**Goal:** Let users consume agx's parsers programmatically instead of
shelling out to `agx --export json`. Turn agx from an app into an
ecosystem primitive, the way rgx-style tools become building blocks.

**Why this phase exists (new vs prior roadmap):** Anyone building a custom
eval harness, a CI guard, or a lightweight dashboard would rather `pip
install agx-core` than spawn a subprocess per session. This is a
workspace-shape refactor, not new features — but it unlocks a new class of
users who never run the TUI.

### Subplans

**7.1 — Workspace split**
- [ ] Convert the repo to a Cargo workspace: `agx-core` (parsers + Step
      model), `agx` (TUI binary depending on agx-core)
- [ ] `agx-core` has zero TUI deps (no ratatui, no crossterm, no arboard)
- [ ] Public API surface in agx-core: `Format`, `Step`, `StepKind`,
      `load(path) -> Result<Vec<Step>>`, format-specific loaders for
      direct use
- [ ] `agx-core` publishes to crates.io independently; version-locked to
      `agx` within a major

**7.2 — Python bindings**
- [ ] `agx-py` crate using `pyo3`, ships `agx` PyPI wheel (the python
      package named `agx` or `agx_core` — resolve at publish time)
- [ ] Surface: `agx.load(path) -> list[Step]`, `agx.load_corpus(dir) ->
      iterator[Step]`, `Step` as a frozen dataclass
- [ ] Wheels built via `maturin` in CI for linux-x86_64, linux-aarch64,
      macos-arm64, windows-x86_64
- [ ] No Python runtime requirement for the main `agx` binary

**7.3 — TypeScript / WASM bindings**
- [ ] `agx-wasm` crate via `wasm-bindgen`, published as `@agx/core` (or
      similar) on npm
- [ ] Surface matches Python: `load(buffer) -> Step[]`, `loadCorpus` accepts
      an async iterable of `{name, buffer}`
- [ ] Primary use case: browser-based dashboards (agx's hosted siblings)
      reusing agx parsers without a Rust build

**7.4 — Stability commitments**
- [ ] `agx-core` public API follows SemVer from v0.8.0
- [ ] `Step` JSON schema from Phase 1.4 is the wire format between the
      binary and any out-of-process consumer
- [ ] Breaking changes require a deprecation cycle documented in
      CHANGELOG.md

**Acceptance:** `pip install agx && python -c "import agx; print(len(agx.load('session.jsonl')))"`
works on Linux, macOS, and Windows. `npm install @agx/core` exposes the
same API in TS. The `agx` binary still ships as a single static binary
with no runtime dependencies.

**Depends on:** Phase 1 (stable Step + JSON schema), Phase 2 (full parser
coverage), Phase 6 (clearer use cases).

**Rationale vs prior roadmap:** new phase. The rgx-family ambition is to be
"the tool every developer reaches for," and tools at that reach are
library + CLI, not CLI-only. Python binding specifically is the only way
into the LangChain / LlamaIndex / eval-harness audience's scripts.

---

## Phase 8 — v1.0: Format Long Tail, Docs, Stabilization

**Goal:** Graduate from v0.x, commit to API stability, mop up the long tail
of format support that didn't fit earlier phases.

### Subplans

**8.1 — Long-tail CLI parsers**
- [ ] **Aider** — `.aider.chat.history.md` parser (`src/aider.rs`), markdown
      with `####` turn headers + fenced tool I/O
- [ ] **Cline / Roo Code** — VS Code extension JSON under
      `~/Library/Application Support/Code/User/globalStorage/...` /
      XDG equivalent on Linux / APPDATA on Windows
- [ ] **Cursor** — reverse-engineer storage; if hostile (encrypted SQLite),
      fall back to Cursor's "Export Chat" JSON only
- [ ] **Windsurf**, **Zed Assistant** — evaluate and pick up if formats have
      stabilized and users have asked
- [ ] **OpenClaw** — TypeScript monorepo; parse `sessions_history` export
      if it's JSON/JSONL, otherwise request an export flag upstream
- [ ] **Hermes Agent (Nous Research)** — Python agent with SQLite FTS5 +
      Markdown persistence; evaluate whether to ship a SQLite feature
      flag here or keep pushing exports
- [ ] Drop / deprecate any parser whose CLI has died

**8.2 — Format drift CI**
- [ ] Monthly GitHub Action: scans release notes of Claude Code, Codex CLI,
      Gemini CLI, Cline, Aider, LangChain, Vercel AI SDK for "session",
      "rollout", "schema", "format", "history" keywords
- [ ] Opens an issue if any match is found in the last month's releases
- [ ] `FORMAT_NOTES.md` tracks each format per-version so drift
      investigations start from a known baseline

**8.3 — Documentation & stabilization**
- [ ] `cargo doc` clean for `agx-core`, all public items documented with
      examples
- [ ] mdBook user guide at the project's docs site: install, every
      keybinding, every format, every flag, one cookbook per audience
      (LangChain user / Claude Code user / eval engineer / RL researcher)
- [ ] SemVer commitment: post-v1.0, breaking changes to CLI flags, session
      file expectations, export schemas, or `agx-core` public API require
      a major-version bump
- [ ] MSRV policy: locked at 1.85 for v1.0 (edition 2024 floor); future
      bumps require a minor bump + CHANGELOG entry

**8.4 — v1.0 release checklist**
- [ ] All subplans through Phase 7 shipped or explicitly deferred with
      written rationale
- [ ] `cargo audit` clean
- [ ] `cargo clippy --all-targets -- -D warnings` clean under strict and
      pedantic
- [ ] 300+ tests passing (ballpark; quality over quantity)
- [ ] README + ROADMAP honest about what's in and what's out

**Acceptance:** `agx` reads every major CLI and framework trace format,
docs are complete, public APIs are SemVer-stable, and a new contributor
can go from clone to merged PR from docs alone.

**Depends on:** all prior phases.

**Rationale vs prior roadmap:** old Phase 5 scope narrowed — OTel, corpus,
and plugin-API items moved earlier. What remains is long-tail CLI
coverage + documentation + stabilization, which is what a v1.0 phase
should actually be. OpenClaw and Hermes added to the long-tail list based
on 2026-04-15 positioning conversation.

---

## Cross-phase: Sustainability

Not tied to any single phase — ongoing practices, updated for the broader
audience:

- **Cut small releases often.** v0.1.1, v0.1.2, v0.2.1 — don't hoard
  changes. Release-plz is already set up; use it.
- **Answer every issue within a week.** With a broader audience, drift
  reports and format-contribution PRs will be the dominant issue traffic.
  Fast response is the feature.
- **Don't chase hosted-tool parity.** Langfuse has search-across-teams;
  agx never will. The moat is terminal-native + zero-instrumentation +
  multi-format + library-mode. Every phase reinforces that.
- **Keep the dep baseline honest.** Each new crate earns its place. Heavy
  deps (ONNX via `fastembed-rs`, OTel proto via `prost`, Python bindings
  via `pyo3`) live behind feature flags or in separate workspace crates
  that users opt into by installing.
- **Pair announcements with rgx.** Same-family terminal debuggers; the
  audience overlaps. Cross-link in READMEs, co-announce major releases.
- **Dogfood with the agent-CLI teams.** Claude Code, Codex, Gemini
  maintainers are natural allies who dogfood their own output. Offer
  `agx --debug-unknowns` output on format-drift issues as a way to make
  drift diagnosis a community-positive interaction.
- **Stay read-only as the default.** Every write-back feature (annotations,
  replay, redaction) is behind a flag or writes to a sibling file. The
  "viewer that never breaks your session files" property is load-bearing.
- **Performance is a feature, not polish.** After Phase 3, never regress
  the <1s large-session load target without an explicit tradeoff in
  CHANGELOG.

---

## When to rethink the roadmap

Triggers that should cause a roadmap revision:

1. **OTel GenAI semconv goes 1.0 or shifts major.** Phase 2 may need
   a fast-follow; the GenAI attribute names are the one brittle surface
   in agx's whole design.
2. **A competing terminal TUI ships with overlapping scope.** Reprioritize
   toward what it doesn't have (cost tracking, corpus analytics, RL
   export, library mode — pick whichever is the biggest gap).
3. **A major CLI deprecates its session file format.** Drop the parser,
   document migration, keep the code around for a release in case users
   have archives.
4. **MCP metadata takes off faster than expected.** Promote Phase 5.2 to
   its own phase; the render layer for MCP-rich tool calls might warrant
   more than a subplan.
5. **Python binding adoption dwarfs CLI adoption.** If `agx-core` on PyPI
   becomes the primary use case, shift gravity: Phase 7 work items move
   up, TUI-only work moves down.
6. **A framework or agent standard not on this roadmap becomes dominant.**
   Insert a parser phase ahead of Phase 8; don't wait for v1.0.

The roadmap is a prediction, not a contract. Revisit on every minor
release.
