# agx-core

Pure, TUI-free parsers + timeline model for the [agx](https://github.com/brevity1swos/agx)
step-through debugger.

This crate ships the format parsers, the shared `Step` model, cost /
pricing tables, corpus aggregation, export writers, annotation
storage, and the credential / PII scanner. The TUI layer lives in the
top-level `agx` binary crate, which consumes this library.

Use this crate when you want to drive agx's parsers programmatically
without pulling in ratatui, crossterm, or arboard — eval harnesses,
custom CI guards, lightweight dashboards, Python / WASM bindings.

## Quick start

```rust
use agx_core::loader::load_session;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let steps = load_session(Path::new("session.jsonl"))?;
    for step in &steps {
        println!("{:?} {}", step.kind, step.label);
    }
    Ok(())
}
```

## Supported formats

- **Claude Code** JSONL (`~/.claude/projects/**/*.jsonl`)
- **Codex CLI** JSONL (`~/.codex/sessions/**/*.jsonl`)
- **Gemini CLI** single-JSON sessions
- **Generic OpenAI-compatible** conversations (`{messages: [...]}`)
- **LangChain / LangSmith** run-tree exports
- **Vercel AI SDK** `generateText` / `streamText` result objects
- **OpenTelemetry GenAI** JSON traces (`resourceSpans` + `gen_ai.*`)
- **OpenTelemetry GenAI** binary protobuf (feature-gated, enable `otel-proto`)

Format detection is content-based (no file-extension heuristic); see
[`format::detect`].

## Feature flags

- `otel-proto` — binary OTLP (.pb / .otlp) parser via `prost`. Off
  by default; adds ~500KB of transitive deps.
- `embedding-search` — semantic search via `fastembed`. Off by
  default; the runtime pulls a ~90MB MiniLM model on first use.
- `notifications` — OS desktop notifications via `notify-rust`.
  Off by default; platform-native deps.

## Stability

Public API tracks the [stepwise-suite conventions](https://github.com/brevity1swos/agx/blob/main/docs/suite-conventions.md)
§5. Schema-breaking changes to `timeline::Step` fields, `format::Format`
variants, or the export JSON shape require a minor-version bump and a
note in the cross-tool compatibility table.

## License

Dual-licensed under MIT OR Apache-2.0.
