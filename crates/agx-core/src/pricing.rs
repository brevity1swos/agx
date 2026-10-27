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
//! Token convention: `tokens_in` is uncached input; cache reads and cache
//! writes are separate counters (parsers for providers that report a
//! cache-inclusive prompt count subtract the cached share). Cache reads
//! and writes use the row's own rate when the provider publishes one and
//! fall back to the input rate otherwise.
//!
//! Some models charge more once a prompt crosses a length threshold
//! (Claude Haiku 5.5 above 100K, Gemini 2.5 Pro above 200K, GPT-5.4+ above
//! 272K). Those rows carry a `long_context` tier, applied per step when
//! `tokens_in + cache_read + cache_create` exceeds the threshold.
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
    /// For Anthropic this is the 5-minute-TTL write rate.
    pub cache_create_per_mtoken: Option<f64>,
    /// Anthropic's 1-hour-TTL cache write rate, applied to
    /// `Step::cache_create_1h`. When `None`, those tokens bill at
    /// `cache_create_per_mtoken`.
    pub cache_create_1h_per_mtoken: Option<f64>,
    /// Higher rates for prompts above a length threshold, when the
    /// provider prices that way.
    pub long_context: Option<LongContextTier>,
    /// Last date a human verified this entry against the provider's pricing
    /// page. Present for audit; not used at runtime.
    #[allow(dead_code)]
    pub last_verified: &'static str,
}

/// Rates that replace a row's base rates once a step's prompt
/// (`tokens_in + cache_read + cache_create`) exceeds `above_prompt_tokens`.
#[derive(Debug, Clone, Copy)]
pub struct LongContextTier {
    pub above_prompt_tokens: u64,
    pub input_per_mtoken: f64,
    pub output_per_mtoken: f64,
    pub cache_read_per_mtoken: Option<f64>,
    pub cache_create_per_mtoken: Option<f64>,
    pub cache_create_1h_per_mtoken: Option<f64>,
}

