//! agx — step-through debugger CLI (TUI + binary).
//!
//! The pure parsers, timeline model, cost tables, corpus aggregation,
//! export writers, annotations, semantic search, PII scanner, and
//! notifications layer live in the companion [`agx-core`](../agx_core/index.html)
//! crate. This crate is a thin TUI + CLI wrapper around it.
//!
//! # Re-exports
//!
//! Everything public on `agx-core` is re-exported here, so existing
//! call sites that write `agx::timeline::Step`, `agx::loader::load_session`,
//! etc. keep working unchanged. The split is purely about publish
//! shape (Python / WASM / eval-harness consumers want the pure core
//! without ratatui) — not about rebinding the public surface.
//!
//! # TUI-only modules
//!
//! [`tui`], [`corpus_tui`], and [`diff_tui`] live here because they
//! depend on `ratatui` + `crossterm` + `arboard`, none of which
//! belong in `agx-core`.

pub use agx_core::{
    annotations, browser, codex, corpus, debug_unknowns, diff_align, export, format, gemini,
    generic, langchain, loader, notify, otel_json, otel_proto, pii, pricing, semantic, session,
    slice, timeline, vercel_ai,
};

pub mod corpus_tui;
pub mod diff_tui;
pub mod replay;
pub mod tui;
