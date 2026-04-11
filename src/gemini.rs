use crate::timeline::{
    Step, assistant_text_step, pretty_json, tool_result_step, tool_use_step, user_text_step,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Session {
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default, rename = "toolCalls")]
    tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    args: serde_json::Value,
    #[serde(default)]
    result: serde_json::Value,
}

pub fn load(path: &Path) -> Result<Vec<Step>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading gemini session file: {}", path.display()))?;
    let session: Session = serde_json::from_str(&content)
        .with_context(|| format!("parsing gemini session file: {}", path.display()))?;

    let mut steps = Vec::new();
    for msg in &session.messages {
        match msg.msg_type.as_str() {
            "user" => {
                let text = extract_message_text(&msg.content);
                if !text.trim().is_empty() {
                    steps.push(user_text_step(&text));
                }
            }
            "gemini" => {
                let text = extract_message_text(&msg.content);
                if !text.trim().is_empty() {
                    steps.push(assistant_text_step(&text));
                }
                for tc in &msg.tool_calls {
                    let input_pretty = pretty_json(&tc.args);
                    steps.push(tool_use_step(&tc.id, &tc.name, &input_pretty));
                    let result_text = extract_gemini_tool_result(&tc.result);
                    steps.push(tool_result_step(
                        &tc.id,
                        &result_text,
                        Some(&tc.name),
                        Some(&input_pretty),
                    ));
                }
            }
            // "info" and unknown types (session end, cancellations, etc.) — skip
            _ => {}
        }
    }
    Ok(steps)
}

// Gemini message.content is polymorphic:
//   - a bare string for assistant messages
//   - a list of {text: "..."} objects for user messages
//   - sometimes empty/null when toolCalls are the real payload
fn extract_message_text(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

// Gemini toolCall.result is a list of wrappers:
//   [{functionResponse: {id, name, response: {output: "..."}}}, ...]
// Extract the first output string if possible; fall back to pretty-printed JSON
// so the detail pane always has something useful.
fn extract_gemini_tool_result(result: &serde_json::Value) -> String {
    if let Some(arr) = result.as_array() {
        for item in arr {
            if let Some(output) = item
                .get("functionResponse")
                .and_then(|fr| fr.get("response"))
                .and_then(|r| r.get("output"))
                .and_then(|o| o.as_str())
            {
                return output.to_string();
            }
        }
    }
    if let Some(s) = result.as_str() {
        return s.to_string();
    }
    if result.is_null() {
        return String::new();
    }
    pretty_json(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::StepKind;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parses_user_and_assistant_messages() {
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {"type": "user", "id": "m1", "content": [{"text": "hello"}]},
                {"type": "gemini", "id": "m2", "content": "hi there"}
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("hello"));
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert!(steps[1].detail.contains("hi there"));
    }

    #[test]
    fn splits_toolcall_into_tool_use_and_tool_result() {
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {
                    "type": "gemini",
                    "id": "m1",
                    "content": "Let me list the files.",
                    "toolCalls": [
                        {
                            "id": "tc1",
                            "name": "list_directory",
                            "args": {"dir_path": "."},
                            "result": [{"functionResponse": {"id": "tc1", "name": "list_directory", "response": {"output": "file1\nfile2"}}}]
                        }
                    ]
                }
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].kind, StepKind::AssistantText);
        assert!(steps[0].detail.contains("list the files"));
        assert_eq!(steps[1].kind, StepKind::ToolUse);
        assert!(steps[1].label.contains("list_directory"));
        assert!(steps[1].detail.contains("dir_path"));
        assert_eq!(steps[2].kind, StepKind::ToolResult);
        assert!(steps[2].label.contains("list_directory"));
        assert!(steps[2].detail.contains("Tool: list_directory"));
        assert!(steps[2].detail.contains("Input:"));
        assert!(steps[2].detail.contains("Result:"));
        assert!(steps[2].detail.contains("file1"));
    }

    #[test]
    fn skips_empty_assistant_content_when_only_toolcalls() {
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {
                    "type": "gemini",
                    "id": "m1",
                    "content": "",
                    "toolCalls": [
                        {"id": "tc1", "name": "Read", "args": {}, "result": []}
                    ]
                }
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        // Empty text is skipped, tool_use + tool_result still emitted
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::ToolUse);
        assert_eq!(steps[1].kind, StepKind::ToolResult);
    }

    #[test]
    fn skips_info_messages() {
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {"type": "info", "id": "m1", "content": "Request cancelled."},
                {"type": "user", "id": "m2", "content": [{"text": "retry"}]}
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::UserText);
    }

    #[test]
    fn falls_back_to_pretty_json_for_nonstandard_tool_result() {
        let json = r#"{
            "sessionId": "s1",
            "messages": [
                {
                    "type": "gemini",
                    "id": "m1",
                    "content": "",
                    "toolCalls": [
                        {"id": "tc1", "name": "weird", "args": {}, "result": {"some": "object"}}
                    ]
                }
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps[1].detail.contains("some"));
    }
}
