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
pub struct UserEntry {
    // Parsed but only read by tests + reserved for future tree-walking
    // (parent_uuid). timestamp + message are actively read from
    // timeline::build().
    #[allow(dead_code)]
    pub uuid: String,
    #[serde(rename = "parentUuid")]
    #[allow(dead_code)]
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub message: UserMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserMessage {
    /// Reserved for future role-aware rendering; serde parses it but
    /// no reader currently exists.
    #[allow(dead_code)]
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
pub struct AssistantEntry {
    #[allow(dead_code)]
    pub uuid: String,
    #[serde(rename = "parentUuid")]
    #[allow(dead_code)]
    pub parent_uuid: Option<String>,
    pub timestamp: Option<String>,
    pub message: AssistantMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessage {
    #[allow(dead_code)]
    pub role: String,
    pub content: Vec<AssistantContentItem>,
    /// Model name (e.g. "claude-opus-4-6"). Optional — older sessions may not
    /// include it at the message level.
    #[serde(default)]
    pub model: Option<String>,
    /// Usage counters for this assistant response. Applies to the whole
    /// message, not per-content-item.
    #[serde(default)]
    pub usage: Option<ClaudeUsage>,
}

/// Claude Code's usage shape, mirrored from Anthropic API responses.
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
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
    // Line-stream via BufReader so we never hold the whole file in memory
    // as a single String — a Claude Code session can be 50MB+ of JSONL,
    // and the old `read_to_string` + `.lines()` path materialized the
    // entire buffer just to iterate over it. BufReader keeps the working
    // set bounded by the longest single line (typically a few KB) while
    // still giving us accurate line-number context for format-drift
    // error messages.
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    let file =
        File::open(path).with_context(|| format!("opening session file: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries = Vec::with_capacity(1024);
    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("reading line {} of session file", i + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry: Entry = serde_json::from_str(&line)
            .with_context(|| format!("parsing line {} of session file", i + 1))?;
        entries.push(entry);
    }
    Ok(entries)
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
    fn parses_usage_and_model_on_assistant_message() {
        let line = r#"{"type":"assistant","uuid":"a1","parentUuid":null,"timestamp":null,"message":{"role":"assistant","model":"claude-opus-4-6","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":10,"cache_read_input_tokens":200},"content":[{"type":"text","text":"hi"}]}}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        let Entry::Assistant(a) = entry else {
            panic!("expected assistant");
        };
        assert_eq!(a.message.model.as_deref(), Some("claude-opus-4-6"));
        let u = a.message.usage.as_ref().unwrap();
        assert_eq!(u.input_tokens, Some(100));
        assert_eq!(u.output_tokens, Some(50));
        assert_eq!(u.cache_creation_input_tokens, Some(10));
        assert_eq!(u.cache_read_input_tokens, Some(200));
    }

    #[test]
    fn assistant_message_without_usage_parses_cleanly() {
        let line = r#"{"type":"assistant","uuid":"a1","parentUuid":null,"timestamp":null,"message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
        let entry: Entry = serde_json::from_str(line).unwrap();
        let Entry::Assistant(a) = entry else {
            panic!("expected assistant");
        };
        assert!(a.message.usage.is_none());
        assert!(a.message.model.is_none());
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
}
