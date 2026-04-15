# agx

Step-through debugger for your agent. Rust TUI app using ratatui + crossterm + serde. Narrow scope, deep engineering, terminal-native. Consumes Claude Code, Codex CLI, and Gemini CLI session files and renders them as a navigable color-coded timeline with bidirectional tool call ↔ tool result pairing regardless of source format.

## Quick Reference

```bash
cargo build --release                      # Build (release)
cargo test                                 # Run all tests
cargo clippy --all-targets -- -D warnings  # Lint (must pass clean)
cargo fmt --check                          # Format check
cargo fmt                                  # Format apply
cargo audit                                # Supply chain audit
./target/release/agx assets/sample_session.jsonl           # Run on Claude Code fixture
./target/release/agx assets/sample_codex_session.jsonl     # Run on Codex fixture
./target/release/agx assets/sample_gemini_session.json     # Run on Gemini fixture
./target/release/agx --summary assets/sample_session.jsonl # Non-interactive mode
```

## Architecture

```
src/
├── main.rs       # CLI entry point: clap + format dispatch + --summary branch
├── format.rs     # Format detection — returns ClaudeCode | Codex | Gemini
├── session.rs    # Claude Code JSONL parser (Entry enum + serde Deserialize)
├── codex.rs      # Codex CLI JSONL parser (response_item + function_call pairing)
├── gemini.rs     # Gemini CLI single-JSON parser (toolCall splitting)
├── timeline.rs   # Shared Step / StepKind + step helpers + count_from_steps
└── tui.rs        # ratatui TUI: two-pane layout, event loop, help overlay, TerminalGuard
```

### Key patterns

- **Format detection** (`src/format.rs`): reads the file content and inspects its shape. Single JSON object with `sessionId` and `messages` → Gemini. Otherwise JSONL; first non-empty line's `type` field is inspected. `session_meta`/`event_msg`/`response_item`/`turn_context` → Codex. Anything else → Claude Code. No file-extension sniffing — content decides.
- **Per-format parser modules**: Each of `session.rs`, `codex.rs`, `gemini.rs` owns its format-specific deserialize types. `session.rs` exposes a Claude Code `Entry` enum that `timeline::build()` walks. `codex.rs` and `gemini.rs` produce `Vec<Step>` directly with no shared intermediate enum. `main.rs` dispatches on the detected format.
- **Shared step helpers** (`timeline.rs`): `user_text_step`, `assistant_text_step`, `tool_use_step`, `tool_result_step`, `truncate`, `short_id`, `pretty_json`, and `count_from_steps` are `pub(crate)` so every parser produces visually identical timeline items and summary counts. `tool_use_step` takes a pre-formatted input string. `tool_result_step` takes optional name/input args so orphan results degrade gracefully to `(unknown)`.
- **Format-specific tool pairing**:
  - **Claude Code**: `tool_use_id` field on tool_result items links back to the originating `tool_use`. Two-pass map build in `timeline::build()`.
  - **Codex**: `call_id` field on `function_call` / `function_call_output` entries. Codex frequently batches multiple `function_call` entries before their outputs arrive; the `call_id` map handles this correctly.
  - **Gemini**: each `toolCall` object in a `gemini` message embeds both call input and result atomically (nested as `result[0].functionResponse.response.output`). agx splits one `toolCall` into a `tool_use` step + a `tool_result` step so the TUI shape matches the other two formats.
- **Parser graceful unknown handling** (Claude Code): `#[serde(other)]` on `Entry`, `UserContentItem`, `AssistantContentItem` variants so unknown entry types or schema drift degrade to `Other` instead of failing the parse. Codex and Gemini parsers use `serde_json::Value` internally for the payload so unknown fields are ignored without panicking.
- **Panic-safe terminal cleanup** (`src/tui.rs`): `TerminalGuard` implements `Drop` to unconditionally call `disable_raw_mode()` and leave the alt screen, even on panic. Prevents the terminal from being stuck in a broken state after a crash.
- **Non-interactive mode** (`src/main.rs`): `--summary` flag prints the format, step counts by kind, and first 20 step labels to stdout and exits without launching the TUI. Useful for scripts, CI, quick inspection, and pipe chains.
- **Single-pass truncate** (`src/timeline.rs`): custom `truncate()` helper replaces newlines with spaces and caps char count in one pass. Unicode-safe.

