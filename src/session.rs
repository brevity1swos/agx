use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Entry {
    User(UserEntry),
    Assistant(AssistantEntry),
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // uuid/parent_uuid/timestamp are parsed for future tree-walking and time-travel
pub struct UserEntry {
    pub uuid: String,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub message: UserMessage,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // role parsed for future role-aware rendering
pub struct UserMessage {
    pub role: String,
    pub content: UserContent,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    Text(String),
    Items(Vec<UserContentItem>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContentItem {
    Text {
        text: String,
    },
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Items(Vec<serde_json::Value>),
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // uuid/parent_uuid/timestamp are parsed for future tree-walking and time-travel
pub struct AssistantEntry {
    pub uuid: String,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub message: AssistantMessage,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // role parsed for future role-aware rendering
pub struct AssistantMessage {
    pub role: String,
    pub content: Vec<AssistantContentItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantContentItem {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

pub fn load(path: &Path) -> Result<Vec<Entry>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading session file: {}", path.display()))?;
    let mut entries = Vec::with_capacity(1024);
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let entry: Entry = serde_json::from_str(line)
            .with_context(|| format!("parsing line {} of session file", i + 1))?;
        entries.push(entry);
    }
    Ok(entries)
}

#[derive(Debug, Default)]
pub struct EntryCounts {
    pub user: usize,
    pub assistant: usize,
    pub other: usize,
    pub tool_uses: usize,
    pub tool_results: usize,
}

pub fn count(entries: &[Entry]) -> EntryCounts {
    let mut counts = EntryCounts::default();
    for entry in entries {
        match entry {
            Entry::User(u) => {
                counts.user += 1;
                if let UserContent::Items(items) = &u.message.content {
                    for item in items {
                        if matches!(item, UserContentItem::ToolResult { .. }) {
                            counts.tool_results += 1;
                        }
                    }
                }
            }
            Entry::Assistant(a) => {
                counts.assistant += 1;
                for item in &a.message.content {
                    if matches!(item, AssistantContentItem::ToolUse { .. }) {
                        counts.tool_uses += 1;
                    }
                }
            }
            Entry::Other => counts.other += 1,
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_user_message() {
        let line = r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-04-11T00:00:00Z","message":{"role":"user","content":"hello"}}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        let Entry::User(u) = entry else {
            panic!("expected user");
        };
        assert!(matches!(u.message.content, UserContent::Text(ref s) if s == "hello"));
    }

    #[test]
    fn parses_tool_result_user_message() {
        let line = r#"{"type":"user","uuid":"u2","parentUuid":"u1","timestamp":"2026-04-11T00:00:01Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"output"}]}}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        let Entry::User(u) = entry else {
            panic!("expected user");
        };
        let UserContent::Items(items) = u.message.content else {
            panic!("expected items");
        };
        assert_eq!(items.len(), 1);
        assert!(
            matches!(&items[0], UserContentItem::ToolResult { tool_use_id, .. } if tool_use_id == "t1")
        );
    }

    #[test]
    fn parses_assistant_with_tool_use() {
        let line = r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-04-11T00:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"thinking..."},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/x"}}]}}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        let Entry::Assistant(a) = entry else {
            panic!("expected assistant");
        };
        assert_eq!(a.message.content.len(), 2);
        assert!(
            matches!(&a.message.content[1], AssistantContentItem::ToolUse { name, .. } if name == "Read")
        );
    }

    #[test]
    fn unknown_top_level_type_becomes_other() {
        let line = r#"{"type":"permission-mode","permissionMode":"default","sessionId":"s1"}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        assert!(matches!(entry, Entry::Other));
    }

    #[test]
    fn unknown_assistant_content_item_becomes_other() {
        let line = r#"{"type":"assistant","uuid":"a2","parentUuid":null,"timestamp":null,"message":{"role":"assistant","content":[{"type":"thinking","content":"hmm"}]}}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        let Entry::Assistant(a) = entry else {
            panic!("expected assistant");
        };
        assert_eq!(a.message.content.len(), 1);
        assert!(matches!(&a.message.content[0], AssistantContentItem::Other));
    }

    #[test]
    fn counts_summarize_correctly() {
        let entries = vec![
            Entry::Other,
            Entry::User(UserEntry {
                uuid: "u1".into(),
                parent_uuid: None,
                timestamp: None,
                message: UserMessage {
                    role: "user".into(),
                    content: UserContent::Text("hi".into()),
                },
            }),
            Entry::Assistant(AssistantEntry {
                uuid: "a1".into(),
                parent_uuid: Some("u1".into()),
                timestamp: None,
                message: AssistantMessage {
                    role: "assistant".into(),
                    content: vec![AssistantContentItem::ToolUse {
                        id: "t1".into(),
                        name: "Read".into(),
                        input: serde_json::Value::Null,
                    }],
                },
            }),
        ];
        let c = count(&entries);
        assert_eq!(c.user, 1);
        assert_eq!(c.assistant, 1);
        assert_eq!(c.other, 1);
        assert_eq!(c.tool_uses, 1);
        assert_eq!(c.tool_results, 0);
    }
}
