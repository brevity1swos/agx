//! agx-core — pure parsers, timeline model, and analytics for the
//! agx step-through-debugger CLI.
//!
//! This crate is the TUI-free half of the agx repo. It ships the
//! parsers for every supported agent-trace format (Claude Code,
//! Codex, Gemini, OpenAI-compatible, LangChain, Vercel AI SDK,
//! OpenTelemetry GenAI — JSON + optional binary), the shared
//! [`timeline::Step`] model, cost estimation, corpus aggregation,
//! export writers, annotation storage, and the PII scanner.
//!
//! The TUI layer lives in the top-level `agx` crate, which consumes
//! this library. Integrators who want to drive agx's parsers
//! programmatically — eval harnesses, custom CI guards, lightweight
//! dashboards — can depend on `agx-core` without pulling in ratatui /
//! crossterm / arboard.
//!
//! # Stability
//!
//! Public API tracks the stepwise-suite conventions in
//! [`docs/suite-conventions.md`](https://github.com/brevity1swos/agx/blob/main/docs/suite-conventions.md)
//! §5. Schema-breaking changes to [`timeline::Step`] field names,
//! [`format::Format`] variants, or the `--export json` shape
//! (mirrored here through [`export::json`]) require a minor-version
//! bump and a note in the main crate's README compat table. New
//! fields may appear; removals or renames are breaking.
//!
//! # Entry point
//!
//! ```no_run
//! use agx_core::loader::load_session;
//! # use std::path::Path;
//! # fn main() -> anyhow::Result<()> {
//! let steps = load_session(Path::new("session.jsonl"))?;
//! for step in &steps {
//!     println!("{:?} {}", step.kind, step.label);
//! }
//! # Ok(())
//! # }
//! ```

pub mod annotations;
pub mod browser;
pub mod codex;
pub mod corpus;
pub mod debug_unknowns;
pub mod diff_align;
pub mod export;
pub mod format;
pub mod gemini;
pub mod generic;
pub mod langchain;
pub mod loader;
pub mod notify;
pub mod otel_json;
pub mod otel_proto;
pub mod pii;
pub mod pricing;
pub mod semantic;
pub mod session;
pub mod slice;
pub mod timeline;
pub mod vercel_ai;
