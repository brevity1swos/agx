# agx

Step-through debugger for AI agent execution traces. Rust TUI app using ratatui + crossterm + serde. Narrow scope, deep engineering, terminal-native. Consumes Claude Code session JSONL files and renders them as a navigable color-coded timeline with bidirectional tool_use ↔ tool_result pairing.

## Quick Reference

```bash
cargo build --release                      # Build (release)
cargo test                                 # Run all tests
cargo clippy --all-targets -- -D warnings  # Lint (must pass clean)
cargo fmt --check                          # Format check
cargo fmt                                  # Format apply
cargo audit                                # Supply chain audit
./target/release/agx assets/sample_session.jsonl           # Run on synthetic fixture
./target/release/agx --summary assets/sample_session.jsonl # Non-interactive mode
```

## Architecture

```
src/
├── main.rs       # CLI entry point: clap CLI definition, --summary branch, tui::run dispatch
├── session.rs    # Parser: serde Deserialize types + load() + count()
├── timeline.rs   # Step builder: flattens entries, pairs tool_use ↔ tool_result via two-pass lookup
└── tui.rs        # ratatui TUI: App state, render closure, event loop, help overlay, TerminalGuard
```

### Key patterns

- **Parser graceful unknown handling** (`src/session.rs`): `#[serde(other)]` on `Entry`, `UserContentItem`, `AssistantContentItem` means unknown entry types, content types, or schema drift degrade to an `Other` variant instead of failing the parse. New Claude Code entry types (attachments, system events, queue ops, file-history snapshots, permission-mode) all fall through cleanly.
- **Two-pass tool pairing** (`src/timeline.rs`): `collect_tool_meta()` walks all entries first to build a `HashMap<tool_use_id, (name, input_pretty)>`. The second pass (`build()`) uses the map to annotate each `tool_result` step with its originating tool's name and full input, so the detail view shows both call and response in one place. Falls back to `(unknown)` for orphan tool_results.
- **Panic-safe terminal cleanup** (`src/tui.rs`): `TerminalGuard` implements `Drop` to unconditionally call `disable_raw_mode()` and leave the alt screen, even on panic. Prevents the terminal from being stuck in a broken state after a crash.
- **Non-interactive mode** (`src/main.rs`): `--summary` flag prints the entry counts and timeline structure to stdout and exits without launching the TUI. Useful for scripts, CI, quick inspection, and pipe chains.
- **Single-pass truncate** (`src/timeline.rs`): custom `truncate()` helper replaces newlines with spaces and caps char count in one pass, then appends an ellipsis only if more chars remain. Unicode-safe.

## Code Conventions

- **Formatting**: default rustfmt (`cargo fmt`)
- **Lints**: `cargo clippy --all-targets -- -D warnings` must pass clean. Pedantic clippy also clean except for two justified `#[allow]`s:
  - `dead_code` on serde fields parsed for future use (`parent_uuid` for tree-walking, `timestamp` for time-travel, `uuid`/`role` for role-aware rendering)
  - `too_many_lines` on `run_loop` in tui.rs — the render function is logically one operation per frame; splitting hurts readability
- **Tests**: unit tests inline via `#[cfg(test)] mod tests` in each module. Shared integration fixture at `assets/sample_session.jsonl`.
- **Commits**: Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `build:`, `perf:`)
- **MSRV**: Rust 1.74 (edition 2024)
- **Errors**: `anyhow::Result` at crate boundary, serde errors wrapped with `.with_context()` for line number context

## Common Tasks

**Add a new step kind**: Add variant to `StepKind` in `src/timeline.rs`. Handle it in `build()` where the source entry type maps to steps. Update `kind_color()` in `src/tui.rs` and the `detail_title` match. Add test coverage in `timeline.rs::tests`.

**Add a new keybinding**: Add match arm in `run_loop`'s event handler in `src/tui.rs`. Update the help overlay's `help_lines` vec to document it. For global shortcuts (help toggle, quit), place them before the main keybindings so they work from any state.

**Add a new TUI panel or overlay**: Define state fields on `App` (e.g. `show_help: bool`). Add toggle/action methods. Render conditionally in the `terminal.draw` closure. Use the `Clear` widget before overlay content to punch through the background. Add `#[allow(clippy::too_many_lines)]` if the draw closure grows past 100 lines.

**Support a new agent trace format**: Add a new parser module (e.g. `src/vercel_ai.rs`) with its own serde types. Extend `main.rs` to detect the format (file extension, content sniffing, or explicit flag). Convert parsed entries into the shared `Entry` enum or introduce a common trait. Keep the existing Claude Code path working; new formats are additive.

**Regenerate supply chain audit**: `cargo audit`. If a new advisory appears in a transitive dep, first check whether the vulnerable code path is actually reachable from agx. If it's a build-time dep pulled in by a default feature you don't use, disable that feature (see the ratatui `default-features = false` treatment in `Cargo.toml` for precedent).

## Not to Do

- Do not broaden scope. Multi-format support, filtering, search, diffing, heatmap — these are explicitly deferred until after the v0 validation step lands.
- Do not add hosted components (web UI, cloud sync, telemetry). agx is terminal-native.
- Do not pull in heavy dependencies. Every new dep should justify its weight against the ~900 LOC and 6-dep baseline.
- Do not suppress clippy warnings without a `#[allow]` + comment explaining why.
- Do not commit real session JSONL files as fixtures. Use `assets/sample_session.jsonl` (synthetic, zero personal data) or generate new synthetic fixtures following the same pattern.
