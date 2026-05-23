//! Timeline slicing — parsers + application for `--after` / `--before`
//! duration filters, `--after-step` / `--before-step` / `--range` index
//! filters, and the `:@<duration>` TUI jump command.
//!
//! Duration grammar (permissive, case-insensitive):
//!
//! - `30s` / `30sec`        → 30 seconds
//! - `5m`  / `5min`         → 5 minutes
//! - `2h`  / `2hr`          → 2 hours
//! - `1d`  / `1day`         → 1 day
//! - `1h30m` / `90m30s`     → concatenated components, summed
//! - Bare integer           → seconds (e.g. `300` = 5m)
//!
//! Range grammar: `start..end` (exclusive end, mirrors Rust's
//! `Range<usize>`). Open-ended forms: `..500`, `100..`, or just `..`
//! (no-op). 1-based step numbers internally convert to 0-based so
//! `--range 1..11` = the first 10 steps regardless of format.
//!
//! Time semantics: `--after 2h` / `--before 10m` are relative to the
//! *session's first step*, not to wall-clock now. This is unambiguous
//! for archived sessions where "now" is meaningless, and matches the
//! intuitive read of "give me what happened in the first 10 minutes of
//! this session". Sessions with no timestamps get a stderr warning and
//! pass through unfiltered.

use crate::timeline::Step;
use anyhow::{Result, anyhow};

/// Inclusive start, exclusive end, both 0-based.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepRange {
    pub start: Option<usize>,
    pub end: Option<usize>,
}

impl StepRange {
    pub fn is_identity(&self) -> bool {
        self.start.is_none() && self.end.is_none()
    }

    fn contains(&self, idx: usize) -> bool {
        if let Some(s) = self.start
            && idx < s
        {
            return false;
        }
        if let Some(e) = self.end
            && idx >= e
        {
            return false;
        }
        true
    }
}

/// Parse a duration string like `1h30m`, `45s`, `2h`, `90m30s`, or a
/// bare integer (seconds). Returns milliseconds.
pub fn parse_duration_ms(raw: &str) -> Result<u64> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Err(anyhow!("empty duration"));
    }
    // Bare integer → seconds.
    if let Ok(n) = s.parse::<u64>() {
        return Ok(n * 1_000);
    }
    let mut total_ms: u64 = 0;
    let mut num_buf = String::new();
    let mut unit_buf = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            if !unit_buf.is_empty() {
                total_ms = total_ms
                    .checked_add(commit_component(&num_buf, &unit_buf)?)
                    .ok_or_else(|| anyhow!("duration overflowed u64"))?;
                num_buf.clear();
                unit_buf.clear();
            }
            num_buf.push(ch);
        } else if ch.is_ascii_alphabetic() {
            unit_buf.push(ch);
        } else {
            return Err(anyhow!("unexpected character `{ch}` in duration `{raw}`"));
        }
    }
    if num_buf.is_empty() {
        return Err(anyhow!("duration `{raw}` has no number"));
    }
    if unit_buf.is_empty() {
        return Err(anyhow!(
            "duration `{raw}` has no unit suffix (try `{num_buf}s`)"
        ));
    }
    total_ms = total_ms
        .checked_add(commit_component(&num_buf, &unit_buf)?)
        .ok_or_else(|| anyhow!("duration overflowed u64"))?;
    Ok(total_ms)
}

fn commit_component(num: &str, unit: &str) -> Result<u64> {
    let n: u64 = num
        .parse()
        .map_err(|_| anyhow!("invalid number `{num}` in duration"))?;
    let multiplier_ms: u64 = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000,
        "m" | "min" | "mins" | "minute" | "minutes" => 60 * 1_000,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60 * 60 * 1_000,
        "d" | "day" | "days" => 24 * 60 * 60 * 1_000,
        other => {
            return Err(anyhow!(
                "unknown duration unit `{other}` (use s / m / h / d)"
            ));
        }
    };
    n.checked_mul(multiplier_ms)
        .ok_or_else(|| anyhow!("duration component `{num}{unit}` overflowed"))
}

