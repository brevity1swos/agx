use crate::session::{
    AssistantContentItem, Entry, ToolResultContent, UserContent, UserContentItem,
};
use std::collections::HashMap;

pub(crate) const LABEL_PREVIEW_WIDTH: usize = 60;
pub(crate) const RESULT_PREVIEW_WIDTH: usize = 50;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Step {
    pub label: String,
    pub detail: String,
    pub kind: StepKind,
    pub tool_name: Option<String>,
    pub timestamp_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    /// Model name for this step, if known. Attached to the first step emitted
    /// from each assistant message (see `attach_usage_to_first` below).
    pub model: Option<String>,
    /// Input tokens. Anthropic / OpenAI naming: one-time prompt tokens sent
    /// to the model for this assistant response.
    pub tokens_in: Option<u64>,
    /// Output tokens: tokens the model generated in this assistant response.
    pub tokens_out: Option<u64>,
    /// Tokens read from the prompt cache (Anthropic). None for providers
    /// that don't support or report cache reads.
    pub cache_read: Option<u64>,
    /// Tokens written to the prompt cache in this response (Anthropic).
    pub cache_create: Option<u64>,
    /// True when this step is the root of a conversation branch — i.e.
    /// its originating entry shares a `parentUuid` with at least one
    /// other entry in the same session. Only set by the Claude Code
    /// parser today (the only format where `parentUuid` is a
    /// first-class field); other parsers leave this `false`. Powers
    /// the TUI fork list overlay and status-bar fork count (Phase 5.1).
    #[serde(default)]
    pub is_fork_root: bool,
    /// The tool-call ID for `ToolUse` / `ToolResult` steps, pairing
    /// them across the timeline. Populated by `tool_use_step` and
    /// `tool_result_step`; every format parser passes this through
    /// from its native ID field. Powers Phase 6 trajectory exports
    /// (OpenAI fine-tuning format needs the explicit ID on each
    /// tool_calls entry and on the matching tool message). `None`
    /// for non-tool step kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Step {
    /// USD cost for this step, computed from its token counters and model.
    /// Returns `None` when the model is unknown or there are no tokens to
    /// cost. Delegates to the pricing table in `crate::pricing`.
    #[must_use]
    pub fn cost_usd(&self) -> Option<f64> {
        crate::pricing::cost_usd(
            self.model.as_deref(),
            self.tokens_in,
            self.tokens_out,
            self.cache_read,
            self.cache_create,
        )
    }
}

/// Session-level totals for the `--summary` mode and future corpus
/// analytics. Cost is `None` when no step had a known model; otherwise it
/// sums `Step::cost_usd()` across steps that could be costed.
#[derive(Debug, Default, serde::Serialize)]
pub struct SessionTotals {
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_read: u64,
    pub cache_create: u64,
    pub cost_usd: Option<f64>,
    pub unique_models: Vec<String>,
}

impl SessionTotals {
    #[must_use]
    pub fn has_tokens(&self) -> bool {
        self.tokens_in > 0 || self.tokens_out > 0 || self.cache_read > 0 || self.cache_create > 0
    }
}

/// Aggregate token counters and cost across a set of steps. Returns zeros
/// when nothing has usage data; callers should check `has_tokens()` before
/// displaying.
#[must_use]
pub fn compute_session_totals(steps: &[Step]) -> SessionTotals {
    let mut t = SessionTotals::default();
    let mut models: Vec<String> = Vec::new();
    let mut any_cost: Option<f64> = None;
    for step in steps {
        t.tokens_in += step.tokens_in.unwrap_or(0);
        t.tokens_out += step.tokens_out.unwrap_or(0);
        t.cache_read += step.cache_read.unwrap_or(0);
        t.cache_create += step.cache_create.unwrap_or(0);
        if let Some(m) = &step.model
            && !models.iter().any(|existing| existing == m)
        {
            models.push(m.clone());
        }
        if let Some(c) = step.cost_usd() {
            any_cost = Some(any_cost.unwrap_or(0.0) + c);
        }
    }
    t.cost_usd = any_cost;
    t.unique_models = models;
    t
}

/// Normalized usage numbers any parser can produce. Each parser extracts its
/// format-specific usage shape and lowers to this struct, which is then
/// attached to the first step emitted from the corresponding assistant
/// message via `attach_usage_to_first`.
#[derive(Debug, Clone, Default)]
pub(crate) struct Usage {
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_create: Option<u64>,
}

impl Usage {
    pub fn is_empty(&self) -> bool {
        self.tokens_in.is_none()
            && self.tokens_out.is_none()
            && self.cache_read.is_none()
            && self.cache_create.is_none()
    }
}

