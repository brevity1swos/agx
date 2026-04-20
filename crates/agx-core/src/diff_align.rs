//! Session-to-session alignment. Pure-algorithm module with no TUI
//! dependencies — all rendering lives in `diff_tui.rs`. Kept separate so
//! the alignment logic can be unit-tested cleanly and reused for non-TUI
//! diff modes later.
//!
//! Algorithm: longest common subsequence (LCS) over a "structural
//! signature" that ignores per-step content. Steps are considered equal
//! for alignment purposes when they share the same `StepKind` and, for
//! tool-related steps, the same `tool_name`. Content equality (text,
//! tool input, tool output) is computed separately per aligned pair so
//! the TUI can color rows by "match vs input-differs vs only-one-side".
//!
//! Why LCS over (kind, tool_name): real agent sessions that do "the same
//! thing" often insert extra tool calls or assistant messages on one
//! side. Position-based alignment fails immediately. Content-based
//! alignment over-matches on boilerplate. Structural alignment gets you
//! "the agents performed the same tool call sequence" which is the
//! signal worth surfacing.
//!
//! Complexity: O(N * M) time and space on the DP table. For typical
//! session sizes (< 2000 steps) that's trivial. Hunt-Szymanski or Myers
//! would be needed if corpora push into the 10k+ range per session; not
//! in scope yet.

use crate::timeline::{Step, StepKind};

/// The structural signature used for LCS equality. Derived once per
/// step on each side so the DP table's inner loop is cheap.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sig {
    kind: StepKind,
    tool: Option<String>,
}

impl Sig {
    fn of(step: &Step) -> Self {
        Sig {
            kind: step.kind,
            tool: step.tool_name.clone(),
        }
    }
}

/// How an aligned row relates its left / right halves. The TUI maps
/// these to foreground colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignKind {
    /// Both sides present, same kind + tool, and identical detail text.
    Match,
    /// Both sides present, same kind + tool, but detail text differs.
    /// Typical for assistant messages with paraphrased wording or tool
    /// calls with slightly different inputs.
    Differ,
    /// Step exists only on the left (deletion in A → B terms).
    LeftOnly,
    /// Step exists only on the right (insertion in A → B terms).
    RightOnly,
}

/// One row of the two-pane diff rendering. Exactly one of `left` /
/// `right` is always populated for LeftOnly / RightOnly; both for
/// Match / Differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlignRow {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub kind: AlignKind,
}

/// Align two step sequences. Returns a sequence of rows suitable for
/// rendering in a two-pane TUI. Rows are emitted in a consistent
/// "walk both sides together" order so that row N on screen
/// corresponds to row N in the timeline on both sides (with gaps
/// shown as gray gutters).
pub fn align(left: &[Step], right: &[Step]) -> Vec<AlignRow> {
    if left.is_empty() && right.is_empty() {
        return Vec::new();
    }
    let left_sigs: Vec<Sig> = left.iter().map(Sig::of).collect();
    let right_sigs: Vec<Sig> = right.iter().map(Sig::of).collect();
    let pairs = lcs_indices(&left_sigs, &right_sigs);
    weave(left, right, &pairs)
}

