//! agx — step-through debugger for AI agent traces.
//!
//! This crate exposes the parser, timeline, corpus, and pricing layers so
//! the bench harness under `benches/` (and future external consumers like
//! a Phase 7 `agx-core` split) can drive them directly. The CLI entry
//! point lives in `src/main.rs` and consumes this library.
//!
//! # Stability
//!
//! **Not a stable public API yet.** Expect breaking changes between
//! 0.x releases. Phase 7 of the roadmap will formalize a stable subset
//! when the library split happens.
//!
//! # Module groupings
//!
//! - Parsers: [`session`], [`codex`], [`gemini`], [`generic`],
//!   [`langchain`], [`otel_json`], [`otel_proto`], [`vercel_ai`] —
//!   each format-specific layer lands in its own module and produces
//!   `Vec<Step>` via shared helpers from [`timeline`].
//! - Shared model: [`timeline`] (Step / StepKind / Usage /
//!   SessionTotals), [`pricing`], [`format`] (detection), [`loader`]
//!   (format dispatch).
//! - Corpus + slicing: [`corpus`], [`slice`], [`diff_align`].
//! - Features: [`semantic`] (gated on `embedding-search`),
//!   [`otel_proto`] (gated on `otel-proto`).
//! - TUI + bin-only layers: [`tui`], [`browser`], [`corpus_tui`],
//!   [`diff_tui`], [`export`], [`debug_unknowns`], [`annotations`].
//!
//! The TUI-family modules are exposed here rather than kept `mod`-only
//! in the bin because they're useful for integration tests and any
//! future tooling that wants to drive the same rendering logic.

pub mod annotations;
pub mod browser;
pub mod codex;
pub mod corpus;
pub mod corpus_tui;
pub mod debug_unknowns;
pub mod diff_align;
pub mod diff_tui;
pub mod export;
pub mod format;
pub mod gemini;
pub mod generic;
pub mod langchain;
pub mod loader;
pub mod notify;
pub mod otel_json;
pub mod otel_proto;
pub mod pricing;
pub mod semantic;
pub mod session;
pub mod slice;
pub mod timeline;
pub mod tui;
pub mod vercel_ai;