/// Attach model + usage to the first step at-or-after `start` in `steps`.
/// Assistant messages in all formats carry a single usage counter for the
/// whole message even though agx emits multiple steps (text + tool_uses).
/// Attaching to the first step avoids double-counting when summing a corpus.
pub(crate) fn attach_usage_to_first(
    steps: &mut [Step],
    start: usize,
    model: Option<&str>,
    usage: &Usage,
) {
    if let Some(step) = steps.get_mut(start) {
        if let Some(m) = model {
            step.model = Some(m.to_string());
        }
        if !usage.is_empty() {
            step.tokens_in = usage.tokens_in;
            step.tokens_out = usage.tokens_out;
            step.cache_read = usage.cache_read;
            step.cache_create = usage.cache_create;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    #[default]
    UserText,
    ToolResult,
    AssistantText,
    ToolUse,
}

#[derive(Debug, Default)]
pub struct StepCounts {
    pub user: usize,
    pub assistant: usize,
    pub tool_uses: usize,
    pub tool_results: usize,
}

// Heuristic: does this step look like a failed tool call?
// Only examines ToolResult steps. Extracts the "Result:" section of the
// step detail and scans for substring indicators common across Claude Code,
// Codex, and Gemini error outputs. Conservative — prefers false negatives
// over false positives so users can trust the red marker.
pub fn is_error_result(step: &Step) -> bool {
    const INDICATORS: &[&str] = &[
        "\"error\"",
        "error:",
        " failed",
        "\nfailed",
        "traceback",
        "panic!",
        "exception:",
        "no such file",
        "permission denied",
        "command failed",
        "exit code 1",
        "exit code 2",
        "exit code 3",
        "exit code 4",
        "exit code 5",
        "exit code 6",
        "exit code 7",
        "exit code 8",
        "exit code 9",
        "process exited with code 1",
        "process exited with code 2",
    ];
    if step.kind != StepKind::ToolResult {
        return false;
    }
    let haystack = step
        .detail
        .split("\nResult:\n")
        .nth(1)
        .unwrap_or(&step.detail)
        .to_lowercase();
    INDICATORS.iter().any(|kw| haystack.contains(kw))
}

pub fn count_from_steps(steps: &[Step]) -> StepCounts {
    let mut c = StepCounts::default();
    for step in steps {
        match step.kind {
            StepKind::UserText => c.user += 1,
            StepKind::AssistantText => c.assistant += 1,
            StepKind::ToolUse => c.tool_uses += 1,
            StepKind::ToolResult => c.tool_results += 1,
        }
    }
    c
}

#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    pub name: String,
    pub use_count: usize,
    pub result_count: usize,
    pub error_count: usize,
}

impl ToolStats {
    pub fn error_rate(&self) -> Option<f64> {
        if self.result_count == 0 {
            None
        } else {
            #[allow(clippy::cast_precision_loss)]
            Some(self.error_count as f64 / self.result_count as f64)
        }
    }
}

/// Aggregate per-tool statistics from a timeline. Returns a vector of
/// `ToolStats` sorted by `use_count` descending.
pub fn compute_tool_stats(steps: &[Step]) -> Vec<ToolStats> {
    let mut map: HashMap<String, ToolStats> = HashMap::new();
    for step in steps {
        let Some(name) = &step.tool_name else {
            continue;
        };
        let entry = map.entry(name.clone()).or_insert_with(|| ToolStats {
            name: name.clone(),
            ..ToolStats::default()
        });
        match step.kind {
            StepKind::ToolUse => entry.use_count += 1,
            StepKind::ToolResult => {
                entry.result_count += 1;
                if is_error_result(step) {
                    entry.error_count += 1;
                }
            }
            _ => {}
        }
    }
    let mut stats: Vec<ToolStats> = map.into_values().collect();
    stats.sort_by(|a, b| {
        b.use_count
            .cmp(&a.use_count)
            .then_with(|| a.name.cmp(&b.name))
    });
    stats
}

#[derive(Debug, Clone)]
struct ToolMeta {
    name: String,
    input_pretty: String,
}

/// Entries that share a `parentUuid` (including the implicit `None` root)
/// form a fork. Returns the set of uuids that are fork roots — the
/// entries whose originating step should be flagged `is_fork_root`.
///
/// Rules:
/// - A parent with ≥2 children: every child is a fork root.
/// - >1 root-level entries (parent_uuid == None): every root is a fork root.
/// - A parent with exactly 1 child: normal linear continuation, not a fork.
///
/// Kept Claude-Code-private because only `session::Entry` carries
/// `parentUuid`; other formats don't have the concept.
fn collect_fork_root_uuids(entries: &[Entry]) -> std::collections::HashSet<&str> {
    use std::collections::HashMap;
    let mut children_by_parent: HashMap<Option<&str>, Vec<&str>> = HashMap::new();
    for entry in entries {
        let (uuid, parent) = match entry {
            Entry::User(u) => (u.uuid.as_str(), u.parent_uuid.as_deref()),
            Entry::Assistant(a) => (a.uuid.as_str(), a.parent_uuid.as_deref()),
            Entry::Other => continue,
        };
        children_by_parent.entry(parent).or_default().push(uuid);
    }
    children_by_parent
        .into_iter()
        .filter(|(_, children)| children.len() > 1)
        .flat_map(|(_, children)| children.into_iter())
        .collect()
}

/// Count of fork-root steps in a timeline. Useful for status-bar hints
/// ("[forks: N]") without re-walking the steps every frame.
#[must_use]
pub fn fork_root_count(steps: &[Step]) -> usize {
    steps.iter().filter(|s| s.is_fork_root).count()
}

/// Indices (into `steps`) of every fork-root step, in the order they
/// appear. The TUI's fork-list overlay walks this to let users
/// jump-to-fork.
#[must_use]
pub fn fork_root_indices(steps: &[Step]) -> Vec<usize> {
    steps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.is_fork_root)
        .map(|(i, _)| i)
        .collect()
}

