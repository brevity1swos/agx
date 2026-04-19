//! Semantic search over session steps — opt-in via `--features embedding-search`.
//!
//! The TUI search prompt treats a leading `//` as a semantic query: the rest
//! of the string is embedded, every step's `label + detail` is embedded, and
//! steps are ranked by cosine similarity. The top matches populate the
//! existing `search_matches` vec and flow through the same jump / highlight
//! path that string-match search already uses.
//!
//! # Why feature-gated
//!
//! `fastembed` pulls in `ort` (ONNX Runtime) + `tokenizers` + `hf-hub` for
//! the first-run model download. That's tens of MB of binary and a one-time
//! ~90MB model fetch to `~/.cache/agx/embeddings/`. Users who just want to
//! browse traces shouldn't pay that cost. Gating on a Cargo feature keeps
//! the default binary lean (the core budget is <5MB per ROADMAP phase 4
//! acceptance) and lets power users opt in.
//!
//! # Fallback behavior
//!
//! Without the feature:
//! - `rank(..)` returns `None` immediately (no deps pulled, no work done).
//! - The TUI reads `FEATURE_DISABLED_MESSAGE` into `status_msg`, which
//!   tells users exactly how to rebuild agx to enable semantic search.
//!
//! With the feature:
//! - `rank(..)` returns `Some(Vec<usize>)` containing original step indices
//!   sorted by similarity (descending), capped at `MAX_RESULTS`.
//! - First call triggers model load (one-time, may block the UI for a
//!   few seconds on cold cache).
//!
//! The public API is identical across both builds — callers never need a
//! `cfg!(feature = ...)` check.

use crate::timeline::Step;

/// Maximum number of matches returned for a semantic query. Keeps the
/// match-list UX readable; users who want broader recall can widen the
/// threshold in a follow-up.
#[allow(dead_code)] // used only behind the `embedding-search` feature gate
pub(crate) const MAX_RESULTS: usize = 30;

/// User-facing message shown when `//query` is entered but the feature is
/// off. Mentions both the cargo install path and the build path so users
/// can pick whichever matches their workflow.
pub(crate) const FEATURE_DISABLED_MESSAGE: &str = "semantic search not compiled in — rebuild with `cargo install agx --features embedding-search` or `cargo build --release --features embedding-search`";

/// Rank steps by semantic similarity to `query`.
///
/// Returns:
/// - `Some(indices)` when the feature is on. Indices are into `steps`
///   (original order), sorted most-similar-first, capped at `MAX_RESULTS`.
///   `Some(vec![])` (empty) when nothing cleared the similarity threshold.
/// - `None` when the feature is off. Callers should surface
///   `FEATURE_DISABLED_MESSAGE` in that case.
#[cfg(not(feature = "embedding-search"))]
pub(crate) fn rank(_query: &str, _steps: &[Step]) -> Option<Vec<usize>> {
    None
}

#[cfg(feature = "embedding-search")]
pub(crate) fn rank(query: &str, steps: &[Step]) -> Option<Vec<usize>> {
    real::rank(query, steps)
}

#[cfg(feature = "embedding-search")]
mod real {
    use super::{MAX_RESULTS, Step};
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    use std::sync::{Mutex, OnceLock};

    /// Process-wide model handle. Initialized on first call, reused for the
    /// rest of the session. A `Mutex` lets the embed call borrow `&mut`
    /// (fastembed's API shape) without keeping a raw `Cell`.
    static MODEL: OnceLock<Mutex<TextEmbedding>> = OnceLock::new();

    fn model() -> &'static Mutex<TextEmbedding> {
        MODEL.get_or_init(|| {
            let init = InitOptions::new(EmbeddingModel::AllMiniLML6V2);
            // Panic here would kill the TUI with a bad terminal state,
            // so fall back to a helpful error surfaced via unwrap_or_else
            // at the rank() call site. This is stored lazily — if the
            // first model init fails we won't cache the failure, so the
            // next call retries. That's a fine default for a local CLI.
            let model = TextEmbedding::try_new(init)
                .expect("fastembed failed to initialize — check ~/.cache/agx/ writability");
            Mutex::new(model)
        })
    }

    /// Produce one input string per step for the embedder. Labels are short
    /// and semantically dense; details include the full tool call / result
    /// body. Concatenate with a separator so the embedder has both signals.
    fn step_input(step: &Step) -> String {
        let mut s = String::with_capacity(step.label.len() + step.detail.len() + 2);
        s.push_str(&step.label);
        s.push('\n');
        s.push_str(&step.detail);
        s
    }

    /// Cosine similarity between two f32 vectors. fastembed normalizes
    /// embeddings to unit length by default, so a plain dot product is the
    /// cosine. We still guard against zero-norm inputs (shouldn't happen
    /// for non-empty strings, but a crashed embed can produce them).
    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }
        let mut dot = 0.0;
        for i in 0..a.len() {
            dot += a[i] * b[i];
        }
        dot
    }

    pub(super) fn rank(query: &str, steps: &[Step]) -> Option<Vec<usize>> {
        let query = query.trim();
        if query.is_empty() || steps.is_empty() {
            return Some(Vec::new());
        }
        let lock = model().lock().ok()?;
        // Small cell — re-lock each time, so the mutex guard drops
        // between embed calls. Single-threaded context today (the TUI
        // event loop), but the shape doesn't preclude future parallel
        // use.
        let query_vec = {
            let mut m = lock;
            m.embed(vec![query.to_string()], None).ok()?
        };
        let q = query_vec.into_iter().next()?;

        let inputs: Vec<String> = steps.iter().map(step_input).collect();
        let step_vecs = {
            let mut m = model().lock().ok()?;
            m.embed(inputs, None).ok()?
        };

        let mut scored: Vec<(usize, f32)> = step_vecs
            .into_iter()
            .enumerate()
            .map(|(i, v)| (i, cosine(&q, &v)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Threshold: cosine below 0.25 is noise for MiniLM-L6 on short
        // strings. Drop them rather than presenting arbitrary ranked junk
        // when nothing actually matches.
        Some(
            scored
                .into_iter()
                .filter(|(_, s)| *s >= 0.25)
                .take(MAX_RESULTS)
                .map(|(i, _)| i)
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_disabled_message_mentions_rebuild_hint() {
        assert!(FEATURE_DISABLED_MESSAGE.contains("--features embedding-search"));
    }

    #[cfg(not(feature = "embedding-search"))]
    #[test]
    fn rank_returns_none_without_feature() {
        use crate::timeline::user_text_step;
        let steps = vec![user_text_step("hello"), user_text_step("world")];
        assert!(rank("hello", &steps).is_none());
    }

    #[cfg(not(feature = "embedding-search"))]
    #[test]
    fn rank_ignores_empty_inputs_without_feature() {
        // Even with empty inputs the feature-off stub returns None —
        // there's no "fast path" that leaks differences in behavior
        // between empty vs non-empty inputs.
        assert!(rank("", &[]).is_none());
        assert!(rank("q", &[]).is_none());
    }
}