/// Parse `start..end`, `..end`, `start..`, or `..` into a [`StepRange`].
/// End is always exclusive — `1..11` means the first ten steps. The CLI
/// surface accepts 1-based numbers; the conversion to 0-based indices
/// happens at the slice site.
pub fn parse_step_range(raw: &str) -> Result<StepRange> {
    let s = raw.trim();
    let Some((left, right)) = s.split_once("..") else {
        return Err(anyhow!(
            "range `{raw}` must contain `..` (e.g. `100..500`, `..500`, `100..`)"
        ));
    };
    let start = if left.trim().is_empty() {
        None
    } else {
        Some(
            left.trim()
                .parse::<usize>()
                .map_err(|_| anyhow!("range start `{left}` is not a number"))?,
        )
    };
    let end = if right.trim().is_empty() {
        None
    } else {
        Some(
            right
                .trim()
                .parse::<usize>()
                .map_err(|_| anyhow!("range end `{right}` is not a number"))?,
        )
    };
    if let (Some(s), Some(e)) = (start, end)
        && s > e
    {
        return Err(anyhow!("range `{raw}` has start > end ({s} > {e})"));
    }
    Ok(StepRange { start, end })
}

/// Build a `StepRange` from top-level `--after-step` / `--before-step`
/// scalars. Either may be `None` for an open bound.
pub fn step_range_from_bounds(after_step: Option<usize>, before_step: Option<usize>) -> StepRange {
    StepRange {
        start: after_step,
        end: before_step,
    }
}

/// Slice the steps by index range and optional time bounds. Time
/// bounds are offsets in milliseconds from the session's first step's
/// timestamp. When no step carries a timestamp, time filters are
/// silently skipped (and the caller gets a stderr warning from
/// `warn_if_time_filter_ignored` — kept out here so this function
/// stays pure).
pub fn slice_steps(
    steps: Vec<Step>,
    range: &StepRange,
    after_ms: Option<u64>,
    before_ms: Option<u64>,
) -> Vec<Step> {
    let has_time_filter = after_ms.is_some() || before_ms.is_some();
    // Resolve the time-filter anchor once. `None` → time filter is a
    // no-op for this session (either no timestamps anywhere, or no
    // `--after` / `--before` in play). Either way the closure below
    // can treat the time branch as inactive with a single check.
    let time_anchor = has_time_filter
        .then(|| steps.iter().find_map(|s| s.timestamp_ms))
        .flatten();

    steps
        .into_iter()
        .enumerate()
        .filter(|(idx, step)| {
            if !range.contains(*idx) {
                return false;
            }
            if let Some(start) = time_anchor {
                let Some(ts) = step.timestamp_ms else {
                    // Step has no timestamp in a session that does —
                    // drop it rather than include anomalously.
                    return false;
                };
                let offset = ts.saturating_sub(start);
                if let Some(a) = after_ms
                    && offset < a
                {
                    return false;
                }
                if let Some(b) = before_ms
                    && offset >= b
                {
                    return false;
                }
            }
            true
        })
        .map(|(_, step)| step)
        .collect()
}

