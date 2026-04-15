# Contributing to agx

Thanks for considering a contribution. agx is a small project — every PR
gets read carefully and most get merged.

## Quick start

```bash
git clone https://github.com/brevity1swos/agx.git
cd agx
cargo build
cargo test
```

Before opening a PR:

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

These three must pass cleanly. CI will reject PRs that don't.

## What kind of contributions help most

In rough priority order:

1. **Anonymized session fixtures** for formats that already exist (Claude
   Code, Codex, Gemini, generic). The more variety we have in
   `tests/corpus/`, the harder it is for format drift to break things
   silently. See "Contributing fixtures" below.
2. **New format parsers** (Aider, Cline, Cursor, Windsurf, etc). See
   "Adding a new agent trace format" below.
3. **Bug fixes with a reproduction.** A failing test case that shows the
   bug is worth more than a fix without one.
4. **TUI polish** — keybinding tweaks, layout improvements, color choices.
5. **Documentation fixes** — typos, outdated examples, missing keybindings.

What we generally don't want:

- Hosted components (telemetry, cloud sync, web UI). agx is terminal-native
  by design — see the "Not to Do" section in CLAUDE.md.
- Heavy new dependencies. Each new crate has to justify its weight against
  the current ~8-dep baseline.
- Refactors-for-cleanliness without a concrete user-visible benefit.
- Speculative architecture changes (e.g. unifying parsers behind a shared
  Entry trait — explicitly avoided, see CLAUDE.md).

## Adding a new agent trace format

The architecture is built for this — each format is a self-contained
parser module that produces `Vec<Step>`. No shared base class, no
unification.

Step-by-step:

1. Create `src/<format>.rs` with format-specific deserialize types
   (use `serde_json::Value` for fields you don't care about).
2. Define `pub fn load(path: &Path) -> Result<Vec<Step>>`. This is the
   only required public function — same shape as `codex::load` and
   `gemini::load`.
3. Build steps via the shared helpers in `timeline.rs`:
   `user_text_step`, `assistant_text_step`, `tool_use_step`,
   `tool_result_step`. Don't define your own Step constructors — uniformity
   happens at the `Step` level so the TUI renders every format identically.
4. Pair tool calls with their results inside your parser. Each format does
   this differently (Claude Code's `tool_use_id`, Codex's `call_id`,
   Gemini's atomic `toolCall`); your parser owns the pairing logic.
5. Add a variant to `Format` in `src/format.rs` and extend `format::detect`
   with a content-shape check that identifies your format unambiguously.
   Detection is by JSON shape, **not** file extension.
6. Extend `main.rs::load_session` with a match arm dispatching to your new
   parser.
7. Add a synthetic fixture under `assets/sample_<format>_session.<ext>`
   following the same pattern as the others — obviously-fake UUIDs,
   generic content, **zero personal data**.
8. Add unit tests in your module's `mod tests` block. Use
   `tempfile::NamedTempFile` to write synthetic content and pass paths to
   `load()`.

## Contributing fixtures

agx's parsers degrade gracefully on unknown fields (`#[serde(other)]`),
but real-world session files surface edge cases that synthetic fixtures
miss. Anonymized real fixtures are gold.

Before contributing a fixture:

1. **Anonymize.** Replace absolute paths with `/tmp/<placeholder>`. Strip
   email addresses, API keys, tokens, and any company-specific identifiers
   via search/replace. Replace usernames with `user`. Inspect the file
   line-by-line — `grep` is not enough.
2. **Verify.** `agx --summary <your-fixture>` should produce sensible
   output. `agx --debug-unknowns <your-fixture>` should not list any
   surprising entry types (or if it does, that's a great PR — it's
   format drift we should know about).
3. **Place** the file under `tests/corpus/<format>/` with a descriptive
   name like `claude_code_branching_session.jsonl` or
   `codex_with_reasoning_blocks.jsonl`.
4. **Commit message:** include what's interesting about the fixture —
   "first fixture with parallel tool calls" or "covers Codex CLI 0.40+
   reasoning entries".

The integration test in `tests/corpus_test.rs` will pick up your file
automatically and assert it parses without error.

## Reporting format drift

If a CLI you use has changed its session format and agx now misparses or
misses content, please open an issue with the
`format_drift` template. We need:

- Which CLI and which version
- The first 10–20 lines of an affected session file (anonymized)
- What agx shows vs. what you expected

Format drift PRs are highest priority — they keep agx working for everyone.

## Code conventions

- **Formatting:** default rustfmt (`cargo fmt`). No project-specific
  `rustfmt.toml`.
- **Lints:** `cargo clippy --all-targets -- -D warnings` must be clean.
  Pedantic clippy is also clean except for two justified `#[allow]`s
  documented in CLAUDE.md.
- **Tests:** unit tests inline via `#[cfg(test)] mod tests` in each module.
  Integration tests in `tests/`.
- **Errors:** `anyhow::Result` at crate boundary, `with_context()` at
  serde error sites for line-number context.
- **Commits:** Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`,
  `test:`, `chore:`, `build:`, `perf:`).
- **MSRV:** Rust 1.74 (edition 2024). Don't bump without discussion.

See CLAUDE.md for the full architecture map and "Common Tasks" recipes.

## License

By contributing, you agree your contributions are dual-licensed under
MIT OR Apache-2.0, matching the project's license.
