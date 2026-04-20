# Adding a new format parser

This is the checklist for shipping a new agent-trace format in agx.
Audience: contributors who want to add support for Aider, Windsurf,
Zed Assistant, Cline, Continue, or any future agent CLI that writes
session data locally.

Every shipped format today follows this pattern. The 8 existing
parsers (`session`, `codex`, `gemini`, `generic`, `langchain`,
`otel_json`, `otel_proto`, `vercel_ai`) are all faithful to the
rules below — read any of them as a reference before starting.

## Before writing code

1. **Confirm the format writes locally.** agx only consumes
   at-rest session files; hosted-only tools (Cursor without
   `.cursor/` logs, Windsurf before v1.x, etc.) don't fit.
2. **Get three real-world sample files from the target CLI.**
   Anonymize them. The parser design will be wrong if you only
   have one sample.
3. **Confirm there's a deterministic detection signal.** A single
   bytes/line pattern that unambiguously identifies this format.
   No file-extension sniffing. If the format overlaps with an
   existing one, document the disambiguation rule.
4. **Check stability.** If the upstream CLI changes its session
   shape every minor release, parser maintenance will dominate.
   Either commit to keeping up (add a `--debug-unknowns` scanner
   from day one) or skip the format.

## The 12-step checklist

### 1. Create the parser module

`crates/agx-core/src/<format>.rs`. Public entry point:

```rust
pub fn load(path: &Path) -> Result<Vec<Step>> { /* … */ }
```

Same shape as `codex::load`, `gemini::load`, etc. Use
`agx_core::timeline::*` helpers (`user_text_step`,
`assistant_text_step`, `tool_use_step`, `tool_result_step`,
`truncate`, `attach_usage_to_first`) so the TUI renders your
format identically to the others.

### 2. Use format-local deserialize types

Do NOT share a top-level `Entry` trait with other parsers. Every
existing format owns its deserialize types; format-specific
concerns stay inside the module. Produce `Vec<Step>` directly.

### 3. Declare the module in `lib.rs`

Add `pub mod <format>;` to `crates/agx-core/src/lib.rs`.

### 4. Add a `Format` variant

`crates/agx-core/src/format.rs` → `pub enum Format { … }`. Bump
the Display impl too so `agx --summary` shows a human-readable
name.

### 5. Wire up detection

`crates/agx-core/src/format.rs::detect` — add the content-based
check. Ordering matters when your format overlaps another.
Document the disambiguation rationale in a comment next to the
check (every existing entry does this).

### 6. Wire up the loader

`crates/agx-core/src/loader.rs::load_session` — add the match
arm that dispatches to `<format>::load(path)`.

### 7. Attach tokens / model via the shared anchor

Assistant messages in any format carry ONE usage counter for the
whole message. If your parser emits multiple steps per message
(text + tool_uses), call `timeline::attach_usage_to_first(steps,
start, model, &usage)` once per message. This is how every other
parser avoids double-counting in corpus sums.

### 8. Add a drift scanner

`crates/agx-core/src/debug_unknowns.rs` — write a `scan_<format>`
function. Walk the raw JSON a second time (zero cost when
`--debug-unknowns` isn't set) and report unknown entry types /
fields. Users filing format-drift issues paste this output.

### 9. Add a synthetic fixture

`assets/sample_<format>_session.<ext>` — obviously-fake UUIDs,
generic content, zero personal data. See existing fixtures for
the shape. Cover at least: one user message, one assistant
message, one tool_use / tool_result pair.

### 10. Write unit tests

Inside `crates/agx-core/src/<format>.rs` tests module:
- `parses_fixture_end_to_end` — loads the fixture, checks step
  count + first few labels.
- `usage_attaches_to_first_step` — if the format carries usage.
- `graceful_on_unknown_fields` — add a fake unknown field to a
  sample and assert parse still succeeds.

Use `tempfile::NamedTempFile` for inline content (already a
dev-dep on agx-core).

### 11. Update docs

- **README.md** — add the row to the Format Support table.
- **CHANGELOG.md** — `[Unreleased]` / Added section entry.
- **docs/eval-integration.md** — if your format carries
  format-specific metadata (e.g. MCP tool metadata) that should
  land in the export JSON schema, document it.

### 12. Verify

From repo root:
```
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass. CI will re-run them on the PR.

## Known formats waiting for a parser

These are formats worth adding but not yet shipped. If you own
one of these, the checklist above is the full plan.

### Aider

- **Location**: `$PROJECT/.aider.chat.history.md` +
  `$PROJECT/.aider.llm.history`.
- **Shape**: the `.md` file is Markdown transcript (not JSONL);
  the `.llm.history` appears closer to a JSONL message log. The
  latter is the better parse target — structured role/content
  pairs.
- **Detection**: `.md` starts with `# aider chat started at …`;
  JSONL variant starts with a JSON line containing LLM
  request/response. Prefer the JSONL variant for detection —
  better-structured.
- **Challenge**: Markdown parsing is inherently lossy; extracting
  tool calls from the human-formatted transcript needs careful
  regex work. Start with `.llm.history` if that file exists.
- **Useful reference**: https://aider.chat/docs/ (FAQ + usage pages).

### Windsurf

- **Location**: varies by version; recent versions write to
  `~/Library/Application Support/Windsurf/…` (macOS) or
  `~/.config/Windsurf/…` (Linux) in workspace-scoped SQLite or
  JSON.
- **Shape**: hosted-by-default; on-disk format not stable yet.
  Confirm current state before committing to support.

### Zed Assistant

- **Location**: `~/Library/Application Support/Zed/db/…`
  (SQLite).
- **Shape**: SQLite database. agx's parsers today all target
  JSON/JSONL; adding SQLite would require `rusqlite` behind a
  feature flag (pattern mirrors `otel-proto` — gate heavy deps).
- **Challenge**: First SQLite parser in agx; expect a ~1-week
  design pass for the feature-gate + schema discovery.

### Cline / Continue / Cursor

- Mostly hosted with limited on-disk state. Check current
  behavior before shipping a parser.

## When NOT to add a parser

- If the upstream CLI doesn't write session data locally.
- If the format changes more than once per month without a
  versioning scheme.
- If the format is proprietary SQLite with no stable schema
  guarantee.
- If fewer than 3 real-world samples are available.

In those cases, better options: contribute to the upstream tool
to add an open-export flag, or write a one-shot converter from
their format into an existing agx format (generic OpenAI-compatible
is the common target).

## Questions / drift reports

File at https://github.com/brevity1swos/agx/issues with the
`format-drift` label. Include `agx --debug-unknowns <session>`
output on anything that looks close-but-wrong.
