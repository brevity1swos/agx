use crate::session::{
    AssistantContentItem, Entry, ToolResultContent, UserContent, UserContentItem,
};
use std::collections::HashMap;

const LABEL_PREVIEW_WIDTH: usize = 60;
const RESULT_PREVIEW_WIDTH: usize = 50;

#[derive(Debug, Clone)]
pub struct Step {
    pub label: String,
    pub detail: String,
    pub kind: StepKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    UserText,
    ToolResult,
    AssistantText,
    ToolUse,
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
            Entry::User(u) => match &u.message.content {
                UserContent::Text(text) => steps.push(user_text_step(text)),
                UserContent::Items(items) => {
                    for item in items {
                        match item {
                            UserContentItem::Text { text } => steps.push(user_text_step(text)),
                            UserContentItem::ToolResult {
                                tool_use_id,
                                content,
                            } => steps.push(tool_result_step(tool_use_id, content, &tool_meta)),
                            UserContentItem::Other => {}
                        }
                    }
                }
            },
            Entry::Assistant(a) => {
                for item in &a.message.content {
                    match item {
                        AssistantContentItem::Text { text } => {
                            steps.push(assistant_text_step(text));
                        }
                        AssistantContentItem::ToolUse { id, name, input } => {
                            steps.push(tool_use_step(id, name, input));
                        }
                        AssistantContentItem::Other => {}
                    }
                }
            }
            Entry::Other => {}
        }
    }
    steps
}

fn user_text_step(text: &str) -> Step {
    Step {
        label: format!("[user]   {}", truncate(text, LABEL_PREVIEW_WIDTH)),
        detail: text.to_string(),
        kind: StepKind::UserText,
    }
}

fn assistant_text_step(text: &str) -> Step {
    Step {
        label: format!("[asst]   {}", truncate(text, LABEL_PREVIEW_WIDTH)),
        detail: text.to_string(),
        kind: StepKind::AssistantText,
    }
}

fn tool_use_step(id: &str, name: &str, input: &serde_json::Value) -> Step {
    let pretty = pretty_json(input);
    Step {
        label: format!("[tool]   {} ({})", name, short_id(id)),
        detail: format!("Tool: {name}\nID: {id}\n\nInput:\n{pretty}"),
        kind: StepKind::ToolUse,
    }
}

fn tool_result_step(
    tool_use_id: &str,
    content: &ToolResultContent,
    tool_meta: &HashMap<String, ToolMeta>,
) -> Step {
    let result_text = match content {
        ToolResultContent::Text(s) => s.clone(),
        ToolResultContent::Items(v) => pretty_json(v),
    };
    let meta = tool_meta.get(tool_use_id);
    let display_name = meta.map_or("(unknown)", |m| m.name.as_str());
    let input_section = meta
        .map(|m| format!("Input:\n{}\n\n", m.input_pretty))
        .unwrap_or_default();
    Step {
        label: format!(
            "[result] {} → {}",
            display_name,
            truncate(&result_text, RESULT_PREVIEW_WIDTH)
        ),
        detail: format!(
            "Tool: {display_name}\nID: {tool_use_id}\n\n{input_section}Result:\n{result_text}"
        ),
        kind: StepKind::ToolResult,
    }
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

fn pretty_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn truncate(s: &str, n: usize) -> String {
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

fn short_id(id: &str) -> String {
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
}
