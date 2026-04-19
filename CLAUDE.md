# agx

Step-through debugger for your agent. Rust TUI app using ratatui + crossterm + serde. Narrow scope, deep engineering, terminal-native. Consumes Claude Code, Codex CLI, Gemini CLI, OpenTelemetry GenAI JSON, and generic OpenAI-compatible conversation files and renders them as a navigable color-coded timeline with bidirectional tool call ↔ tool result pairing regardless of source format. Per-step token usage and USD cost estimates; exports to Markdown / HTML / JSON.

## Quick Reference

```bash
cargo build --release                                # Build (release, default features)
cargo build --release --features otel-proto          # Release build with binary OTLP support
cargo build --release --features embedding-search    # Release build with semantic search (fastembed / ONNX)
cargo build --release --features notifications       # Release build with --notify-on-error / --notify-on-idle support
cargo test                                           # Run all tests (feature-off path)
cargo test --features otel-proto                     # Run all tests (feature-on path — prost included)
cargo test --features embedding-search               # Run all tests (feature-on path — fastembed included)
cargo test --features notifications                  # Run all tests (feature-on path — notify-rust included)
cargo clippy --all-targets -- -D warnings            # Lint, default features
cargo clippy --all-targets --features otel-proto -- -D warnings  # Lint with feature on
cargo clippy --all-targets --features embedding-search -- -D warnings  # Lint with semantic feature on
cargo clippy --all-targets --features notifications -- -D warnings  # Lint with notifications feature on
cargo bench --bench agx_bench                        # Run criterion bench suite (parsers + aggregate + corpus)
cargo bench --bench agx_bench -- --save-baseline main  # Capture baseline before a perf-targeted refactor
cargo bench --bench agx_bench -- --baseline main     # Compare after-change run against saved baseline
cargo fmt --check                                    # Format check
cargo fmt                                            # Format apply
cargo audit                                          # Supply chain audit
./target/release/agx assets/sample_session.jsonl             # Claude Code fixture
./target/release/agx assets/sample_codex_session.jsonl       # Codex fixture
./target/release/agx assets/sample_gemini_session.json       # Gemini fixture
./target/release/agx assets/sample_generic_session.json      # OpenAI-compatible fixture
./target/release/agx assets/sample_otel_json_traces.json     # OTel GenAI JSON fixture
./target/release/agx --summary        assets/sample_session.jsonl  # Non-interactive summary with tokens + cost
./target/release/agx --export md      assets/sample_session.jsonl  # Transcript → stdout (md | html | json)
./target/release/agx --debug-unknowns assets/sample_session.jsonl  # Format-drift diagnostics to stderr
./target/release/agx --no-cost        assets/sample_session.jsonl  # Suppress cost estimates
```

## Architecture

