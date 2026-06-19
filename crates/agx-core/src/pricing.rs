//! Per-model pricing lookup. Converts a `Step`'s token counters into a USD
//! cost estimate.
//!
//! Prices are hardcoded and WILL drift as providers change their rates. Each
//! entry carries a `last_verified` date so maintainers can audit staleness
//! without re-reading every source. See the comment at the top of the PRICES
//! array for the source-of-truth pages.
//!
//! When a model name is unknown, `cost_usd` returns `None` rather than
//! guessing — agx doesn't fabricate cost numbers.
//!
//! Cache pricing follows Anthropic's public model: cache reads are billed
//! at ~10% of the input rate, cache creation at ~125% of the input rate.
//! OpenAI and Google structure caching differently; for those providers
//! agx treats `cache_read` as a flat input-rate discount and
//! `cache_create` as zero until better data is available.
//!
//! `ModelPricing::last_verified` is audited by a dedicated test but not
//! read at runtime; its field-level `#[allow(dead_code)]` is the only
//! intentional allow in this module.

/// USD per 1M tokens. Small struct so adding a new model is one row.
#[derive(Debug, Clone, Copy)]
pub struct ModelPricing {
    pub name: &'static str,
    pub input_per_mtoken: f64,
    pub output_per_mtoken: f64,
    /// Set this when the provider charges a separate cache-read rate
    /// (e.g. Anthropic). When `None`, cache-read tokens are billed at the
    /// input rate.
    pub cache_read_per_mtoken: Option<f64>,
    /// Set this when the provider charges a separate cache-create rate.
    /// When `None`, cache-create tokens are billed at the input rate.
    pub cache_create_per_mtoken: Option<f64>,
    /// Last date a human verified this entry against the provider's pricing
    /// page. Present for audit; not used at runtime.
    #[allow(dead_code)]
    pub last_verified: &'static str,
}

// Source pages at time of last verification:
//   Anthropic: https://www.anthropic.com/pricing
//   OpenAI:    https://platform.openai.com/docs/pricing
//   Google:    https://ai.google.dev/gemini-api/docs/pricing
//
// Rates below are ESTIMATES. Treat cost output as a ballpark until a
// maintainer verifies against the current pricing page.
const PRICES: &[ModelPricing] = &[
    // --- Anthropic Claude 4.8 family ---
    ModelPricing {
        name: "claude-opus-4-8",
        input_per_mtoken: 15.0,
        output_per_mtoken: 75.0,
        cache_read_per_mtoken: Some(1.50),
        cache_create_per_mtoken: Some(18.75),
        last_verified: "2026-06-19 (estimate; unverified)",
    },
    // --- Anthropic Claude 4.6 family ---
    ModelPricing {
        name: "claude-opus-4-6",
        input_per_mtoken: 15.0,
        output_per_mtoken: 75.0,
        cache_read_per_mtoken: Some(1.50),
        cache_create_per_mtoken: Some(18.75),
        last_verified: "2026-04-15 (estimate; unverified)",
    },
    ModelPricing {
        name: "claude-sonnet-4-6",
        input_per_mtoken: 3.0,
        output_per_mtoken: 15.0,
        cache_read_per_mtoken: Some(0.30),
        cache_create_per_mtoken: Some(3.75),
        last_verified: "2026-04-15 (estimate; unverified)",
    },
    ModelPricing {
        name: "claude-haiku-4-5",
        input_per_mtoken: 1.0,
        output_per_mtoken: 5.0,
        cache_read_per_mtoken: Some(0.10),
        cache_create_per_mtoken: Some(1.25),
        last_verified: "2026-04-15 (estimate; unverified)",
    },
    // --- OpenAI ---
    ModelPricing {
        name: "gpt-5",
        input_per_mtoken: 10.0,
        output_per_mtoken: 30.0,
        cache_read_per_mtoken: Some(2.50),
        cache_create_per_mtoken: None,
        last_verified: "2026-04-15 (estimate; unverified)",
    },
    ModelPricing {
        name: "gpt-5-mini",
        input_per_mtoken: 0.5,
        output_per_mtoken: 2.0,
        cache_read_per_mtoken: Some(0.10),
        cache_create_per_mtoken: None,
        last_verified: "2026-04-15 (estimate; unverified)",
    },
    // --- Google Gemini ---
    ModelPricing {
        name: "gemini-2-5-pro",
        input_per_mtoken: 2.50,
        output_per_mtoken: 15.0,
        cache_read_per_mtoken: Some(0.625),
        cache_create_per_mtoken: None,
        last_verified: "2026-04-15 (estimate; unverified)",
    },
    ModelPricing {
        name: "gemini-2-5-flash",
        input_per_mtoken: 0.30,
        output_per_mtoken: 2.50,
        cache_read_per_mtoken: Some(0.075),
        cache_create_per_mtoken: None,
        last_verified: "2026-04-15 (estimate; unverified)",
    },
];

