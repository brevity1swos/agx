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

## Phase 2 — v0.3: OpenTelemetry GenAI + Framework Traces ✅ (shipped 2026-04-18)

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

**2.3 — LangChain / LangSmith export** ✅
- [x] `src/langchain.rs` parser for LangSmith's single-JSON "export run"
      shape — a tree of `Run` objects linked by `child_runs` and walked in
      chronological order via `start_time`
- [x] Run-type mapping:
      - `chain` at root → user text extracted from `inputs.input` /
        `.question` / `.query` / `.prompt`, with a fallback to the first
        `human` message in `inputs.messages[0]`
      - `chat_model` / `llm` → assistant text from
        `outputs.generations[0][0].message.data.content` + `tool_use`
        steps per `tool_calls` entry (modern tool-calling shape)
      - `tool` → paired `tool_result` (and `tool_use` if not already
        emitted by the prior chat_model) — avoids duplicating the call
        when the chat_model already announced it
      - `chain` / `retriever` / `parser` inner runs skipped (no render)
- [x] Token usage from `outputs.llm_output.token_usage` with
      `prompt_tokens` / `input_tokens` and `completion_tokens` /
      `output_tokens` fallback keys (handles OpenAI and Anthropic
      provider conventions). Model from `outputs.llm_output.model_name`
      with `extra.invocation_params.model_name` / `.model` fallback.
- [x] Detection: single JSON with top-level `run_type` + either `inputs`
      or `outputs` → `Format::Langchain`. Probed before Gemini / Generic
      in `format::detect`.
- [x] Synthetic fixture `assets/sample_langchain_export.json` —
      AgentExecutor → ChatOpenAI with tool_call → list_dir tool →
      ChatOpenAI final. Exercises tool_call↔tool_run pairing, usage
      attachment on both LLM runs, and the "don't double-emit tool_use"
      heuristic.
- [x] 9 unit tests cover end-to-end fixture, usage attachment on first
      vs last chat_model, root-input extraction from `.input` and
      `.messages`, standalone tool runs, invocation_params model
      fallback, and Anthropic-style `input_tokens` / `output_tokens`
      token-usage keys.
- [x] `--debug-unknowns` scans LangChain run trees recursively and
      reports unknown `run_type` values (known set: chain, llm,
      chat_model, tool); retriever/parser/prompt show up as drift signal.
- [x] Browser label: `[LChain]`
- [ ] **Deferred**: LangChain tracer v1 `.log` JSONL (`post` / `patch`
      event stream) and `astream_events` JSONL — different wire shape,
      wire up when a real fixture lands in `tests/corpus/langchain/`

**2.4 — Vercel AI SDK traces** ✅
- [x] `src/vercel_ai.rs` parser for `generateText` / `streamText` saved
      result objects (the shape most backends actually serialize to disk)
- [x] Walks `steps[]` when present (multi-step agent loops) — per-step
      usage + model attach to each step's first emitted timeline row;
      treats the root object as a single implicit step when `steps[]` is
      absent (plain single-turn `generateText` result)
- [x] camelCase tool-call fields: `toolCallId` / `toolName` / `args` as
      a JSON object (not a serialized string the way OpenAI does it);
      keeps agx faithful to the SDK's own wire shape
- [x] Token counters handle both AI SDK v4 (`promptTokens` /
      `completionTokens`) and v5+ (`inputTokens` / `outputTokens`)
      naming plus cache fields (`cachedInputTokens` /
      `cacheCreationInputTokens`). All-zero usage blocks are treated as
      "no LLM call on this step" so tool-result-only steps don't sprout
      misleading zero-token rows.
- [x] User-prompt extraction: `prompt` string → first `messages[]`
      entry with `role: "user"` → `content` as string, array of
      `{type, text}` parts, or message-level `parts` (v5 UI shape)
- [x] Model from `response.modelId` per step with root-level
      `response.modelId` / `modelId` / `model` fallback — but usage has
      NO root-level fallback since root usage is an aggregate and would
      double-count at the corpus level
