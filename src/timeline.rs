use crate::session::{
    AssistantContentItem, Entry, ToolResultContent, UserContent, UserContentItem,
};
use std::collections::HashMap;

pub(crate) const LABEL_PREVIEW_WIDTH: usize = 60;
pub(crate) const RESULT_PREVIEW_WIDTH: usize = 50;

#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub detail: String,
    pub kind: StepKind,
    pub tool_name: Option<String>,
    pub timestamp_ms: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
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

pub fn build(entries: &[Entry]) -> Vec<Step> {
    let tool_meta = collect_tool_meta(entries);
    let mut steps = Vec::new();
    for entry in entries {
        match entry {
            Entry::User(u) => {
                let ts = u.timestamp.as_deref().and_then(parse_iso_ms);
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
            }
            Entry::Assistant(a) => {
                let ts = a.timestamp.as_deref().and_then(parse_iso_ms);
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
            }
            Entry::Other => {}
        }
    }
    compute_durations(&mut steps);
    steps
}

pub(crate) fn user_text_step(text: &str) -> Step {
    Step {
        label: format!("[user]   {}", truncate(text, LABEL_PREVIEW_WIDTH)),
        detail: text.to_string(),
        kind: StepKind::UserText,
        tool_name: None,
        timestamp_ms: None,
        duration_ms: None,
    }
}

pub(crate) fn assistant_text_step(text: &str) -> Step {
    Step {
        label: format!("[asst]   {}", truncate(text, LABEL_PREVIEW_WIDTH)),
        detail: text.to_string(),
        kind: StepKind::AssistantText,
        tool_name: None,
        timestamp_ms: None,
        duration_ms: None,
    }
}

pub(crate) fn tool_use_step(id: &str, name: &str, input_pretty: &str) -> Step {
    Step {
        label: format!("[tool]   {} ({})", name, short_id(id)),
        detail: format!("Tool: {name}\nID: {id}\n\nInput:\n{input_pretty}"),
        kind: StepKind::ToolUse,
        tool_name: Some(name.to_string()),
        timestamp_ms: None,
        duration_ms: None,
    }
}

pub(crate) fn tool_result_step(
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
        timestamp_ms: None,
        duration_ms: None,
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
pub(crate) fn format_duration_ms(ms: u64) -> String {
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

pub(crate) fn truncate(s: &str, n: usize) -> String {
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
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}…", &id[..11])
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
    fn tool_result_label_uses_tool_name_from_paired_use() {
        let entries = vec![
            Entry::Assistant(AssistantEntry {
                uuid: "a1".into(),
                parent_uuid: None,
                timestamp: None,
                message: AssistantMessage {
                    role: "assistant".into(),
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