/// Print a one-line stderr warning when the user asked for a time
/// filter but the session has no usable timestamps. Keeps
/// `slice_steps` itself pure.
pub fn warn_if_time_filter_ignored(steps: &[Step], after_ms: Option<u64>, before_ms: Option<u64>) {
    let requested = after_ms.is_some() || before_ms.is_some();
    let has_ts = steps.iter().any(|s| s.timestamp_ms.is_some());
    if requested && !has_ts {
        eprintln!(
            "agx: --after / --before requested but session has no step timestamps; skipping time filter"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{assistant_text_step, user_text_step};

    #[test]
    fn parse_duration_basic_units() {
        assert_eq!(parse_duration_ms("30s").unwrap(), 30_000);
        assert_eq!(parse_duration_ms("5m").unwrap(), 5 * 60 * 1_000);
        assert_eq!(parse_duration_ms("2h").unwrap(), 2 * 60 * 60 * 1_000);
        assert_eq!(parse_duration_ms("1d").unwrap(), 24 * 60 * 60 * 1_000);
    }

    #[test]
    fn parse_duration_long_unit_names() {
        assert_eq!(parse_duration_ms("30sec").unwrap(), 30_000);
        assert_eq!(parse_duration_ms("5minutes").unwrap(), 5 * 60 * 1_000);
        assert_eq!(parse_duration_ms("2hours").unwrap(), 2 * 60 * 60 * 1_000);
    }

    #[test]
    fn parse_duration_compound() {
        assert_eq!(parse_duration_ms("1h30m").unwrap(), (60 + 30) * 60 * 1_000);
        assert_eq!(
            parse_duration_ms("2h15m30s").unwrap(),
            (2 * 60 * 60 + 15 * 60 + 30) * 1_000
        );
    }

    #[test]
    fn parse_duration_case_insensitive() {
        assert_eq!(parse_duration_ms("2H").unwrap(), 2 * 60 * 60 * 1_000);
        assert_eq!(parse_duration_ms("5MIN").unwrap(), 5 * 60 * 1_000);
    }

    #[test]
    fn parse_duration_bare_integer_is_seconds() {
        // Convention: bare integer without a unit means seconds. Lets
        // users write `--after 90` for "90 seconds into the session"
        // without reaching for the suffix.
        assert_eq!(parse_duration_ms("90").unwrap(), 90_000);
    }

    #[test]
    fn parse_duration_rejects_empty_and_malformed() {
        assert!(parse_duration_ms("").is_err());
        assert!(parse_duration_ms("   ").is_err());
        assert!(parse_duration_ms("h").is_err()); // no number
        assert!(parse_duration_ms("5x").is_err()); // unknown unit
        assert!(parse_duration_ms("5.5h").is_err()); // no floats in this grammar
    }

    #[test]
    fn parse_step_range_closed() {
        let r = parse_step_range("100..500").unwrap();
        assert_eq!(r.start, Some(100));
        assert_eq!(r.end, Some(500));
    }

    #[test]
    fn parse_step_range_open_start() {
        let r = parse_step_range("..500").unwrap();
        assert_eq!(r.start, None);
        assert_eq!(r.end, Some(500));
    }

    #[test]
    fn parse_step_range_open_end() {
        let r = parse_step_range("100..").unwrap();
        assert_eq!(r.start, Some(100));
        assert_eq!(r.end, None);
    }

    #[test]
    fn parse_step_range_empty_is_identity() {
        let r = parse_step_range("..").unwrap();
        assert!(r.is_identity());
    }

    #[test]
    fn parse_step_range_rejects_reversed() {
        assert!(parse_step_range("500..100").is_err());
    }

    #[test]
    fn parse_step_range_rejects_non_range() {
        assert!(parse_step_range("not a range").is_err());
        assert!(parse_step_range("100").is_err());
    }

    #[test]
    fn step_range_contains_respects_exclusive_end() {
        let r = StepRange {
            start: Some(2),
            end: Some(5),
        };
        assert!(!r.contains(1));
        assert!(r.contains(2));
        assert!(r.contains(4));
        assert!(!r.contains(5)); // exclusive
    }

    #[test]
    fn slice_steps_by_index_range() {
        let steps: Vec<_> = (0..10).map(|i| user_text_step(&format!("s{i}"))).collect();
        let sliced = slice_steps(
            steps,
            &StepRange {
                start: Some(2),
                end: Some(5),
            },
            None,
            None,
        );
        assert_eq!(sliced.len(), 3);
        assert!(sliced[0].detail.contains("s2"));
        assert!(sliced[2].detail.contains("s4"));
    }

    #[test]
    fn slice_steps_by_time_offset() {
        let mut steps = vec![
            user_text_step("t0"),
            assistant_text_step("t5"),
            assistant_text_step("t10"),
            assistant_text_step("t20"),
        ];
        steps[0].timestamp_ms = Some(1_000_000);
        steps[1].timestamp_ms = Some(1_000_000 + 5_000);
        steps[2].timestamp_ms = Some(1_000_000 + 10_000);
        steps[3].timestamp_ms = Some(1_000_000 + 20_000);
        // Keep steps at 5s ≤ offset < 15s.
        let sliced = slice_steps(steps, &StepRange::default(), Some(5_000), Some(15_000));
        assert_eq!(sliced.len(), 2);
        assert!(sliced[0].detail.contains("t5"));
        assert!(sliced[1].detail.contains("t10"));
    }

    #[test]
    fn slice_steps_time_filter_no_op_without_timestamps() {
        let steps = vec![user_text_step("a"), user_text_step("b")];
        // Time filters present, but no step has a timestamp. Slice
        // should leave the steps alone.
        let sliced = slice_steps(steps, &StepRange::default(), Some(1_000), Some(10_000));
        assert_eq!(sliced.len(), 2);
    }

    #[test]
    fn slice_steps_identity_when_no_filters() {
        let steps: Vec<_> = (0..5).map(|i| user_text_step(&format!("s{i}"))).collect();
        let before = steps.len();
        let sliced = slice_steps(steps, &StepRange::default(), None, None);
        assert_eq!(sliced.len(), before);
    }

    #[test]
    fn step_range_from_bounds_combines_after_and_before() {
        let r = step_range_from_bounds(Some(10), Some(50));
        assert_eq!(r.start, Some(10));
        assert_eq!(r.end, Some(50));
        // Open ends.
        let r = step_range_from_bounds(None, Some(50));
        assert_eq!(r.start, None);
        assert_eq!(r.end, Some(50));
        let r = step_range_from_bounds(Some(10), None);
        assert_eq!(r.start, Some(10));
        assert_eq!(r.end, None);
    }
}