- [x] Detection (in `format::detect`): three independent heuristics —
      `finishReason` at top level, `steps[0].stepType` present, or
      camelCase `toolCalls[0].toolCallId` — any one triggers. Probed
      before Generic so Vercel wins on its specific markers while plain
      OpenAI-compatible conversations still fall through.
- [x] Synthetic fixture `assets/sample_vercel_ai_session.json` —
      three-step agent: chat with tool_call → tool-result-only step with
      zero usage → continue step with final answer. Exercises every
      branch: user extraction, multi-step walking, zero-usage handling,
      usage anchor per step.
- [x] 10 unit tests cover end-to-end fixture, usage anchor convention,
      zero-usage suppression, single-step shape (no `steps[]`), v5
      `inputTokens`/`outputTokens` aliases, `prompt` string user
      extraction, content-array parts, tool_call args preservation.
- [x] `--debug-unknowns` scans `steps[].stepType` and reports unknown
      values (known: initial, continue, tool-result)
- [x] Browser label: `[Vercel]`
- [ ] **Deferred** (tracked in module docs): `useChat` / React UI
      message format with per-message `parts` arrays containing
      `tool-invocation` items — different idiom, will wire when a real
      fixture lands in `tests/corpus/vercel_ai/`

**2.5 — LlamaIndex + Pydantic AI quick wins** ✅ (no new parser needed)
- [x] LlamaIndex: inventory pass confirmed OTel is the default export
      path for every LlamaIndex instrumentation we could find
      (`llama-index-instrumentation-openinference`, `arize-phoenix`
      callbacks, Traceloop's OpenLLMetry SDK all emit OTel GenAI). Any
      trace from those paths lands in `otel_json.rs` / `otel_proto.rs`
      already. No native parser justified until a non-OTel fixture
      contribution shows up.
- [x] Pydantic AI: same story — the default `logfire` / OTel path
      covers the `agent.run_sync()` log shape. Native parser deferred
      to Phase 8 long-tail if a user files a fixture showing a non-OTel
      save format.
- [x] Decision documented in this roadmap entry so future-me doesn't
      re-litigate it without new evidence.

**2.6 — Detection reshuffle** ✅
- [x] `format::detect` order documented as a docstring at the top of
      the function with the full probe sequence:
      non-UTF-8 → OtelProto; single JSON {resourceSpans → OtelJson;
      run_type+inputs/outputs → Langchain; Vercel markers → VercelAi;
      sessionId+messages → Gemini; bare messages → Generic}; JSONL
      first-line type → Codex vs ClaudeCode.
- [x] Order preserves the "most specific first" rule — Vercel's
      `finishReason`/`stepType`/camelCase-toolCallId is checked
      before Gemini and Generic so AI SDK saves that also happen to
      contain `messages` don't misroute.
- [x] Content-based only; extension sniffing still forbidden.
- [x] Unit tests now cover every disambiguator: ClaudeCode by first
      line, Codex by session_meta and response_item, Gemini by
      sessionId+messages, Generic by bare messages, Langchain by
      run_type+inputs, Vercel by finishReason / stepType /
      camelCase-toolCallId, OtelJson by resourceSpans, OtelProto by
      non-UTF-8 bytes. Plus negative tests: Generic falls through when
      Vercel markers are absent; partial Langchain markers
      (run_type alone, no inputs/outputs) fall through to Generic.
- [x] All five other files that match on `Format` (main.rs dispatch,
      browser.rs tag, debug_unknowns.rs scan, otel_proto.rs gate,
      vercel_ai.rs detection helper) kept in sync with the enum.

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

## Phase 3 — v0.4: Corpus Analysis & Performance (core subplans shipped 2026-04-18)

**Goal:** Let a researcher or eval engineer point agx at 10,000 trajectories
and get answers in seconds. Two intertwined concerns — cross-session
analytics and the raw speed to make them tolerable — so they ship together.

**Why this third (moved up from v1.0):** The RL / eval audience cannot wait
for v1.0 to ask "across these 10k trajectories, which tools error most?"
Nous Research and similar trajectory-heavy users need this early or they'll
build around agx instead of with it.

### Subplans

**3.1 — `agx corpus` command** ✅
- [x] `agx corpus <dir>` subcommand (clap `Subcommand` derive) loads
      every session in a directory tree, format-auto-detected per file.
      Existing `agx <file>` flow untouched.
- [x] Parallel load via `rayon` (new dep, always-on — corpus is the
      flagship use case and the dep is small: `rayon-core` + two tiny
      `crossbeam-*` helpers, all pure-Rust). `AGX_CORPUS_SERIAL=1` env
      var forces serial loading for debugging.
- [x] Recursive directory walk is stdlib-only (`std::fs::read_dir` +
      depth limit); no `walkdir` dep. `--max-depth` defaults to 8 so
      Claude Code / Codex / Gemini canonical layouts are all reachable.
- [x] Silent skip on non-session files. `format::detect` failures are
      dropped without noise; detection-succeeds-but-load-fails surfaces
      as a real parse error. Binary files routed to OtelProto when the
      feature is off are treated as non-sessions (skipping avoids
      spamming "rebuild with --features" across every unrelated image).
- [x] Cross-session aggregates: file count, parse success / error /
      filtered-out counts; total steps; total tokens (in/out/cache_read/
      cache_create); total cost; per-model breakdown (session count,
      tokens, cost — sorted by session count); per-tool breakdown
      (use count, error count — sorted by use count); per-format
      breakdown (session count — sorted descending). All stable
      orderings with alphabetic tie-breaks for reproducibility.
- [x] `--filter model=<name>` / `--filter tool=<name>` / `--filter
      errored` post-filter predicates. Multiple `--filter`s AND-combined.
- [x] Output modes: text summary (default) and `--json` (full stats as
      pretty-printed JSON). Text output surfaces the first 5 parse
      errors with file paths so drift is visible at a glance.
- [x] 11 new unit tests cover Filter::parse for all three forms,
      Filter::matches on priced / tooled / errored sessions,
      `aggregate` sum/sort/empty behavior, per-model and per-tool
      ordering, filtered/errored counters, and tie-break stability.
- [x] End-to-end verified on `assets/`: 9 files scanned, 7 parsed
      (every shipped fixture), 0 errored, $0.0911 aggregate cost
      across the 3 priced-model sessions.
- [ ] Deferred to Phase 3.4: `--export csv`, `--json-lines` streaming,
      per-tool p50/p95 latency (requires per-tool duration tracking
      that agx's current `Step.duration_ms` doesn't provide), and
      sessions/day histogram (needs timestamp-binning infrastructure).

**3.2 — Performance pass** (in progress)
- [x] Line-streaming read for both JSONL parsers (`session.rs` and
      `codex.rs`) via `BufReader::lines()`. Previously both did
      `read_to_string` + `.lines()`, which materialized the full file
      as a single `String` before iterating — for a 50MB session that's
      50MB of string memory held just to walk over it. The new path
      keeps peak working set bounded by the longest single line
      (typically a few KB). Line-number context is preserved for
      format-drift error messages. Gemini / Generic / LangChain /
      Vercel / OTel-JSON parsers still use `read_to_string` because
      those formats are single-JSON-object files where streaming
      gains nothing.
- [x] `--bench` hidden flag prints load / walk / aggregate timings to
      stderr. Works on both the single-session flow
      (`agx --bench --summary foo.jsonl` → `[bench] load: 1.09ms
      (11 steps)`) and the corpus subcommand (`agx corpus --bench dir/`
      → `[bench] walk: 0.11ms (9 files)  load: 1.22ms (7 parsed, 0
      errored)  aggregate: 0.01ms  total: 1.34ms`). stdout stays
      clean for piping.
- [x] Memory-target note added to CLAUDE.md's "Key patterns" section
      so future contributors don't regress the streaming path back to
      `read_to_string`.
- [ ] `criterion` benchmarks (needs a `src/lib.rs` shim so the
      `benches/` crate can import parsers). Separate commit — touches
      workspace shape, better kept isolated.
- [ ] Tool-name interning in `App` + lazy detail expansion
      (`Step.detail` held as offset + length into the file buffer
      instead of an owned `String`). Separate commit — crosses the
      TUI / parser boundary and is the biggest win for very large
      sessions.
- [ ] Regression test for the ~3× file-size memory ceiling. Needs
      `criterion` + a large synthetic fixture — tracked alongside
      the benchmark commit.

**3.3 — Corpus TUI view** ✅
- [x] `agx corpus --tui <dir>` launches a two-pane TUI: session list on
      the left, selected-session summary on the right, corpus totals
      in a cyan header bar, keybinding hints in a gray footer.
- [x] `src/corpus_tui.rs` owns its raw-mode lifecycle via a
      `TerminalGuard`. Drill-in (Enter) tears down the corpus TUI, runs
      the existing per-session `tui::run`, then re-enters the corpus
      view when that exits. Clean because raw mode is process-global,
      not stackable.
- [x] Sort cycle via `s`: mtime ↓ → cost ↓ → errors ↓ → tokens ↓ →
      format/name → (wrap). Current mode shown in the header. Selected
      session's identity survives re-sorts — list cursor follows the
      session, not the row index.
- [x] Keybindings mirror the per-session TUI verbatim (j/k/g/G/
      Home/End/PgUp/PgDn navigation, ?/F1 help overlay, q/Esc quit)
      plus two corpus-specific additions (Enter drill-in, s sort).
- [x] `--tui` is `conflicts_with = "json"` at the clap level — the TUI
      owns the terminal, JSON needs stdout clean.
- [x] `mtime_secs` plumbed into `ParsedSession` so the default
      mtime-desc sort is meaningful; populated from `fs::metadata`
      during parallel load.
- [x] 9 new unit tests cover sort cycle ordering, mtime-desc with
      None at bottom, cost/errors/tokens-desc ordering, alphabetic
      tie-break, selection-survives-sort-cycle, and format-tag
      short-label trimming.
- [ ] **Deferred**: per-tool heatmap across sessions — this deserves a
      dedicated design pass (heatmap color palette in the corpus
      context isn't quite the same signal as the per-session one).
      Tracked as a Phase 3.3 extension.
- [ ] **Deferred**: in-TUI filter/search. The CLI `--filter` already
      covers the main use case; live filtering could come later.

**3.4 — Eval-loop integration** ✅
- [x] `agx corpus <dir> --jsonl` emits one JSON object per session to
      stdout (compact, line-delimited, not pretty-printed). Schema is a
      dedicated `SessionLine` struct — flat / stable / downstream-safe.
      Parse errors go to stderr so `--jsonl | jq` etc. don't see
      corrupted output. Named `--jsonl` to match the extension
      convention and avoid ambiguity with `--json` (the pretty-printed
      aggregate variant).
- [x] `agx corpus <dir> --fail-on-errored` exits with a nonzero status
      (code 1 via anyhow — simpler than carving out a dedicated 2)
      when any parse error OR any is_error_result tool_result is
      present in the corpus. Orthogonal to rendering mode: combines
      cleanly with `--json` / `--jsonl` / `--tui` / default text.
- [x] Clap-level `conflicts_with_all = ["json", "jsonl"]` on `--tui`
      and `conflicts_with = "json"` on `--jsonl` — prevents nonsensical
      "TUI owns terminal but also stdout-JSON" combinations at parse
      time rather than runtime.
- [x] End-to-end verified: `--jsonl` produces valid one-JSON-per-line
      output parseable by Python `json.loads`; `--fail-on-errored`
      exits 0 on a clean corpus (asserted via shell `$?`).
- [ ] **Deferred** (low priority): true streaming during parallel parse
      via a channel + print thread so `tail -f` actually shows lines as
      they complete parsing. Current implementation collects first, then
      prints — fine for small-to-medium corpora; upgrade if users ask.

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

## Phase 4 — v0.5: Diff, Search Depth, Annotations (in progress)

**Goal:** Turn agx from a viewer into an analysis tool. Real side-by-side
diff, deeper search, and notes that survive session edits.

### Subplans

**4.1 — Interactive side-by-side diff** ✅
- [x] Pure-algorithm alignment module (`src/diff_align.rs`) —
      longest-common-subsequence over a structural `Sig` (step kind +
      tool name), no TUI deps. O(N·M) DP with backtrack. 10 unit
      tests.
- [x] `AlignRow { left, right, kind }` with
      `AlignKind::{Match, Differ, LeftOnly, RightOnly}`.
- [x] Two-pane TUI (`src/diff_tui.rs`) renders the alignment with
      synchronized scrolling. The two ratatui `List`s share one
      `ListState` across both `render_stateful_widget` calls — panes
      are the same height (horizontal split), so ratatui's
      "keep-selected-visible" offset math produces identical offsets
      on both sides, scrolling the panes in lockstep for free.
- [x] Color coding per row: Match green (`=`), Differ yellow (`~`),
      LeftOnly red (`-`) on left + gray "(absent)" right, RightOnly
      green (`+`) on right + gray "(absent)" left. ASCII prefixes only.
- [x] Header shows both file labels with format + tokens + cost plus
      the alignment counts (N match · N differ · N only-A · N only-B).
      Footer shows key hints.
- [x] Navigation: j/k/g/G/Home/End/PgUp/PgDn mirrors per-session and
      corpus TUIs exactly. ?/F1 help overlay with color legend, q/Esc
      quit. Raw-mode owned via `TerminalGuard` (same pattern as the
      other TUIs).
- [x] `--diff-tui` CLI flag on top-level Cli. `requires = "diff"`
      enforces that `--diff <path>` must also be set; conflicts_with
      `--summary` / `--export` since those own stdout.
- [x] 6 unit tests on the TUI side cover align-kind counting,
      App::new selection state, navigation clamping, row_style
      LeftOnly/RightOnly asymmetry, and build_items gap-side
      "(absent)" sentinel.
- [ ] **Later extensions** (tracked as 4.1 follow-ups): `Tab` jumps
      to next unaligned-only row, `d` toggles inline diff of the
      selected row's detail, drill-in from a diff row into the
      single-session TUI on either side.

**4.2 — Jump-to-time + trim** ✅
- [x] `src/slice.rs` — pure parser + slicer module. Duration grammar
      supports `30s` / `5m` / `2h` / `1d`, compounds like `1h30m`,
      long-form units (`minutes`, `hours`, ...), case-insensitive,
      and a bare-integer-as-seconds convenience. 7 unit tests.
- [x] Range grammar: `start..end` (exclusive end, mirrors Rust's
      `Range<usize>`) with open-ended forms (`..500`, `100..`, `..`).
      Malformed / reversed ranges return `Result::Err` at parse time.
      6 unit tests.
- [x] `slice_steps` applies index + time filters in one pass.
      `warn_if_time_filter_ignored` keeps the core pure while giving
      users a stderr warning when they asked for `--after` / `--before`
      on a session without timestamps.
- [x] CLI flags: `--after <DURATION>`, `--before <DURATION>`,
      `--after-step <N>`, `--before-step <N>`, `--range <a..b>`. Clap-
      level `conflicts_with = "range"` prevents the step-scalars from
      combining with the range shorthand.
- [x] Time semantics: filters are relative to the *session's first
      step*, not wall-clock now. Unambiguous for archived sessions.
- [x] Bench-hint integration — when `--bench` is on, slicing prints
      `[bench] slice: before → after steps`.
- [x] TUI extension: `:@<duration>` command jumps to the first step
      at-or-after that offset from the session's first-step timestamp.
      Uses the same `slice::parse_duration_ms` parser so CLI and TUI
      speak the same grammar. Reports "no step timestamps" / "no step
      at-or-after +Xms" / "hidden by the active filter" cleanly.
      Help overlay updated with the new command. 4 unit tests.
- [x] End-to-end verified on the Claude Code fixture:
      `--range 2..6` trims to 4 steps, `--after 3s` trims to 7 steps,
      `--after 10h` trims to 0 steps.
- [ ] **Deferred**: absolute-time `:@HH:MM:SS` jump (ambiguous across
      days; would require a date prefix or day-of-session heuristic).
      `..=` inclusive-end range syntax (trivial add when asked for).

**4.3 — Annotations** (shipped end-to-end: MVP + overlay + export + corpus filter)
- [x] `a` in the TUI opens an annotation prompt for the selected
      step. Prefills with the existing note for edit-in-place, or
      opens blank for new notes. Enter upserts, empty text deletes,
      Esc discards.
- [x] Storage: `~/.agx/notes/<session-stem>-<fnv1a-hash8>.json`.
      Decided against the sibling `.agx/` + home-dir-fallback scheme
      — single location keeps retrieval logic trivial and is more
      portable across workstations where users mount session dirs
      read-only or from different machines. Override via `AGX_HOME`
      env var (used by the test suite).
- [x] Keyed by FNV-1a of the canonical path. Hand-rolled FNV keeps
      hashes deterministic across agx invocations (std's hashmap
      hasher has a random seed per process) and adds zero deps.
- [x] Atomic writes via temp-file + `rename(2)`. Corrupted notes
      files are reported to stderr and silently dropped rather than
      preventing the TUI from launching.
- [x] Rendered as a magenta `*` prefix on annotated rows in the
      timeline list. Takes precedence over the `║` batch marker
      when both apply (annotations are more load-bearing user
      signal than derived structure). Detail pane prepends a
      `[note: ...]` meta line.
- [x] Help overlay updated with the `a` keybinding and the color
      legend entry.
- [x] 12 unit tests for the annotations module (empty / upsert /
      trim / delete-on-empty / idempotent-identity / updated_at
      refresh / numeric-order iter / round-trip save+load /
      missing-file-tolerance / malformed-file-tolerance / filename
      format / hash determinism). Race-safe via a module-local
      `Mutex<()>` around `AGX_HOME` writes since `cargo test` runs
      in parallel by default.
- [x] `A` list-overlay showing all annotations, with `j`/`k` navigation
      and `Enter` to jump the main timeline cursor to the selected
      step. Esc (or any other key) closes. Reports
      "hidden by the active filter" via `status_msg` when the
      target step is filtered out, instead of silently moving
      somewhere else.
- [x] Export integration: `--export md` emits a blockquoted
      `> **note**: …` below the per-step meta; `--export html`
      renders a magenta-bordered `<div class="note">`;
      `--export json` adds an optional top-level `annotations`
      array of `{step_index, text, created_at_ms, updated_at_ms}`.
      All three omit the annotations section entirely when the
      session has no notes (keeps common-case output small).
- [x] `agx corpus --filter annotated` keeps sessions with ≥1 note.
      `ParsedSession.annotation_count` + `SessionLine.annotation_count`
      are loaded eagerly during the parallel scan and surfaced in
      `--jsonl` output for downstream tooling.

**4.4 — Semantic search (opt-in feature flag)** ✅ (shipped 2026-04-19)
- [x] `--features embedding-search` compile flag, default off. Cargo.toml
      adds an optional `fastembed = "5"` dep behind the feature.
- [x] `//query` prefix in the TUI search prompt triggers semantic lookup.
      The rest of the string is embedded; each step's `label + detail`
      is embedded; cosine similarity ranks matches; threshold 0.25
      drops noise; `MAX_RESULTS = 30` caps list length. Results flow
      through the existing `search_matches` vec so highlighting, jump-
      to-next (`n`), and jump-to-prev (`N`) work unchanged.
- [x] Uses `fastembed-rs` with `AllMiniLML6V2` as the default model.
      Lazy-initialized via `OnceLock<Mutex<TextEmbedding>>` so repeat
      queries don't re-load the model. First call downloads ~90MB to
      `~/.cache/fastembed/` (fastembed's default path); no further
      network activity ever.
- [x] Without the feature: the `//` dispatch in `tui::apply_search`
      surfaces `semantic::FEATURE_DISABLED_MESSAGE` via the status
      bar. The message tells the user both the `cargo install` and
      `cargo build` paths to enable the feature. No change to the
      default binary — verified at 2.6MB after Phase 4.4 shipped
      (budget: <5MB).
- [x] On filter change after a semantic search, the search is cleared
      rather than re-embedded. Re-running `//query` is cheap-enough
      and avoids a surprise multi-second block when filters toggle.
- [x] 6 new unit tests (3 in `semantic.rs` + 3 in `tui.rs`) cover
      feature-disabled path, message content, empty-query error,
      and the "don't clobber existing string-search" invariant.

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