/// Look up pricing for a given model name. Returns `None` when the model is
/// not in the table. Uses case-insensitive exact match — no fuzzy matching,
/// no family fallback (avoids silent wrong numbers for new variants).
#[must_use]
pub fn lookup(model: &str) -> Option<&'static ModelPricing> {
    PRICES.iter().find(|p| p.name.eq_ignore_ascii_case(model))
}

/// Compute USD cost for a single step given its token counters and model.
/// Returns `None` when the model is unknown OR when there are no non-zero
/// token counters (nothing to cost).
#[must_use]
pub fn cost_usd(
    model: Option<&str>,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    cache_read: Option<u64>,
    cache_create: Option<u64>,
) -> Option<f64> {
    let pricing = lookup(model?)?;
    let has_any = [tokens_in, tokens_out, cache_read, cache_create]
        .iter()
        .any(|v| v.is_some_and(|n| n > 0));
    if !has_any {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let t_in = tokens_in.unwrap_or(0) as f64;
    #[allow(clippy::cast_precision_loss)]
    let t_out = tokens_out.unwrap_or(0) as f64;
    #[allow(clippy::cast_precision_loss)]
    let t_cr = cache_read.unwrap_or(0) as f64;
    #[allow(clippy::cast_precision_loss)]
    let t_cc = cache_create.unwrap_or(0) as f64;
    let cost = t_in * pricing.input_per_mtoken
        + t_out * pricing.output_per_mtoken
        + t_cr
            * pricing
                .cache_read_per_mtoken
                .unwrap_or(pricing.input_per_mtoken)
        + t_cc
            * pricing
                .cache_create_per_mtoken
                .unwrap_or(pricing.input_per_mtoken);
    Some(cost / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_known_model_case_insensitive() {
        assert!(lookup("claude-opus-4-6").is_some());
        assert!(lookup("Claude-Opus-4-6").is_some());
        assert!(lookup("CLAUDE-OPUS-4-6").is_some());
    }

    #[test]
    fn lookup_returns_none_for_unknown_model() {
        assert!(lookup("llama-99-ultra").is_none());
        assert!(lookup("").is_none());
    }

    #[test]
    fn cost_unknown_model_returns_none() {
        assert_eq!(
            cost_usd(Some("unknown"), Some(100), Some(50), None, None),
            None
        );
    }

    #[test]
    fn cost_none_model_returns_none() {
        assert_eq!(cost_usd(None, Some(100), Some(50), None, None), None);
    }

    #[test]
    fn cost_zero_tokens_returns_none() {
        // No non-zero counter → nothing to cost → None (not 0.0).
        // Matters because downstream code formats None as "—" and 0 as "$0.00".
        assert_eq!(
            cost_usd(Some("claude-opus-4-6"), Some(0), Some(0), Some(0), Some(0)),
            None
        );
        assert_eq!(
            cost_usd(Some("claude-opus-4-6"), None, None, None, None),
            None
        );
    }

    #[test]
    fn cost_computes_input_plus_output() {
        // opus-4-6: $15/Mtok input, $75/Mtok output.
        // 1M input + 1M output = $15 + $75 = $90.
        let c = cost_usd(
            Some("claude-opus-4-6"),
            Some(1_000_000),
            Some(1_000_000),
            None,
            None,
        )
        .unwrap();
        assert!((c - 90.0).abs() < 1e-6, "expected 90.0, got {c}");
    }

    #[test]
    fn cost_cache_read_uses_discounted_rate_when_provider_sets_one() {
        // opus-4-6 cache_read rate is $1.50/Mtok (10% of $15 input).
        // 1M cache_read → $1.50 alone.
        let c = cost_usd(Some("claude-opus-4-6"), None, None, Some(1_000_000), None).unwrap();
        assert!((c - 1.50).abs() < 1e-6, "expected 1.50, got {c}");
    }

    #[test]
    fn cost_falls_back_to_input_rate_when_cache_rate_missing() {
        // gpt-5 has no cache_create rate, so cache_create is billed at
        // input rate ($10/Mtok).
        let c = cost_usd(Some("gpt-5"), None, None, None, Some(1_000_000)).unwrap();
        assert!((c - 10.0).abs() < 1e-6, "expected 10.0, got {c}");
    }

    #[test]
    fn every_entry_has_last_verified_date() {
        for p in PRICES {
            assert!(
                !p.last_verified.is_empty(),
                "{} missing last_verified",
                p.name
            );
        }
    }

    #[test]
    fn no_duplicate_model_names() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for p in PRICES {
            assert!(seen.insert(p.name), "duplicate pricing entry: {}", p.name);
        }
    }

    #[test]
    fn claude_opus_4_8_is_priced() {
        // Regression: agx_session_summary returned cost_usd:null for the
        // current flagship because this row was missing.
        let c = cost_usd(
            Some("claude-opus-4-8"),
            Some(1_000_000),
            Some(1_000_000),
            None,
            None,
        )
        .expect("claude-opus-4-8 must be in the pricing table");
        assert!((c - 90.0).abs() < 1e-6, "expected 90.0, got {c}");
    }
}