pub fn build(entries: &[Entry]) -> Vec<Step> {
    let tool_meta = collect_tool_meta(entries);
    // Count children per parent_uuid to detect branch roots. An entry is
    // a fork root when it shares its `parentUuid` with ≥1 other entry —
    // i.e. the conversation forked at that parent. Also count
    // root-level entries (parent_uuid = None): if there are >1, the
    // file contains multiple independent conversation threads, each
    // root is a fork root. Built in one pass so `build()` stays O(N).
    let fork_uuids = collect_fork_root_uuids(entries);
    let mut steps = Vec::new();
    for entry in entries {
        match entry {
            Entry::User(u) => {
                let ts = u.timestamp.as_deref().and_then(parse_iso_ms);
                let is_fork = fork_uuids.contains(u.uuid.as_str());
                let entry_first_idx = steps.len();
                match &u.message.content {
                    UserContent::Text(text) => {
                        let mut step = user_text_step(text);
                        step.timestamp_ms = ts;
                        steps.push(step);
                    }
                    UserContent::Items(items) => {
                        for item in items {
                            match item {
                                UserContentItem::Text { text } => {
                                    let mut step = user_text_step(text);
                                    step.timestamp_ms = ts;
                                    steps.push(step);
                                }
                                UserContentItem::ToolResult {
                                    tool_use_id,
                                    content,
                                } => {
                                    let result_text = match content {
                                        ToolResultContent::Text(s) => s.clone(),
                                        ToolResultContent::Items(v) => pretty_json(v),
                                    };
                                    let meta = tool_meta.get(tool_use_id);
                                    let mut step = tool_result_step(
                                        tool_use_id,
                                        &result_text,
                                        meta.map(|m| m.name.as_str()),
                                        meta.map(|m| m.input_pretty.as_str()),
                                    );
                                    step.timestamp_ms = ts;
                                    steps.push(step);
                                }
                                UserContentItem::Other => {}
                            }
                        }
                    }
                }
                if is_fork && let Some(first) = steps.get_mut(entry_first_idx) {
                    first.is_fork_root = true;
                }
            }
            Entry::Assistant(a) => {
                let ts = a.timestamp.as_deref().and_then(parse_iso_ms);
                let first_idx = steps.len();
                let is_fork = fork_uuids.contains(a.uuid.as_str());
                for item in &a.message.content {
                    match item {
                        AssistantContentItem::Text { text } => {
                            let mut step = assistant_text_step(text);
                            step.timestamp_ms = ts;
                            steps.push(step);
                        }
                        AssistantContentItem::ToolUse { id, name, input } => {
                            let input_pretty = pretty_json(input);
                            let mut step = tool_use_step(id, name, &input_pretty);
                            step.timestamp_ms = ts;
                            steps.push(step);
                        }
                        AssistantContentItem::Other => {}
                    }
                }
                if is_fork && let Some(first) = steps.get_mut(first_idx) {
                    first.is_fork_root = true;
                }
                // If this assistant message produced at least one step, attach
                // model + usage to the first of them. Format parsers across
                // agx follow this same convention to avoid double-counting
                // when summing a corpus.
                if steps.len() > first_idx {
                    let usage = a
                        .message
                        .usage
                        .as_ref()
                        .map(|u| Usage {
                            tokens_in: u.input_tokens,
                            tokens_out: u.output_tokens,
                            cache_read: u.cache_read_input_tokens,
                            cache_create: u.cache_creation_input_tokens,
                        })
                        .unwrap_or_default();
                    attach_usage_to_first(
                        &mut steps,
                        first_idx,
                        a.message.model.as_deref(),
                        &usage,
                    );
                }
            }
            Entry::Other => {}
        }
    }
    compute_durations(&mut steps);
    steps
}

pub fn user_text_step(text: &str) -> Step {
    Step {
        label: format!("[user]   {}", truncate(text, LABEL_PREVIEW_WIDTH)),
        detail: text.to_string(),
        kind: StepKind::UserText,
        ..Step::default()
    }
}

pub fn assistant_text_step(text: &str) -> Step {
    Step {
        label: format!("[asst]   {}", truncate(text, LABEL_PREVIEW_WIDTH)),
        detail: text.to_string(),
        kind: StepKind::AssistantText,
        ..Step::default()
    }
}

pub fn tool_use_step(id: &str, name: &str, input_pretty: &str) -> Step {
    Step {
        label: format!("[tool]   {} ({})", name, short_id(id)),
        detail: format!("Tool: {name}\nID: {id}\n\nInput:\n{input_pretty}"),
        kind: StepKind::ToolUse,
        tool_name: Some(name.to_string()),
        tool_call_id: Some(id.to_string()),
        ..Step::default()
    }
}