// Source pages at time of last verification (2026-10-27):
//   Anthropic: https://platform.claude.com/docs/en/about-claude/pricing
//   OpenAI:    https://developers.openai.com/api/docs/pricing
//   Google:    https://ai.google.dev/gemini-api/docs/pricing
//
// Anthropic cache columns are 5-minute cache writes. Where Anthropic only
// publishes the standard multipliers (reads 0.1x input, 5-minute writes
// 1.25x), the rate is derived from them. Model names are matched exactly,
// so they must be spelled the way session files record them (Gemini uses
// dots: `gemini-2.5-pro`).
const PRICES: &[ModelPricing] = &[
    // --- Anthropic ---
    ModelPricing {
        name: "claude-fable-5-1",
        input_per_mtoken: 10.0,
        output_per_mtoken: 50.0,
        cache_read_per_mtoken: Some(0.25),
        cache_create_per_mtoken: Some(12.5),
        cache_create_1h_per_mtoken: Some(20.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-mythos-5-1",
        input_per_mtoken: 10.0,
        output_per_mtoken: 50.0,
        cache_read_per_mtoken: Some(0.25),
        cache_create_per_mtoken: Some(12.5),
        cache_create_1h_per_mtoken: Some(20.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-fable-5",
        input_per_mtoken: 10.0,
        output_per_mtoken: 50.0,
        cache_read_per_mtoken: Some(1.0),
        cache_create_per_mtoken: Some(12.5),
        cache_create_1h_per_mtoken: Some(20.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-opus-5-5",
        input_per_mtoken: 4.0,
        output_per_mtoken: 20.0,
        cache_read_per_mtoken: Some(0.2),
        cache_create_per_mtoken: Some(5.0),
        cache_create_1h_per_mtoken: Some(8.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-opus-5",
        input_per_mtoken: 5.0,
        output_per_mtoken: 25.0,
        cache_read_per_mtoken: Some(0.5),
        cache_create_per_mtoken: Some(6.25),
        cache_create_1h_per_mtoken: Some(10.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-opus-4-8",
        input_per_mtoken: 5.0,
        output_per_mtoken: 25.0,
        cache_read_per_mtoken: Some(0.5),
        cache_create_per_mtoken: Some(6.25),
        cache_create_1h_per_mtoken: Some(10.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-opus-4-7",
        input_per_mtoken: 5.0,
        output_per_mtoken: 25.0,
        cache_read_per_mtoken: Some(0.5),
        cache_create_per_mtoken: Some(6.25),
        cache_create_1h_per_mtoken: Some(10.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-opus-4-6",
        input_per_mtoken: 5.0,
        output_per_mtoken: 25.0,
        cache_read_per_mtoken: Some(0.5),
        cache_create_per_mtoken: Some(6.25),
        cache_create_1h_per_mtoken: Some(10.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-sonnet-5-5",
        input_per_mtoken: 2.0,
        output_per_mtoken: 10.0,
        cache_read_per_mtoken: Some(0.2),
        cache_create_per_mtoken: Some(2.5),
        cache_create_1h_per_mtoken: Some(4.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-sonnet-5",
        input_per_mtoken: 2.0,
        output_per_mtoken: 10.0,
        cache_read_per_mtoken: Some(0.2),
        cache_create_per_mtoken: Some(2.5),
        cache_create_1h_per_mtoken: Some(4.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-sonnet-4-6",
        input_per_mtoken: 3.0,
        output_per_mtoken: 15.0,
        cache_read_per_mtoken: Some(0.3),
        cache_create_per_mtoken: Some(3.75),
        cache_create_1h_per_mtoken: Some(6.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-haiku-5-5",
        input_per_mtoken: 0.1,
        output_per_mtoken: 0.5,
        cache_read_per_mtoken: Some(0.01),
        cache_create_per_mtoken: Some(0.125),
        cache_create_1h_per_mtoken: Some(0.2),
        long_context: Some(LongContextTier {
            above_prompt_tokens: 100000,
            input_per_mtoken: 0.5,
            output_per_mtoken: 2.5,
            cache_read_per_mtoken: Some(0.05),
            cache_create_per_mtoken: Some(0.625),
            cache_create_1h_per_mtoken: Some(1.0),
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "claude-haiku-4-5",
        input_per_mtoken: 1.0,
        output_per_mtoken: 5.0,
        cache_read_per_mtoken: Some(0.1),
        cache_create_per_mtoken: Some(1.25),
        cache_create_1h_per_mtoken: Some(2.0),
        long_context: None,
        last_verified: "2026-10-27",
    },
    // --- OpenAI (long context: >272K input tokens) ---
    ModelPricing {
        name: "gpt-5.6-sol",
        input_per_mtoken: 4.0,
        output_per_mtoken: 20.0,
        cache_read_per_mtoken: Some(0.4),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 272000,
            input_per_mtoken: 8.0,
            output_per_mtoken: 30.0,
            cache_read_per_mtoken: Some(0.8),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.6-terra",
        input_per_mtoken: 2.0,
        output_per_mtoken: 12.0,
        cache_read_per_mtoken: Some(0.2),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 272000,
            input_per_mtoken: 4.0,
            output_per_mtoken: 18.0,
            cache_read_per_mtoken: Some(0.4),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.6-luna",
        input_per_mtoken: 0.2,
        output_per_mtoken: 1.2,
        cache_read_per_mtoken: Some(0.02),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 272000,
            input_per_mtoken: 0.4,
            output_per_mtoken: 1.8,
            cache_read_per_mtoken: Some(0.04),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.5",
        input_per_mtoken: 5.0,
        output_per_mtoken: 30.0,
        cache_read_per_mtoken: Some(0.5),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 272000,
            input_per_mtoken: 10.0,
            output_per_mtoken: 45.0,
            cache_read_per_mtoken: Some(1.0),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.4",
        input_per_mtoken: 2.5,
        output_per_mtoken: 15.0,
        cache_read_per_mtoken: Some(0.25),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 272000,
            input_per_mtoken: 5.0,
            output_per_mtoken: 22.5,
            cache_read_per_mtoken: Some(0.5),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.4-mini",
        input_per_mtoken: 0.75,
        output_per_mtoken: 4.5,
        cache_read_per_mtoken: Some(0.075),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.4-nano",
        input_per_mtoken: 0.2,
        output_per_mtoken: 1.25,
        cache_read_per_mtoken: Some(0.02),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.3-codex",
        input_per_mtoken: 1.75,
        output_per_mtoken: 14.0,
        cache_read_per_mtoken: Some(0.175),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.2",
        input_per_mtoken: 1.75,
        output_per_mtoken: 14.0,
        cache_read_per_mtoken: Some(0.175),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5.1",
        input_per_mtoken: 1.25,
        output_per_mtoken: 10.0,
        cache_read_per_mtoken: Some(0.125),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5",
        input_per_mtoken: 1.25,
        output_per_mtoken: 10.0,
        cache_read_per_mtoken: Some(0.125),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5-mini",
        input_per_mtoken: 0.25,
        output_per_mtoken: 2.0,
        cache_read_per_mtoken: Some(0.025),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gpt-5-nano",
        input_per_mtoken: 0.05,
        output_per_mtoken: 0.4,
        cache_read_per_mtoken: Some(0.005),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    // --- Google Gemini (text / image / video input rates) ---
    ModelPricing {
        name: "gemini-3.1-pro-preview",
        input_per_mtoken: 2.0,
        output_per_mtoken: 12.0,
        cache_read_per_mtoken: Some(0.2),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 200000,
            input_per_mtoken: 4.0,
            output_per_mtoken: 18.0,
            cache_read_per_mtoken: Some(0.4),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gemini-3-flash-preview",
        input_per_mtoken: 0.5,
        output_per_mtoken: 3.0,
        cache_read_per_mtoken: Some(0.05),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gemini-2.5-pro",
        input_per_mtoken: 1.25,
        output_per_mtoken: 10.0,
        cache_read_per_mtoken: Some(0.125),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: Some(LongContextTier {
            above_prompt_tokens: 200000,
            input_per_mtoken: 2.5,
            output_per_mtoken: 15.0,
            cache_read_per_mtoken: Some(0.25),
            cache_create_per_mtoken: None,
            cache_create_1h_per_mtoken: None,
        }),
        last_verified: "2026-10-27",
    },
    ModelPricing {
        name: "gemini-2.5-flash",
        input_per_mtoken: 0.3,
        output_per_mtoken: 2.5,
        cache_read_per_mtoken: Some(0.03),
        cache_create_per_mtoken: None,
        cache_create_1h_per_mtoken: None,
        long_context: None,
        last_verified: "2026-10-27",
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
/// token counters (nothing to cost). Bills every cache write at the
/// 5-minute rate; use [`cost_usd_with_ttl`] when the 1-hour share is known.
#[must_use]
pub fn cost_usd(
    model: Option<&str>,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    cache_read: Option<u64>,
    cache_create: Option<u64>,
) -> Option<f64> {
    cost_usd_with_ttl(model, tokens_in, tokens_out, cache_read, cache_create, None)
}

/// [`cost_usd`], with `cache_create_1h` — the part of `cache_create`
/// written with Anthropic's 1-hour TTL — billed at the 1-hour rate.
#[must_use]
pub fn cost_usd_with_ttl(
    model: Option<&str>,
    tokens_in: Option<u64>,
    tokens_out: Option<u64>,
    cache_read: Option<u64>,
    cache_create: Option<u64>,
    cache_create_1h: Option<u64>,
) -> Option<f64> {
    let pricing = lookup(model?)?;
    let has_any = [tokens_in, tokens_out, cache_read, cache_create]
        .iter()
        .any(|v| v.is_some_and(|n| n > 0));
    if !has_any {
        return None;
    }
    let (n_in, n_out, n_cr, n_cc) = (
        tokens_in.unwrap_or(0),
        tokens_out.unwrap_or(0),
        cache_read.unwrap_or(0),
        cache_create.unwrap_or(0),
    );
    let prompt = n_in.saturating_add(n_cr).saturating_add(n_cc);
    let (input, output, cr_rate, cc_rate, cc_1h_rate) = match pricing.long_context {
        Some(t) if prompt > t.above_prompt_tokens => (
            t.input_per_mtoken,
            t.output_per_mtoken,
            t.cache_read_per_mtoken,
            t.cache_create_per_mtoken,
            t.cache_create_1h_per_mtoken,
        ),
        _ => (
            pricing.input_per_mtoken,
            pricing.output_per_mtoken,
            pricing.cache_read_per_mtoken,
            pricing.cache_create_per_mtoken,
            pricing.cache_create_1h_per_mtoken,
        ),
    };
    let cc_rate = cc_rate.unwrap_or(input);
    // `cache_create_1h` is a share of `cache_create`, never extra tokens.
    let n_cc_1h = cache_create_1h.unwrap_or(0).min(n_cc);
    #[allow(clippy::cast_precision_loss)]
    let cost = n_in as f64 * input
        + n_out as f64 * output
        + n_cr as f64 * cr_rate.unwrap_or(input)
        + (n_cc - n_cc_1h) as f64 * cc_rate
        + n_cc_1h as f64 * cc_1h_rate.unwrap_or(cc_rate);
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
        // opus-4-6: $5/Mtok input, $25/Mtok output.
        // 1M input + 1M output = $5 + $25 = $30.
        let c = cost_usd(
            Some("claude-opus-4-6"),
            Some(1_000_000),
            Some(1_000_000),
            None,
            None,
        )
        .unwrap();
        assert!((c - 30.0).abs() < 1e-6, "expected 30.0, got {c}");
    }

    #[test]
    fn cost_cache_read_uses_discounted_rate_when_provider_sets_one() {
        // opus-4-6 cache_read rate is $0.50/Mtok (10% of $5 input).
        // 1M cache_read → $0.50 alone.
        let c = cost_usd(Some("claude-opus-4-6"), None, None, Some(1_000_000), None).unwrap();
        assert!((c - 0.50).abs() < 1e-6, "expected 0.50, got {c}");
    }

    #[test]
    fn cost_falls_back_to_input_rate_when_cache_rate_missing() {
        // gpt-5 has no cache_create rate, so cache_create is billed at
        // input rate ($1.25/Mtok).
        let c = cost_usd(Some("gpt-5"), None, None, None, Some(1_000_000)).unwrap();
        assert!((c - 1.25).abs() < 1e-6, "expected 1.25, got {c}");
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
        assert!((c - 30.0).abs() < 1e-6, "expected 30.0, got {c}");
    }

    #[test]
    fn models_seen_in_current_sessions_are_priced() {
        // Model names as Claude Code / Codex / Gemini CLI write them.
        for m in [
            "claude-opus-5-5",
            "claude-sonnet-5-5",
            "claude-sonnet-5",
            "claude-opus-4-7",
            "gpt-5.4",
            "gemini-3-flash-preview",
            "gemini-2.5-pro",
        ] {
            assert!(lookup(m).is_some(), "{m} missing from the pricing table");
        }
    }

    #[test]
    fn long_context_tier_applies_above_threshold() {
        // claude-haiku-5-5: $0.10 input up to 100K prompt tokens, $0.50 above.
        let short = cost_usd(Some("claude-haiku-5-5"), Some(100_000), None, None, None).unwrap();
        assert!((short - 0.01).abs() < 1e-9, "got {short}");
        let long = cost_usd(Some("claude-haiku-5-5"), Some(100_001), None, None, None).unwrap();
        assert!((long - 100_001.0 * 0.50 / 1e6).abs() < 1e-9, "got {long}");
    }

    #[test]
    fn long_context_threshold_counts_cached_prompt_tokens() {
        // 10K uncached + 300K cache reads is a 310K prompt: above gpt-5.4's
        // 272K line, so the long-context cache-read rate ($0.50) applies.
        let c = cost_usd(Some("gpt-5.4"), Some(10_000), None, Some(300_000), None).unwrap();
        let expected = (10_000.0 * 5.0 + 300_000.0 * 0.50) / 1e6;
        assert!((c - expected).abs() < 1e-9, "expected {expected}, got {c}");
    }

    #[test]
    fn long_context_tiers_are_pricier_than_base() {
        for p in PRICES {
            if let Some(t) = p.long_context {
                assert!(t.input_per_mtoken > p.input_per_mtoken, "{}", p.name);
                assert!(t.output_per_mtoken >= p.output_per_mtoken, "{}", p.name);
            }
        }
    }

    #[test]
    fn one_hour_cache_writes_bill_at_the_1h_rate() {
        // Token counts and total from a real Claude Code `cost-state`
        // entry (claude-opus-5-5, every cache write at the 1-hour TTL);
        // Claude Code recorded costUSD = 1.6526048.
        let c = cost_usd_with_ttl(
            Some("claude-opus-5-5"),
            Some(1_122),
            Some(16_105),
            Some(3_836_724),
            Some(69_834),
            Some(69_834),
        )
        .unwrap();
        assert!((c - 1.652_604_8).abs() < 1e-6, "got {c}");
    }

    #[test]
    fn one_hour_share_is_capped_at_total_cache_writes() {
        let capped = cost_usd_with_ttl(
            Some("claude-opus-5-5"),
            None,
            None,
            None,
            Some(10),
            Some(99),
        );
        let all_1h = cost_usd_with_ttl(
            Some("claude-opus-5-5"),
            None,
            None,
            None,
            Some(10),
            Some(10),
        );
        assert_eq!(capped, all_1h);
    }
}