/// Standard LCS DP → backtrack. Returns the aligned `(li, ri)` index
/// pairs in strictly increasing order on both components.
fn lcs_indices(left: &[Sig], right: &[Sig]) -> Vec<(usize, usize)> {
    let n = left.len();
    let m = right.len();
    // `dp[i][j]` = length of LCS of `left[..i]` and `right[..j]`.
    // Allocated as `(n+1) * (m+1)` with a row offset helper.
    let row = m + 1;
    let mut dp = vec![0u32; (n + 1) * row];
    for i in 1..=n {
        for j in 1..=m {
            let idx = i * row + j;
            if left[i - 1] == right[j - 1] {
                dp[idx] = dp[(i - 1) * row + (j - 1)] + 1;
            } else {
                dp[idx] = dp[(i - 1) * row + j].max(dp[i * row + (j - 1)]);
            }
        }
    }
    // Backtrack collecting matched index pairs.
    let mut pairs = Vec::new();
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        if left[i - 1] == right[j - 1] {
            pairs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[(i - 1) * row + j] >= dp[i * row + (j - 1)] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    pairs
}

/// Walk the two sequences in lockstep, using the LCS pair list to know
/// which elements are "aligned" and which are one-sided gaps.
fn weave(left: &[Step], right: &[Step], pairs: &[(usize, usize)]) -> Vec<AlignRow> {
    let mut rows = Vec::with_capacity(left.len().max(right.len()));
    let mut li = 0usize;
    let mut ri = 0usize;
    for &(pi, pj) in pairs {
        // Emit left-only rows up to the next matched left index.
        while li < pi {
            rows.push(AlignRow {
                left: Some(li),
                right: None,
                kind: AlignKind::LeftOnly,
            });
            li += 1;
        }
        // Emit right-only rows up to the next matched right index.
        while ri < pj {
            rows.push(AlignRow {
                left: None,
                right: Some(ri),
                kind: AlignKind::RightOnly,
            });
            ri += 1;
        }
        // Matched pair — decide Match vs Differ on detail content.
        let kind = if left[li].detail == right[ri].detail {
            AlignKind::Match
        } else {
            AlignKind::Differ
        };
        rows.push(AlignRow {
            left: Some(li),
            right: Some(ri),
            kind,
        });
        li += 1;
        ri += 1;
    }
    // Drain trailing one-sided rows.
    while li < left.len() {
        rows.push(AlignRow {
            left: Some(li),
            right: None,
            kind: AlignKind::LeftOnly,
        });
        li += 1;
    }
    while ri < right.len() {
        rows.push(AlignRow {
            left: None,
            right: Some(ri),
            kind: AlignKind::RightOnly,
        });
        ri += 1;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{assistant_text_step, tool_result_step, tool_use_step, user_text_step};

    #[test]
    fn identical_sequences_all_match() {
        let seq = vec![
            user_text_step("hi"),
            assistant_text_step("hello"),
            tool_use_step("t1", "Read", "{}"),
            tool_result_step("t1", "ok", Some("Read"), Some("{}")),
        ];
        let rows = align(&seq, &seq);
        assert_eq!(rows.len(), 4);
        for r in &rows {
            assert_eq!(r.kind, AlignKind::Match);
            assert!(r.left.is_some() && r.right.is_some());
        }
    }

    #[test]
    fn empty_inputs_return_empty() {
        let rows = align(&[], &[]);
        assert!(rows.is_empty());
    }

    #[test]
    fn left_only_when_right_is_empty() {
        let left = vec![user_text_step("hi"), assistant_text_step("ok")];
        let rows = align(&left, &[]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, AlignKind::LeftOnly);
        assert_eq!(rows[1].kind, AlignKind::LeftOnly);
        assert!(rows.iter().all(|r| r.right.is_none()));
    }

    #[test]
    fn right_only_when_left_is_empty() {
        let right = vec![user_text_step("hi"), assistant_text_step("ok")];
        let rows = align(&[], &right);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.kind == AlignKind::RightOnly));
        assert!(rows.iter().all(|r| r.left.is_none()));
    }

    #[test]
    fn extra_right_tool_call_becomes_right_only_row() {
        // A: user → asst → done
        // B: user → asst → tool_use → tool_result → asst (done)
        // LCS on (kind, tool_name): user, asst_text, asst_text
        // The tool_use / tool_result on the right are gaps; the trailing
        // asst on the right aligns with the "done" asst on the left.
        let left = vec![
            user_text_step("q"),
            assistant_text_step("thinking"),
            assistant_text_step("done"),
        ];
        let right = vec![
            user_text_step("q"),
            assistant_text_step("thinking"),
            tool_use_step("t1", "Bash", "{}"),
            tool_result_step("t1", "ok", Some("Bash"), Some("{}")),
            assistant_text_step("done"),
        ];
        let rows = align(&left, &right);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].kind, AlignKind::Match); // user
        assert_eq!(rows[1].kind, AlignKind::Match); // asst "thinking"
        assert_eq!(rows[2].kind, AlignKind::RightOnly); // tool_use
        assert_eq!(rows[3].kind, AlignKind::RightOnly); // tool_result
        assert_eq!(rows[4].kind, AlignKind::Match); // asst "done"
    }

    #[test]
    fn same_structure_different_text_produces_differ_rows() {
        // Same signature sequence (user / asst), different text on the
        // assistant message — should align as Differ, not Match.
        let left = vec![
            user_text_step("q"),
            assistant_text_step("Hello, I'll help with that."),
        ];
        let right = vec![
            user_text_step("q"),
            assistant_text_step("Sure, let me try."),
        ];
        let rows = align(&left, &right);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, AlignKind::Match); // user "q" identical
        assert_eq!(rows[1].kind, AlignKind::Differ); // asst text differs
    }

    #[test]
    fn different_tool_names_at_same_position_become_one_sided() {
        // LCS signature matching requires tool_name equality, so
        // Read vs Write on the same position don't pair — they land as
        // left-only + right-only gaps. This preserves the signal that
        // "the agents made different tool choices here" instead of
        // hiding it as a "differ" match.
        let left = vec![tool_use_step("t1", "Read", "{}")];
        let right = vec![tool_use_step("t2", "Write", "{}")];
        let rows = align(&left, &right);
        assert_eq!(rows.len(), 2);
        // Order is deterministic: left-only comes first in our weave.
        assert_eq!(rows[0].kind, AlignKind::LeftOnly);
        assert_eq!(rows[1].kind, AlignKind::RightOnly);
    }

    #[test]
    fn same_tool_different_input_becomes_differ() {
        let left = vec![tool_use_step("t1", "Bash", "ls")];
        let right = vec![tool_use_step("t2", "Bash", "ls -la")];
        let rows = align(&left, &right);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, AlignKind::Differ);
    }

    #[test]
    fn reordered_tool_calls_produce_mix_of_match_and_gaps() {
        // A: Read, Bash. B: Bash, Read. LCS length is 1 — either the
        // Reads or the Bashes align. Tool IDs differ between left and
        // right, so detail strings differ and the paired row is
        // Differ, not Match. The other two rows are one-sided gaps.
        let left = vec![
            tool_use_step("r1", "Read", "{}"),
            tool_use_step("b1", "Bash", "{}"),
        ];
        let right = vec![
            tool_use_step("b2", "Bash", "{}"),
            tool_use_step("r2", "Read", "{}"),
        ];
        let rows = align(&left, &right);
        assert_eq!(rows.len(), 3);
        // Exactly one aligned pair (Match or Differ — differs here
        // because of the synthetic tool IDs).
        let paired = rows
            .iter()
            .filter(|r| matches!(r.kind, AlignKind::Match | AlignKind::Differ))
            .count();
        assert_eq!(paired, 1);
        // Remaining two are gaps.
        let gaps = rows
            .iter()
            .filter(|r| matches!(r.kind, AlignKind::LeftOnly | AlignKind::RightOnly))
            .count();
        assert_eq!(gaps, 2);
    }

    #[test]
    fn lcs_prefers_longer_alignment_over_short() {
        // A: user, asst, asst, asst
        // B: user, asst
        // LCS length is 2 over signatures — the two user+asst pairs.
        // The three assts on the left share the same signature, so
        // LCS has three equivalent choices for which asst to pair with
        // the right side's asst; our backtrack is deterministic but
        // the *structure* is the same regardless: 4 output rows
        // containing exactly 2 aligned pairs and 2 left-only rows.
        let left = vec![
            user_text_step("hi"),
            assistant_text_step("a"),
            assistant_text_step("b"),
            assistant_text_step("c"),
        ];
        let right = vec![user_text_step("hi"), assistant_text_step("z")];
        let rows = align(&left, &right);
        assert_eq!(rows.len(), 4);
        // First row pairs the user messages (identical text → Match).
        assert_eq!(rows[0].kind, AlignKind::Match);
        assert!(rows[0].left == Some(0) && rows[0].right == Some(0));
        // Among the remaining three rows, exactly one is an
        // aligned pair (the asst) and two are LeftOnly.
        let tail = &rows[1..];
        let paired = tail
            .iter()
            .filter(|r| matches!(r.kind, AlignKind::Match | AlignKind::Differ))
            .count();
        let left_only = tail
            .iter()
            .filter(|r| r.kind == AlignKind::LeftOnly)
            .count();
        assert_eq!(paired, 1);
        assert_eq!(left_only, 2);
    }
}
