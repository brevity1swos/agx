use crate::timeline::{
    Step, assistant_text_step, compute_durations, pretty_json, tool_result_step, tool_use_step,
    user_text_step,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Conversation {
    #[serde(default)]
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: serde_json::Value,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
    #[serde(default)]
    tool_call_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ToolCall {
    #[serde(default)]
    id: String,
    #[serde(default)]
    function: ToolFunction,
}

#[derive(Debug, Default, Deserialize)]
struct ToolFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

pub fn load(path: &Path) -> Result<Vec<Step>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading conversation file: {}", path.display()))?;
    let conv: Conversation = serde_json::from_str(&content)
        .with_context(|| format!("parsing conversation file: {}", path.display()))?;

    let tool_meta = collect_tool_meta(&conv.messages);
    let mut steps = Vec::new();
    for msg in &conv.messages {
        match msg.role.as_str() {
            "user" => {
                let text = extract_text(&msg.content);
                if !text.trim().is_empty() {
                    steps.push(user_text_step(&text));
                }
            }
            "assistant" => {
                let text = extract_text(&msg.content);
                if !text.trim().is_empty() {
                    steps.push(assistant_text_step(&text));
                }
                for tc in &msg.tool_calls {
                    let input_pretty = prettify_arguments(&tc.function.arguments);
                    steps.push(tool_use_step(&tc.id, &tc.function.name, &input_pretty));
                }
            }
            "tool" => {
                let result_text = extract_text(&msg.content);
                let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                let meta = tool_meta.get(call_id);
                steps.push(tool_result_step(
                    call_id,
                    &result_text,
                    meta.map(|m| m.name.as_str()),
                    meta.map(|m| m.input_pretty.as_str()),
                ));
            }
            // System prompts and unknown roles — skip
            _ => {}
        }
    }
    compute_durations(&mut steps);
    Ok(steps)
}

#[derive(Debug, Clone)]
struct ToolMeta {
    name: String,
    input_pretty: String,
}

fn collect_tool_meta(messages: &[Message]) -> HashMap<String, ToolMeta> {
    let mut map = HashMap::new();
    for msg in messages {
        if msg.role != "assistant" {
            continue;
        }
        for tc in &msg.tool_calls {
            map.insert(
                tc.id.clone(),
                ToolMeta {
                    name: tc.function.name.clone(),
                    input_pretty: prettify_arguments(&tc.function.arguments),
                },
            );
        }
    }
    map
}

fn extract_text(content: &serde_json::Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        return arr
            .iter()
            .filter_map(|item| {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    item.get("text").and_then(|t| t.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    String::new()
}

fn prettify_arguments(args: &str) -> String {
    if args.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<serde_json::Value>(args) {
        Ok(v) => pretty_json(&v),
        Err(_) => args.to_string(),
    }
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
        let json = r#"{"messages":[
            {"role":"user","content":"hello"},
            {"role":"assistant","content":"hi there"}
        ]}"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert_eq!(steps[1].kind, StepKind::AssistantText);
    }

    #[test]
    fn parses_tool_calls_and_results() {
        let json = r#"{"messages":[
            {"role":"assistant","content":"","tool_calls":[
                {"id":"call_1","function":{"name":"search","arguments":"{\"q\":\"test\"}"}}
            ]},
            {"role":"tool","tool_call_id":"call_1","content":"found 3 results"}
        ]}"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::ToolUse);
        assert!(steps[0].detail.contains("search"));
        assert_eq!(steps[1].kind, StepKind::ToolResult);
        assert!(steps[1].label.contains("search"));
        assert!(steps[1].detail.contains("found 3 results"));
    }

    #[test]
    fn skips_system_messages() {
        let json = r#"{"messages":[
            {"role":"system","content":"you are helpful"},
            {"role":"user","content":"hi"}
        ]}"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::UserText);
    }

    #[test]
    fn handles_array_content_format() {
        let json = r#"{"messages":[
            {"role":"user","content":[{"type":"text","text":"hello world"}]}
        ]}"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].detail.contains("hello world"));
    }
}