pub fn tool_result_step(
    id: &str,
    result: &str,
    tool_name: Option<&str>,
    input_pretty: Option<&str>,
) -> Step {
    let display_name = tool_name.unwrap_or("(unknown)");
    let input_section = input_pretty
        .map(|p| format!("Input:\n{p}\n\n"))
        .unwrap_or_default();
    Step {
        label: format!(
            "[result] {} → {}",
            display_name,
            truncate(result, RESULT_PREVIEW_WIDTH)
        ),
        detail: format!("Tool: {display_name}\nID: {id}\n\n{input_section}Result:\n{result}"),
        kind: StepKind::ToolResult,
        tool_name: tool_name.map(str::to_string),
        tool_call_id: Some(id.to_string()),
        ..Step::default()
    }
}

/// Compute sequential duration for each step (time since previous step).
pub fn compute_durations(steps: &mut [Step]) {
    for i in 1..steps.len() {
        if let (Some(prev), Some(cur)) = (steps[i - 1].timestamp_ms, steps[i].timestamp_ms)
            && cur >= prev
        {
            steps[i].duration_ms = Some(cur - prev);
        }
    }
}

/// Format a duration in ms to a compact human-readable string.
#[allow(clippy::cast_precision_loss)]
pub fn format_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{:.1}min", ms as f64 / 60_000.0)
    }
}

/// Parse ISO 8601 UTC timestamp to unix milliseconds. Handles
/// `YYYY-MM-DDTHH:MM:SS[.fff][Z]` — the format all three CLIs produce.
#[allow(
    clippy::many_single_char_names,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