```
src/
├── lib.rs              # Library shim re-exporting every module as `pub mod`; consumed by main.rs and benches/
├── main.rs             # CLI entry point: clap + format dispatch + --summary / --export / --diff branches
├── format.rs           # Format detection — returns ClaudeCode | Codex | Gemini | Generic | OtelJson
├── browser.rs          # Multi-session discovery + picker (scans ~/.claude, ~/.codex, ~/.gemini)
├── session.rs          # Claude Code JSONL parser (Entry enum + serde Deserialize + ClaudeUsage)
├── codex.rs            # Codex CLI JSONL parser (response_item + function_call pairing)
├── gemini.rs           # Gemini CLI single-JSON parser (toolCall splitting + usageMetadata)
├── generic.rs          # OpenAI-compatible conversation parser ({messages: [{role, content, tool_calls}]})
├── langchain.rs        # LangChain / LangSmith run-tree export parser (chain/chat_model/tool)
├── otel_json.rs        # OpenTelemetry GenAI JSON parser (OTLP-JSON envelope + gen_ai.* semconv)
├── otel_proto.rs       # Binary OTLP parser (.pb/.otlp) — feature-gated behind `otel-proto`
├── vercel_ai.rs        # Vercel AI SDK `generateText` / `streamText` result parser (camelCase tool fields, steps[])
├── loader.rs           # `load_session(path)` — format dispatch front door shared by single-session and corpus flows
├── corpus.rs           # `agx corpus <dir>` subcommand: parallel rayon parse, aggregate, filter, text/json output
├── corpus_tui.rs       # `agx corpus --tui <dir>` interactive two-pane TUI (list + detail, drills into per-session tui.rs)
├── diff_align.rs       # Phase 4.1 pure alignment: LCS over (kind, tool_name) → Vec<AlignRow> for session-to-session diff
├── diff_tui.rs         # `agx foo --diff bar --diff-tui` two-pane ratatui rendering of the alignment
├── slice.rs            # Phase 4.2 pure slicing: duration / range parsers + slice_steps applied in main before dispatch
├── annotations.rs      # Phase 4.3 per-step annotations; sidecar JSON under ~/.agx/notes/, atomic rename, fault-tolerant load
├── semantic.rs         # Phase 4.4 semantic search (`//query`); feature-gated `embedding-search`, stub when off
├── notify.rs           # Phase 5.3 desktop notifications for `--live` mode; feature-gated `notifications`, no-op when off
├── timeline.rs         # Shared Step / StepKind / Usage / SessionTotals + step helpers + compute_* functions
├── pricing.rs          # Per-model USD rate table + Step::cost_usd delegation target
├── export.rs           # Markdown / HTML / JSON transcript writers (String-returning, no I/O)
├── debug_unknowns.rs   # Per-format drift scanners for --debug-unknowns
└── tui.rs              # ratatui TUI: three-pane layout, event loop, help / stats overlays, TerminalGuard
```

### Key patterns

- **Format detection** (`src/format.rs`): reads the file as bytes, tries UTF-8 conversion. Non-UTF-8 content → `Format::OtelProto` (binary OTLP). If UTF-8 and a single JSON object: `resourceSpans` → OTel GenAI (JSON). `run_type` + `inputs`/`outputs` → LangChain / LangSmith. `finishReason` / `steps[].stepType` / camelCase `toolCallId` → Vercel AI SDK. `sessionId` + `messages` → Gemini. `messages` alone → Generic OpenAI-compatible. Otherwise JSONL; first non-empty line's `type` field is inspected. `session_meta`/`event_msg`/`response_item`/`turn_context` → Codex. Anything else → Claude Code. No file-extension sniffing — content decides.
- **Per-format parser modules**: Each of `session.rs`, `codex.rs`, `gemini.rs`, `generic.rs`, `otel_json.rs` owns its format-specific deserialize types. `session.rs` exposes a Claude Code `Entry` enum that `timeline::build()` walks. The other four produce `Vec<Step>` directly with no shared intermediate enum. `main.rs` dispatches on the detected format.
- **Shared step helpers** (`timeline.rs`): `user_text_step`, `assistant_text_step`, `tool_use_step`, `tool_result_step`, `truncate`, `short_id`, `pretty_json`, and `count_from_steps` are `pub(crate)` so every parser produces visually identical timeline items and summary counts. `tool_use_step` takes a pre-formatted input string. `tool_result_step` takes optional name/input args so orphan results degrade gracefully to `(unknown)`.
- **Usage + model attach convention** (`timeline::attach_usage_to_first`): assistant messages in every format carry a single usage counter for the whole message even though agx may emit multiple steps (text + tool_uses). The shared `Usage` struct and `attach_usage_to_first(steps, start, model, &usage)` helper attach `model` + tokens to the **first** step emitted from each assistant message / span. All five parsers use this same anchor so corpus-level sums (`timeline::compute_session_totals`) never double-count. When adding a new format parser, mirror this pattern.
- **Format-specific tool pairing**:
  - **Claude Code**: `tool_use_id` field on tool_result items links back to the originating `tool_use`. Two-pass map build in `timeline::build()`.
  - **Codex**: `call_id` field on `function_call` / `function_call_output` entries. Codex frequently batches multiple `function_call` entries before their outputs arrive; the `call_id` map handles this correctly.
  - **Gemini**: each `toolCall` object in a `gemini` message embeds both call input and result atomically (nested as `result[0].functionResponse.response.output`). agx splits one `toolCall` into a `tool_use` step + a `tool_result` step so the TUI shape matches the other two formats.
  - **Generic**: `tool_calls[]` on an assistant message pairs with subsequent `role: "tool"` messages via `tool_call_id`. System-role messages are dropped.
  - **LangChain / LangSmith**: runs form a tree via `child_runs` — flattened and sorted by `start_time` before emission. A `chat_model` run emits assistant text from `outputs.generations[0][0].message.data.content` plus `tool_use` steps from the same message's `tool_calls[]`. A subsequent `tool` run emits only `tool_result` when the prior step is a matching `tool_use` (same tool name); otherwise both `tool_use` + `tool_result` so standalone tool runs remain visible. The root `chain` contributes only the user turn (from `inputs.input` / fallbacks) to avoid the inner chat_models re-emitting it.
  - **Vercel AI SDK**: `steps[]` array when present is walked in order; `toolCalls[]` on a step emits `tool_use` steps (camelCase fields: `toolCallId` / `toolName` / `args`-as-object) and `toolResults[]` emits paired `tool_result` steps. Usage attaches per-step, not from the root aggregate (root is a sum-of-steps — falling back would double-count). All-zero usage on tool-result-only steps is treated as "no LLM call" so the step doesn't sprout misleading zero-token rows.
  - **OTel GenAI**: a span with `gen_ai.operation.name = "execute_tool"` emits `tool_use` + `tool_result` together from `gen_ai.tool.name` / `.call.id` / `.call.arguments` / `.call.result`. LLM spans (`chat`, `text_completion`, `generate_content`) walk `gen_ai.prompt.{N}.role/.content` and `gen_ai.completion.{N}.role/.content` in numeric order. Non-GenAI spans (generic HTTP/DB) are ignored. Spans across ResourceSpans/ScopeSpans boundaries are sorted by `startTimeUnixNano`. The binary OTLP parser (`otel_proto.rs`) decodes the same logical structure with `prost` and reuses `otel_json::append_span` for the actual span → Step conversion.
- **Binary OTLP feature gate** (`otel_proto.rs`): the `otel-proto` Cargo feature is off by default. When on, the module compiles a minimal hand-written prost schema (`TracesData` / `ResourceSpans` / `ScopeSpans` / `Span` / `KeyValue` / `AnyValue`) covering only the fields agx reads — intentionally lighter than pulling the full `opentelemetry-proto` crate. When off, `load()` returns a helpful error directing the user to rebuild with the flag. Format detection always routes non-UTF-8 files to `Format::OtelProto` so the failure mode surfaces at dispatch, not deep in serde.
- **Notifications feature gate** (`notify.rs`): the `notifications` Cargo feature is off by default. The module exposes two functions — `notify::error(label)` and `notify::idle(duration_s)`. Feature off: both return `Ok(())` without touching anything (no `notify-rust` compiled in). Feature on: thin wrappers over `notify_rust::Notification` that set `appname("agx")` and a short summary / body. TUI's `run_loop` owns the *when*: tracks `last_growth: Instant` and `idle_fired: bool` locals; on each reload that grows the file it snapshots newly-added steps *before* `reload_steps()` moves the vec, filters with `is_error_result`, and fires one notification per new-error step. Idle check runs every iteration (not just on reload) so the notification fires promptly once the threshold elapses. All calls are `.ok()`'d — OS notification failures never crash the TUI. Main.rs dispatch prints `notify::FEATURE_DISABLED_MESSAGE` to stderr when either `--notify-on-error` or `--notify-on-idle` is set on a feature-off build so the flag isn't silently a no-op.
- **Semantic search feature gate** (`semantic.rs`): the `embedding-search` Cargo feature is off by default. The module exposes one function — `semantic::rank(query, steps) → Option<Vec<usize>>`. Feature off: `rank` returns `None` immediately (no deps compiled), TUI reads `FEATURE_DISABLED_MESSAGE` into `status_msg` with a rebuild hint. Feature on: lazy-initialized `TextEmbedding` in a process-wide `OnceLock<Mutex<_>>`, embeds the query + each step's `label+detail`, cosine-ranks, drops matches below threshold 0.25, caps at `MAX_RESULTS=30`. TUI dispatch lives in `tui.rs::apply_search`: a leading `//` in the search prompt routes to `apply_semantic_search` which maps the original step indices into current filtered-view positions. On filter change, semantic matches are discarded rather than re-embedded (expensive — user re-runs `//query` to refresh). `fastembed` pulls `ort` + `tokenizers` + `hf-hub`; first call triggers a ~90MB MiniLM-L6-v2 download to `~/.cache/fastembed/` (fastembed's default path), no further network. Default binary budget is <5MB; measured 2.6MB on macOS-arm64 at time of shipping Phase 4.4.
- **Parser graceful unknown handling** (Claude Code): `#[serde(other)]` on `Entry`, `UserContentItem`, `AssistantContentItem` variants so unknown entry types or schema drift degrade to `Other` instead of failing the parse. Codex, Gemini, Generic, and OTel parsers use `serde_json::Value` internally for the payload so unknown fields are ignored without panicking.
- **Format-drift diagnostics** (`src/debug_unknowns.rs`): `--debug-unknowns` adds one `scan_<format>` per format. Each scanner walks the raw JSON/JSONL (second pass, zero runtime cost when the flag is off) and reports unknown top-level types, payload types, content-item types, or operation names — grouped with the first three line/span-index samples. Used in issue templates for format-drift reports.
- **Pricing + cost** (`src/pricing.rs`): a hand-curated `ModelPricing` table keyed by model name, case-insensitive exact match (no fuzzy family fallback — avoids silent wrong numbers on new variants). Returns `None` for unknown models or zero-token inputs rather than fabricating cost. `Step::cost_usd()` is a thin delegate. Every row has a `last_verified` string; a test asserts it's non-empty.
- **Export writers** (`src/export.rs`): `markdown(steps, totals, no_cost, annotations) → String`, `html(steps, totals, no_cost, annotations) → String`, `json(steps, totals, annotations) → Result<String>`. `annotations` is `Option<&Annotations>` — passing `None` yields the annotation-free output shape (same as before). When `Some(&ann)` and `!ann.is_empty()`, markdown emits a `> **note**: …` blockquote below each step's meta, HTML emits a magenta-bordered `<div class="note">`, and JSON adds a top-level `annotations` array of `{step_index, text, created_at_ms, updated_at_ms}`. Empty `Annotations` is treated as `None` so the common-case output stays small. HTML is self-contained with inline CSS, no JS, no external assets; step details and note text are both HTML-escaped. Markdown uses ASCII-only kind prefixes (`[user]`/`[asst]`/`[tool]`/`[result]`) — no emoji per the terminal-native principle. JSON is the reserved public programmatic interface, stable from the Phase 7 library-mode plan.
- **Annotations flow** (`src/annotations.rs` + TUI + export + corpus): notes live in a sidecar JSON under `~/.agx/notes/<stem>-<fnv1a-hash8>.json`, keyed by a hand-rolled FNV-1a over the canonical session path (deterministic across processes — std's hashmap hasher has a per-process random seed). In the TUI, `a` opens an edit prompt for the current step (prefilled with the existing note for edit-in-place) and `A` opens a list overlay of every note with `j`/`k`/`Enter` to jump to its step — jumps to filter-hidden steps surface a status message rather than silently moving. Rows with a note show a magenta `*` prefix (precedence over the `║` batch marker) and the detail pane prepends a `[note: …]` meta line. Loaded lazily in `tui::run` and eagerly in `main.rs`'s `--export` branch and `corpus::load_parallel` so the same `Annotations::load_for(path)` entry point serves both interactive and non-interactive modes. Atomic writes via `fs::rename(2)` from a sibling `*.tmp`; a malformed notes file emits a stderr warning and returns empty rather than failing the TUI launch. `AGX_HOME` env override exists for tests.
- **Panic-safe terminal cleanup** (`src/tui.rs`): `TerminalGuard` implements `Drop` to unconditionally call `disable_raw_mode()` and leave the alt screen, even on panic. Prevents the terminal from being stuck in a broken state after a crash.
- **Non-interactive modes** (`src/main.rs`): `--summary` prints format + step counts + token/cost totals + first 20 step labels. `--export md|html|json` writes a transcript to stdout. `--diff <other>` prints a cross-format tool-usage comparison. `--debug-unknowns` reports drift to stderr alongside whichever mode is active. All are mutually compatible with `--no-cost` to suppress cost lines.
- **Multi-session browser** (`src/browser.rs`): when agx is launched with no path argument, `discover_all()` scans `~/.claude/projects/*/*.jsonl`, `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, and `~/.gemini/tmp/*/chats/session-*.json`, and prompts the user to pick one. Results sorted by mtime.
- **Corpus mode** (`src/corpus.rs`): `agx corpus <dir>` subcommand walks a directory tree, loads every file in parallel via `rayon`, silently skips non-sessions, and aggregates cross-session stats. Filters via `--filter model=X` / `--filter tool=Y` / `--filter errored` / `--filter annotated` are AND-combined. `annotated` loads the notes sidecar eagerly during the parallel scan (`Annotations::load_for(path).notes.len()`) and stores the count on `ParsedSession.annotation_count`; the count is also surfaced in `--jsonl` output as `annotation_count` so downstream tooling can filter / rank by it. Output modes: default text summary (totals + per-format + per-model + per-tool breakdowns + first 5 parse errors), `--json` for pretty-printed aggregate stats, `--jsonl` for line-delimited per-session JSON (stdout only; parse errors to stderr), and `--tui` for the interactive browser. `--fail-on-errored` is orthogonal — exits nonzero when any parse error or tool-error result is present across the corpus, for CI gates. The parallel walk uses `rayon::par_iter` and can be forced serial via `AGX_CORPUS_SERIAL=1` for debugging.
- **Single-pass truncate** (`src/timeline.rs`): custom `truncate()` helper replaces newlines with spaces and caps char count in one pass. Unicode-safe.
- **Line-streaming for JSONL parsers** (`src/session.rs`, `src/codex.rs`): both use `BufReader::lines()` rather than `read_to_string` + `.lines()`, so a multi-megabyte Claude Code / Codex JSONL never materializes as a single `String`. Peak working set during load is bounded by the longest single line (typically a few KB). Line-number context is preserved for format-drift error messages. Gemini / Generic / LangChain / Vercel / OTel-JSON parsers still use `read_to_string` because those formats are single-JSON-object files where streaming gains nothing.
- **`--bench` flag**: hidden diagnostic flag (both on the top-level CLI and on `agx corpus`). When set, agx prints load / walk / aggregate timings to stderr after the main output. Keeps stdout pipeable. Use when filing performance-regression reports.

## Stepwise suite context

agx is the read-only middle tool in a three-tool suite — **stepwise** —
under the `brevity1swos` GitHub org. Shared UX and integration contracts
live in [docs/suite-conventions.md](docs/suite-conventions.md), maintained
**verbatim** against the copies in `rgx/docs/` and `sift/docs/`. When
editing that file in agx, the change must also land in rgx and sift in
the same release cycle; divergence is a smell, never a feature.

Roles, worth internalizing before changing public surfaces:

- **rgx** — regex debugger. Owns regex UX; don't grow regex-authoring
  features in agx.
- **agx** (this project) — session timeline viewer. Read-only by default.
  Owns timeline / corpus / cost / semantic-search UX.
- **sift** — AI write review gate (writable sibling). Consumes agx's
  `--export json` over subprocess. Missing from the tree is fine; agx
  never imports or requires sift.

Two contracts that sift depends on and therefore need extra care during
refactors:

- **`agx --export json <session>`** — stable schema `{totals, steps,
  annotations?}`. Field renames or removals are breaking changes that
  require a minor-version bump and a note in the cross-tool compatibility
  table in README.
- **`agx --jump-to <session>:<step>`** — planned for Phase 5.5. Sift's
  `t`-keybind Timeline-jump targets this flag; land the semantics
  before sift ships the deep integration.

Cross-tool integrations follow suite-conventions §6: feature-detect at
runtime (never require a sibling at build time), silent degrade with a
status-bar hint when a sibling is missing, subprocess boundary only (no
shared Rust crates), named integrations (*Timeline jump*, *Policy debug*,
*Regex lens*), one-way coupling (consumers know producers, producers
never know their consumers). agx is a producer for sift and never a
consumer of sift.

## Code Conventions

- **Formatting**: default rustfmt (`cargo fmt`)
- **Lints**: `cargo clippy --all-targets -- -D warnings` must pass clean. Pedantic clippy also clean except for two justified `#[allow]`s:
  - `dead_code` on serde fields parsed for future use (`parent_uuid` for tree-walking, `timestamp` for time-travel, `uuid`/`role` for role-aware rendering)
  - `too_many_lines` on `run_loop` in tui.rs — the render function is logically one operation per frame; splitting hurts readability
- **Tests**: unit tests inline via `#[cfg(test)] mod tests` in each module. Parser tests use `tempfile::NamedTempFile` to write synthetic content and pass paths to the `load()` function. Shared integration fixtures at `assets/sample_session.jsonl` (Claude Code, enriched with usage), `assets/sample_codex_session.jsonl` (Codex), `assets/sample_gemini_session.json` (Gemini), `assets/sample_generic_session.json` (generic OpenAI), `assets/sample_otel_json_traces.json` (OTel GenAI) — zero personal data. End-to-end integration tests in `tests/summary_test.rs` (CLI output snapshots) and `tests/corpus_test.rs` (no-op scaffold for anonymized real-world fixtures under `tests/corpus/`).
- **Commits**: Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `build:`, `perf:`)
- **MSRV**: Rust 1.85 (required by edition 2024)
- **Errors**: `anyhow::Result` at crate boundary, serde errors wrapped with `.with_context()` for line-number context

## Common Tasks

**Add a new step kind**: Add variant to `StepKind` in `src/timeline.rs`. Handle it in `build()` (Claude Code path) and in `codex.rs` / `gemini.rs` (if applicable). Update `kind_color()` in `src/tui.rs` and the `detail_title` match. Add test coverage in each module's `tests`.

**Add a new keybinding**: Add match arm in `run_loop`'s event handler in `src/tui.rs`. Update the help overlay's `help_lines` vec to document it. For global shortcuts (help toggle, quit), place them before the main keybindings so they work from any state.

**Add a new TUI panel or overlay**: Define state fields on `App` (e.g. `show_help: bool`). Add toggle/action methods. Render conditionally in the `terminal.draw` closure. Use the `Clear` widget before overlay content to punch through the background. Add `#[allow(clippy::too_many_lines)]` if the draw closure grows past 100 lines.

**Support a new agent trace format**: Add a new parser module (e.g. `src/vercel_ai.rs`) with its own deserialize types. The parser's public entry point should be `pub fn load(path: &Path) -> Result<Vec<Step>>` — the same shape as `codex::load`, `gemini::load`, `generic::load`, and `otel_json::load`. Extend `format::Format` enum, `format::detect` with the new variant, and the match arms in `main.rs::load_session`, `browser.rs` (format tag), and `debug_unknowns.rs::scan` (drift scanner). Reuse the shared step helpers from `timeline.rs` so the new format's timeline looks identical to the others. For any format with usage/token data, extract a `Usage` and call `attach_usage_to_first` so cost aggregation stays correct. Add a synthetic fixture under `assets/sample_<format>_session.*` and unit tests that parse it. Do not introduce a shared `Entry` enum across formats — each format keeps its own parser-local types.

**Add a new model to the pricing table**: Edit `src/pricing.rs` and add a `ModelPricing` row to the `PRICES` slice. Set `input_per_mtoken` / `output_per_mtoken` from the provider's public pricing page; set `cache_read_per_mtoken` / `cache_create_per_mtoken` to `Some(rate)` only when the provider charges a separate cache rate (leave `None` to fall back to the input rate). Set `last_verified` to today's date. The `no_duplicate_model_names` and `every_entry_has_last_verified_date` tests will guard the row automatically.

**Add an export format**: Add a new writer function to `src/export.rs` following the `markdown` / `html` / `json` signature shape (takes steps + totals + `no_cost`, returns `String` or `Result<String>`; no I/O). Add a new `ExportFormat` enum variant in `main.rs` and a match arm in the `--export` dispatch. Add a unit test covering round-trip or structural invariants (e.g. no-JS for HTML, section-per-step for markdown). Keep ASCII-only prefixes — no emoji in exported output per the terminal-native principle.

**Regenerate supply chain audit**: `cargo audit`. If a new advisory appears in a transitive dep, first check whether the vulnerable code path is actually reachable from agx. If it's a build-time dep pulled in by a default feature you don't use, disable that feature (see the ratatui `default-features = false` treatment in `Cargo.toml` for precedent).

## Not to Do

- Do not add hosted components (web UI, cloud sync, telemetry). agx is terminal-native.
- Do not pull in heavy dependencies. Every new dep should justify its weight against the current ~6,500 LOC / 8-dep baseline. Heavy optional crates (e.g. `prost`, `opentelemetry-proto`, `fastembed-rs`, `pyo3`) must sit behind a Cargo feature flag that's off by default.
- Do not suppress clippy warnings without a `#[allow]` + comment explaining why.
- Do not commit real session JSONL/JSON files as fixtures. Use the synthetic fixtures in `assets/` or add new synthetic ones following the same pattern — obviously-fake UUIDs, generic content, zero personal data.
- Do not unify the three parsers behind a shared `Entry` trait/enum "for cleanliness." Each format is different enough that unification would leak format-specific concerns into the shared type. Parsers produce `Vec<Step>` directly and the uniformity happens at the Step layer, not the Entry layer.
- Do not let `docs/suite-conventions.md` drift from the copies in rgx and sift. The doc is load-bearing for cross-tool UX and is maintained by discipline, not CI. If you change something in agx that implies a conventions update, propagate the same change to the other two repos in the same release cycle — or revert the agx change and open a conventions-first discussion.
- Do not reach into sift. agx is the producer side of the agx ↔ sift contract; sift consumes agx's `--export json` and (future) `--jump-to` CLI, but agx never spawns sift, never reads sift's `.sift/` directory, and never renders sift's ledger state in the timeline. One-way coupling per suite-conventions §6.5.
- Do not add features that duplicate sift's review gate or rgx's regex authoring surface. The stepwise thesis (human oversight without efficiency cost) relies on each tool staying narrow. If agx starts showing writable-review UI or regex-authoring widgets, it's crept into the sibling's domain — defer instead.