## Code Conventions

- **Formatting**: default rustfmt (`cargo fmt`)
- **Lints**: `cargo clippy --all-targets -- -D warnings` must pass clean. Pedantic clippy also clean except for two justified `#[allow]`s:
  - `dead_code` on serde fields parsed for future use (`parent_uuid` for tree-walking, `timestamp` for time-travel, `uuid`/`role` for role-aware rendering)
  - `too_many_lines` on `run_loop` in tui.rs — the render function is logically one operation per frame; splitting hurts readability
- **Tests**: unit tests inline via `#[cfg(test)] mod tests` in each module. Parser tests use `tempfile::NamedTempFile` to write synthetic content and pass paths to the `load()` function. Shared integration fixtures at `assets/sample_session.jsonl` (Claude Code), `assets/sample_codex_session.jsonl` (Codex), `assets/sample_gemini_session.json` (Gemini) — all Fibonacci-writing conversations in their native schemas, zero personal data.
- **Commits**: Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `build:`, `perf:`)
- **MSRV**: Rust 1.74 (edition 2024)
- **Errors**: `anyhow::Result` at crate boundary, serde errors wrapped with `.with_context()` for line-number context

## Common Tasks

**Add a new step kind**: Add variant to `StepKind` in `src/timeline.rs`. Handle it in `build()` (Claude Code path) and in `codex.rs` / `gemini.rs` (if applicable). Update `kind_color()` in `src/tui.rs` and the `detail_title` match. Add test coverage in each module's `tests`.

**Add a new keybinding**: Add match arm in `run_loop`'s event handler in `src/tui.rs`. Update the help overlay's `help_lines` vec to document it. For global shortcuts (help toggle, quit), place them before the main keybindings so they work from any state.

**Add a new TUI panel or overlay**: Define state fields on `App` (e.g. `show_help: bool`). Add toggle/action methods. Render conditionally in the `terminal.draw` closure. Use the `Clear` widget before overlay content to punch through the background. Add `#[allow(clippy::too_many_lines)]` if the draw closure grows past 100 lines.

**Support a new agent trace format**: Add a new parser module (e.g. `src/vercel_ai.rs`) with its own deserialize types. The parser's public entry point should be `pub fn load(path: &Path) -> Result<Vec<Step>>` — the same shape as `codex::load` and `gemini::load`. Extend `format::Format` enum and `format::detect` with the new variant. Extend `main.rs`'s match arm to dispatch to the new parser. Reuse the shared step helpers from `timeline.rs` so the new format's timeline looks identical to the others. Add a synthetic fixture under `assets/sample_<format>_session.*` and unit tests that parse it. Do not introduce a shared `Entry` enum across formats — each format keeps its own parser-local types.

**Regenerate supply chain audit**: `cargo audit`. If a new advisory appears in a transitive dep, first check whether the vulnerable code path is actually reachable from agx. If it's a build-time dep pulled in by a default feature you don't use, disable that feature (see the ratatui `default-features = false` treatment in `Cargo.toml` for precedent).

## Not to Do

- Do not add hosted components (web UI, cloud sync, telemetry). agx is terminal-native.
- Do not pull in heavy dependencies. Every new dep should justify its weight against the ~1100 LOC and 6-dep baseline.
- Do not suppress clippy warnings without a `#[allow]` + comment explaining why.
- Do not commit real session JSONL/JSON files as fixtures. Use the synthetic fixtures in `assets/` or add new synthetic ones following the same pattern — obviously-fake UUIDs, generic content, zero personal data.
- Do not unify the three parsers behind a shared `Entry` trait/enum "for cleanliness." Each format is different enough that unification would leak format-specific concerns into the shared type. Parsers produce `Vec<Step>` directly and the uniformity happens at the Step layer, not the Entry layer.