pub(crate) fn parse_iso_ms(s: &str) -> Option<u64> {
    if s.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: u64 = s.get(5..7)?.parse().ok()?;
    let d: u64 = s.get(8..10)?.parse().ok()?;
    let h: u64 = s.get(11..13)?.parse().ok()?;
    let mi: u64 = s.get(14..16)?.parse().ok()?;
    let se: u64 = s.get(17..19)?.parse().ok()?;

    // Howard Hinnant's days_from_civil
    let (adj_y, adj_m) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = if adj_y >= 0 { adj_y } else { adj_y - 399 } / 400;
    let yoe = (adj_y - era * 400) as u64;
    let doy = (153 * adj_m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = (era * 146_097 + doe as i64 - 719_468) as u64;

    let secs = days * 86_400 + h * 3_600 + mi * 60 + se;

    // Fractional ms after the seconds
    let bytes = s.as_bytes();
    let ms = if bytes.len() > 19 && bytes[19] == b'.' {
        let end = bytes[20..]
            .iter()
            .position(|c| !c.is_ascii_digit())
            .map_or(bytes.len(), |p| 20 + p);
        let frac = s.get(20..end)?;
        if frac.is_empty() {
            0
        } else {
            let mut val: u64 = frac.parse().ok()?;
            match frac.len() {
                1 => val *= 100,
                2 => val *= 10,
                3 => {}
                n => {
                    val /= 10u64.pow(u32::try_from(n - 3).unwrap_or(0));
                }
            }
            val
        }
    } else {
        0
    };

    Some(secs * 1_000 + ms)
}

fn collect_tool_meta(entries: &[Entry]) -> HashMap<String, ToolMeta> {
    let mut map = HashMap::new();
    for entry in entries {
        if let Entry::Assistant(a) = entry {
            for item in &a.message.content {
                if let AssistantContentItem::ToolUse { id, name, input } = item {
                    map.insert(
                        id.clone(),
                        ToolMeta {
                            name: name.clone(),
                            input_pretty: pretty_json(input),
                        },
                    );
                }
            }
        }
    }
    map
}

pub(crate) fn pretty_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub fn truncate(s: &str, n: usize) -> String {
    let mut head = String::with_capacity(n);
    let mut iter = s.chars().map(|c| if c == '\n' { ' ' } else { c });
    for _ in 0..n {
        match iter.next() {
            Some(c) => head.push(c),
            None => return head,
        }
    }
    if iter.next().is_some() {
        head.push('…');
    }
    head
}

pub(crate) fn short_id(id: &str) -> String {
    // Char-based bounds — `&id[..11]` would panic if the id contains a
    // multi-byte UTF-8 codepoint whose bytes straddle index 11 (and
    // `tool_use_id` is attacker-controllable via session files).
    // Preserves the original byte-length contract: strings of ≤12
    // chars pass through unchanged; longer ones truncate to 11 chars
    // plus an ellipsis.
    if id.chars().count() <= 12 {
        id.to_string()
    } else {
        let head: String = id.chars().take(11).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{AssistantEntry, AssistantMessage, UserContent, UserEntry, UserMessage};

    #[test]
    fn builds_steps_from_user_and_assistant() {
        let entries = vec![
            Entry::User(UserEntry {
                uuid: "u1".into(),
                parent_uuid: None,
                timestamp: None,
                message: UserMessage {
                    role: "user".into(),
                    content: UserContent::Text("hello world".into()),
                },
            }),
            Entry::Assistant(AssistantEntry {
                uuid: "a1".into(),
                parent_uuid: Some("u1".into()),
                timestamp: None,
                message: AssistantMessage {
                    role: "assistant".into(),
                    model: None,
                    usage: None,
                    content: vec![
                        AssistantContentItem::Text {
                            text: "thinking".into(),
                        },
                        AssistantContentItem::ToolUse {
                            id: "toolu_abc".into(),
                            name: "Read".into(),
                            input: serde_json::json!({"file_path": "/x"}),
                        },
                    ],
                },
            }),
        ];
        let steps = build(&entries);
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert_eq!(steps[2].kind, StepKind::ToolUse);
        assert!(steps[2].detail.contains("Read"));
        assert!(steps[2].detail.contains("/x"));
    }

    #[test]
    fn usage_attaches_only_to_first_step_from_assistant_message() {
        // Assistant message with text + tool_use. Usage applies to the whole
        // message; only the first (text) step should carry the numbers so a
        // corpus sum doesn't double-count.
        use crate::session::{AssistantEntry, AssistantMessage, ClaudeUsage};
        let entries = vec![Entry::Assistant(AssistantEntry {
            uuid: "a1".into(),
            parent_uuid: None,
            timestamp: None,
            message: AssistantMessage {
                role: "assistant".into(),
                model: Some("claude-opus-4-6".into()),
                usage: Some(ClaudeUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    cache_creation_input_tokens: Some(10),
                    cache_read_input_tokens: Some(200),
                }),
                content: vec![
                    AssistantContentItem::Text {
                        text: "thinking".into(),
                    },
                    AssistantContentItem::ToolUse {
                        id: "t1".into(),
                        name: "Read".into(),
                        input: serde_json::json!({"path": "/x"}),
                    },
                ],
            },
        })];
        let steps = build(&entries);
        assert_eq!(steps.len(), 2);
        // First step carries everything
        assert_eq!(steps[0].model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(steps[0].tokens_in, Some(100));
        assert_eq!(steps[0].tokens_out, Some(50));
        assert_eq!(steps[0].cache_create, Some(10));
        assert_eq!(steps[0].cache_read, Some(200));
        // Second step carries nothing
        assert_eq!(steps[1].model, None);
        assert_eq!(steps[1].tokens_in, None);
        assert_eq!(steps[1].tokens_out, None);
    }

    #[test]
    fn missing_usage_leaves_all_steps_clean() {
        use crate::session::{AssistantEntry, AssistantMessage};
        let entries = vec![Entry::Assistant(AssistantEntry {
            uuid: "a1".into(),
            parent_uuid: None,
            timestamp: None,
            message: AssistantMessage {
                role: "assistant".into(),
                model: None,
                usage: None,
                content: vec![AssistantContentItem::Text { text: "ok".into() }],
            },
        })];
        let steps = build(&entries);
        assert_eq!(steps[0].tokens_in, None);
        assert_eq!(steps[0].model, None);
    }

    #[test]
    fn attach_usage_to_first_noop_on_empty_slice() {
        let mut steps: Vec<Step> = Vec::new();
        // Should not panic.
        attach_usage_to_first(&mut steps, 0, Some("m"), &Usage::default());
    }

    // -------- Phase 5.1 fork detection --------

    fn user_entry(uuid: &str, parent: Option<&str>, text: &str) -> Entry {
        Entry::User(UserEntry {
            uuid: uuid.into(),
            parent_uuid: parent.map(str::to_string),
            timestamp: None,
            message: UserMessage {
                role: "user".into(),
                content: UserContent::Text(text.into()),
            },
        })
    }

    fn asst_entry(uuid: &str, parent: Option<&str>, text: &str) -> Entry {
        Entry::Assistant(AssistantEntry {
            uuid: uuid.into(),
            parent_uuid: parent.map(str::to_string),
            timestamp: None,
            message: AssistantMessage {
                role: "assistant".into(),
                model: None,
                usage: None,
                content: vec![AssistantContentItem::Text { text: text.into() }],
            },
        })
    }

    #[test]
    fn linear_session_has_no_fork_roots() {
        let entries = vec![
            user_entry("u1", None, "hi"),
            asst_entry("a1", Some("u1"), "hello"),
            user_entry("u2", Some("a1"), "and"),
            asst_entry("a2", Some("u2"), "ok"),
        ];
        let steps = build(&entries);
        assert_eq!(fork_root_count(&steps), 0);
        assert!(fork_root_indices(&steps).is_empty());
    }

    #[test]
    fn two_siblings_of_same_parent_are_fork_roots() {
        // u1 has two children: a1 and a2 — both are fork roots. The
        // assistant's first emitted step carries the marker.
        let entries = vec![
            user_entry("u1", None, "prompt"),
            asst_entry("a1", Some("u1"), "first reply"),
            asst_entry("a2", Some("u1"), "second reply"),
        ];
        let steps = build(&entries);
        assert_eq!(fork_root_count(&steps), 2);
        assert!(steps[1].is_fork_root);
        assert!(steps[2].is_fork_root);
        assert!(!steps[0].is_fork_root);
    }

    #[test]
    fn multiple_root_entries_are_all_fork_roots() {
        // Two independent conversation roots in one file — every root
        // is a fork root because parent_uuid=None shows up >1 time.
        let entries = vec![
            user_entry("u1", None, "thread 1"),
            user_entry("u2", None, "thread 2"),
        ];
        let steps = build(&entries);
        assert_eq!(fork_root_count(&steps), 2);
        assert_eq!(fork_root_indices(&steps), vec![0, 1]);
    }

    #[test]
    fn single_root_entry_is_not_a_fork() {
        let entries = vec![user_entry("u1", None, "only thread")];
        let steps = build(&entries);
        assert_eq!(fork_root_count(&steps), 0);
    }

    #[test]
    fn fork_root_marker_only_on_first_emitted_step() {
        // A forked assistant message with two content items emits two
        // steps — the marker should land on the first one only.
        use crate::session::{AssistantEntry, AssistantMessage};
        let entries = vec![
            user_entry("u1", None, "prompt"),
            Entry::Assistant(AssistantEntry {
                uuid: "a1".into(),
                parent_uuid: Some("u1".into()),
                timestamp: None,
                message: AssistantMessage {
                    role: "assistant".into(),
                    model: None,
                    usage: None,
                    content: vec![
                        AssistantContentItem::Text {
                            text: "thinking".into(),
                        },
                        AssistantContentItem::ToolUse {
                            id: "t1".into(),
                            name: "Read".into(),
                            input: serde_json::json!({}),
                        },
                    ],
                },
            }),
            asst_entry("a2", Some("u1"), "alt reply"),
        ];
        let steps = build(&entries);
        // a1 emits 2 steps (text + tool_use), a2 emits 1. Forks are a1 and a2.
        assert_eq!(steps.len(), 4);
        assert!(steps[1].is_fork_root, "first step from a1 should be marked");
        assert!(
            !steps[2].is_fork_root,
            "subsequent step from same entry should not be marked"
        );
        assert!(steps[3].is_fork_root);
    }

    #[test]
    fn usage_is_empty_detects_all_none() {
        assert!(Usage::default().is_empty());
        let u = Usage {
            tokens_in: Some(1),
            ..Usage::default()
        };
        assert!(!u.is_empty());
    }

    #[test]
    fn step_cost_usd_delegates_to_pricing_table() {
        let mut step = assistant_text_step("hi");
        step.model = Some("claude-opus-4-6".into());
        step.tokens_in = Some(1_000_000);
        step.tokens_out = Some(1_000_000);
        let c = step.cost_usd().unwrap();
        assert!((c - 90.0).abs() < 1e-6);
    }

    #[test]
    fn step_cost_usd_none_when_no_model() {
        let mut step = assistant_text_step("hi");
        step.tokens_in = Some(100);
        assert_eq!(step.cost_usd(), None);
    }

    #[test]
    fn step_cost_usd_none_when_no_tokens() {
        let mut step = assistant_text_step("hi");
        step.model = Some("claude-opus-4-6".into());
        assert_eq!(step.cost_usd(), None);
    }

    #[test]
    fn session_totals_sums_tokens_and_costs_across_steps() {
        let mut s1 = assistant_text_step("a");
        s1.model = Some("claude-opus-4-6".into());
        s1.tokens_in = Some(100);
        s1.tokens_out = Some(50);
        let mut s2 = assistant_text_step("b");
        s2.model = Some("claude-opus-4-6".into());
        s2.tokens_in = Some(200);
        s2.tokens_out = Some(75);
        let t = compute_session_totals(&[s1, s2]);
        assert_eq!(t.tokens_in, 300);
        assert_eq!(t.tokens_out, 125);
        assert_eq!(t.unique_models, vec!["claude-opus-4-6"]);
        assert!(t.cost_usd.is_some());
    }

    #[test]
    fn session_totals_dedupes_unique_models() {
        let mut s1 = assistant_text_step("a");
        s1.model = Some("claude-opus-4-6".into());
        let mut s2 = assistant_text_step("b");
        s2.model = Some("claude-sonnet-4-6".into());
        let mut s3 = assistant_text_step("c");
        s3.model = Some("claude-opus-4-6".into());
        let t = compute_session_totals(&[s1, s2, s3]);
        assert_eq!(t.unique_models.len(), 2);
        assert!(t.unique_models.contains(&"claude-opus-4-6".to_string()));
        assert!(t.unique_models.contains(&"claude-sonnet-4-6".to_string()));
    }

    #[test]
    fn session_totals_cost_none_when_no_known_models() {
        let mut s = assistant_text_step("a");
        s.model = Some("unknown-model".into());
        s.tokens_in = Some(100);
        let t = compute_session_totals(&[s]);
        assert_eq!(t.tokens_in, 100);
        assert_eq!(t.cost_usd, None);
    }

    #[test]
    fn session_totals_has_tokens_false_on_empty() {
        let t = SessionTotals::default();
        assert!(!t.has_tokens());
    }

    #[test]
    fn session_totals_has_tokens_true_when_any_counter_set() {
        let t = SessionTotals {
            tokens_in: 1,
            ..SessionTotals::default()
        };
        assert!(t.has_tokens());
    }

    #[test]
    fn tool_result_label_uses_tool_name_from_paired_use() {
        let entries = vec![
            Entry::Assistant(AssistantEntry {
                uuid: "a1".into(),
                parent_uuid: None,
                timestamp: None,
                message: AssistantMessage {
                    role: "assistant".into(),
                    model: None,
                    usage: None,
                    content: vec![AssistantContentItem::ToolUse {
                        id: "toolu_abc".into(),
                        name: "Bash".into(),
                        input: serde_json::json!({"command": "ls"}),
                    }],
                },
            }),
            Entry::User(UserEntry {
                uuid: "u2".into(),
                parent_uuid: Some("a1".into()),
                timestamp: None,
                message: UserMessage {
                    role: "user".into(),
                    content: UserContent::Items(vec![UserContentItem::ToolResult {
                        tool_use_id: "toolu_abc".into(),
                        content: ToolResultContent::Text("file1\nfile2".into()),
                    }]),
                },
            }),
        ];
        let steps = build(&entries);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].kind, StepKind::ToolResult);
        assert!(
            steps[1].label.contains("Bash"),
            "expected label to include tool name, got: {}",
            steps[1].label
        );
        assert!(steps[1].detail.contains("Tool: Bash"));
        assert!(steps[1].detail.contains("Input:"));
        assert!(steps[1].detail.contains("\"command\""));
        assert!(steps[1].detail.contains("Result:"));
        assert!(steps[1].detail.contains("file1"));
    }

    #[test]
    fn tool_result_falls_back_when_no_paired_use() {
        let entries = vec![Entry::User(UserEntry {
            uuid: "u1".into(),
            parent_uuid: None,
            timestamp: None,
            message: UserMessage {
                role: "user".into(),
                content: UserContent::Items(vec![UserContentItem::ToolResult {
                    tool_use_id: "toolu_orphan".into(),
                    content: ToolResultContent::Text("output".into()),
                }]),
            },
        })];
        let steps = build(&entries);
        assert_eq!(steps.len(), 1);
        assert!(steps[0].label.contains("(unknown)"));
        assert!(steps[0].detail.contains("Tool: (unknown)"));
        assert!(!steps[0].detail.contains("Input:"));
        assert!(steps[0].detail.contains("Result:"));
    }

    #[test]
    fn count_from_steps_works() {
        let steps = vec![
            user_text_step("hi"),
            assistant_text_step("hello"),
            tool_use_step("id1", "Read", "{}"),
            tool_result_step("id1", "output", Some("Read"), Some("{}")),
            tool_use_step("id2", "Bash", "{}"),
        ];
        let c = count_from_steps(&steps);
        assert_eq!(c.user, 1);
        assert_eq!(c.assistant, 1);
        assert_eq!(c.tool_uses, 2);
        assert_eq!(c.tool_results, 1);
    }

    #[test]
    fn truncate_handles_short_strings() {
        assert_eq!(truncate("hi", 10), "hi");
    }

    #[test]
    fn truncate_handles_long_strings() {
        let s = "a".repeat(20);
        assert_eq!(truncate(&s, 5), "aaaaa…");
    }

    #[test]
    fn truncate_replaces_newlines() {
        assert_eq!(truncate("a\nb\nc", 10), "a b c");
    }

    #[test]
    fn truncate_handles_exact_length() {
        assert_eq!(truncate("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_handles_unicode() {
        assert_eq!(truncate("héllo", 3), "hél…");
        assert_eq!(truncate("héllo世界", 5), "héllo…");
        assert_eq!(truncate("héllo世界", 6), "héllo世…");
        assert_eq!(truncate("héllo世界", 7), "héllo世界");
    }

    #[test]
    fn short_id_passes_short_strings_through() {
        assert_eq!(short_id(""), "");
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(short_id("toolu_abcde"), "toolu_abcde");
    }

    #[test]
    fn short_id_truncates_long_strings_at_eleven() {
        assert_eq!(short_id("toolu_0123456789xyz"), "toolu_01234…");
        assert_eq!(short_id("toolu_abcdefghijkl"), "toolu_abcde…");
    }

    #[test]
    fn short_id_handles_exact_twelve_boundary() {
        assert_eq!(short_id("123456789012"), "123456789012");
        assert_eq!(short_id("1234567890123"), "12345678901…");
    }

    #[test]
    fn short_id_does_not_panic_on_multibyte_utf8_near_boundary() {
        // The old `&id[..11]` slice would panic on an input where a
        // 4-byte emoji begins at byte index 11 (byte index falls
        // inside the codepoint). 11 ASCII chars + 😀 + "xyz" = 15
        // chars / 18 bytes, so it's long enough to force truncation.
        let id = "abcdefghijk😀xyz";
        let out = short_id(id);
        // Takes first 11 chars cleanly and appends the ellipsis.
        assert_eq!(out, "abcdefghijk…");
    }

    fn result_step_with_body(body: &str) -> Step {
        tool_result_step("t1", body, Some("Bash"), Some("{}"))
    }

    #[test]
    fn is_error_result_detects_error_keyword() {
        let step = result_step_with_body("error: file not found");
        assert!(is_error_result(&step));
    }

    #[test]
    fn is_error_result_detects_failed_word() {
        let step = result_step_with_body("Command failed with exit code 1");
        assert!(is_error_result(&step));
    }

    #[test]
    fn is_error_result_detects_traceback() {
        let step = result_step_with_body("Traceback (most recent call last):\n  ...");
        assert!(is_error_result(&step));
    }

    #[test]
    fn is_error_result_detects_no_such_file() {
        let step = result_step_with_body("ls: /nonexistent: No such file or directory");
        assert!(is_error_result(&step));
    }

    #[test]
    fn is_error_result_detects_exit_code_nonzero() {
        let step = result_step_with_body("Process exited with code 127");
        // Not in our list — we check 1-9 and 1-2 for process exited
        // For "exit code 127" the substring "exit code 1" matches, so it's detected
        assert!(is_error_result(&step));
    }

    #[test]
    fn is_error_result_detects_json_error_field() {
        let step = result_step_with_body("{\"error\": \"bad request\"}");
        assert!(is_error_result(&step));
    }

    #[test]
    fn is_error_result_returns_false_for_clean_output() {
        let step = result_step_with_body("[0, 1, 1, 2, 3, 5, 8, 13, 21, 34]");
        assert!(!is_error_result(&step));
    }

    #[test]
    fn is_error_result_returns_false_for_non_tool_result() {
        let step = user_text_step("error in my user message");
        assert!(!is_error_result(&step));
    }

    #[test]
    fn is_error_result_only_checks_result_section() {
        // Input section mentions "error" but Result section is clean.
        let step = tool_result_step(
            "t1",
            "all good",
            Some("Bash"),
            Some("{\"command\": \"grep error\"}"),
        );
        assert!(!is_error_result(&step));
    }

    #[test]
    fn tool_use_step_records_tool_name() {
        let s = tool_use_step("t1", "Read", "{}");
        assert_eq!(s.tool_name.as_deref(), Some("Read"));
    }

    #[test]
    fn tool_result_step_records_tool_name() {
        let s = tool_result_step("t1", "ok", Some("Bash"), Some("{}"));
        assert_eq!(s.tool_name.as_deref(), Some("Bash"));
    }

    #[test]
    fn tool_result_step_tool_name_none_for_orphan() {
        let s = tool_result_step("t1", "ok", None, None);
        assert_eq!(s.tool_name, None);
    }

    #[test]
    fn text_steps_have_no_tool_name() {
        assert_eq!(user_text_step("hi").tool_name, None);
        assert_eq!(assistant_text_step("ok").tool_name, None);
    }

    #[test]
    fn compute_tool_stats_groups_by_tool_name() {
        let steps = vec![
            tool_use_step("t1", "Read", "{}"),
            tool_result_step("t1", "content", Some("Read"), Some("{}")),
            tool_use_step("t2", "Read", "{}"),
            tool_result_step("t2", "content2", Some("Read"), Some("{}")),
            tool_use_step("t3", "Bash", "{}"),
            tool_result_step("t3", "output", Some("Bash"), Some("{}")),
        ];
        let stats = compute_tool_stats(&steps);
        assert_eq!(stats.len(), 2);
        // Read should come first (2 uses vs 1)
        assert_eq!(stats[0].name, "Read");
        assert_eq!(stats[0].use_count, 2);
        assert_eq!(stats[0].result_count, 2);
        assert_eq!(stats[0].error_count, 0);
        assert_eq!(stats[1].name, "Bash");
        assert_eq!(stats[1].use_count, 1);
    }

    #[test]
    fn compute_tool_stats_counts_errors() {
        let steps = vec![
            tool_use_step("t1", "Bash", "{}"),
            tool_result_step("t1", "error: command failed", Some("Bash"), Some("{}")),
            tool_use_step("t2", "Bash", "{}"),
            tool_result_step("t2", "success", Some("Bash"), Some("{}")),
        ];
        let stats = compute_tool_stats(&steps);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].use_count, 2);
        assert_eq!(stats[0].error_count, 1);
        assert_eq!(stats[0].error_rate(), Some(0.5));
    }

    #[test]
    fn compute_tool_stats_sorts_by_use_count_descending() {
        let steps = vec![
            tool_use_step("t1", "Apple", "{}"),
            tool_use_step("t2", "Banana", "{}"),
            tool_use_step("t3", "Banana", "{}"),
            tool_use_step("t4", "Banana", "{}"),
            tool_use_step("t5", "Cherry", "{}"),
            tool_use_step("t6", "Cherry", "{}"),
        ];
        let stats = compute_tool_stats(&steps);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].name, "Banana"); // 3 uses
        assert_eq!(stats[1].name, "Cherry"); // 2 uses
        assert_eq!(stats[2].name, "Apple"); // 1 use
    }

    #[test]
    fn compute_tool_stats_empty_for_text_only() {
        let steps = vec![user_text_step("hi"), assistant_text_step("hello")];
        let stats = compute_tool_stats(&steps);
        assert!(stats.is_empty());
    }

    #[test]
    fn tool_stats_error_rate_none_when_no_results() {
        let stats = ToolStats {
            name: "X".into(),
            use_count: 1,
            result_count: 0,
            error_count: 0,
        };
        assert_eq!(stats.error_rate(), None);
    }
}
