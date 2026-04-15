use crate::timeline::{
    self, Step, Usage, assistant_text_step, attach_usage_to_first, compute_durations, parse_iso_ms,
    pretty_json, tool_result_step, tool_use_step, user_text_step,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Entry {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    payload: serde_json::Value,
}

pub fn load(path: &Path) -> Result<Vec<Step>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading codex session file: {}", path.display()))?;
    let entries: Vec<Entry> = content
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, line)| {
            serde_json::from_str::<Entry>(line)
                .with_context(|| format!("parsing line {} of codex session", i + 1))
        })
        .collect::<Result<Vec<_>>>()?;

    let tool_meta = collect_tool_meta(&entries);
    let mut steps = Vec::new();
    for entry in &entries {
        if entry.kind != "response_item" {
            continue;
        }
        let payload_type = entry.payload.get("type").and_then(|t| t.as_str());
        let ts = entry.timestamp.as_deref().and_then(parse_iso_ms);
        let mut maybe_step: Option<Step> = None;
        match payload_type {
            Some("message") => {
                let role = entry
                    .payload
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let text = extract_message_text(&entry.payload);
                if !text.trim().is_empty() {
                    maybe_step = match role {
                        "user" => Some(user_text_step(&text)),
                        "assistant" => Some(assistant_text_step(&text)),
                        _ => None,
                    };
                }
            }
            Some("function_call") => {
                let call_id = entry
                    .payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let name = entry
                    .payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                let input_pretty = prettify_codex_arguments(&entry.payload);
                maybe_step = Some(tool_use_step(call_id, name, &input_pretty));
            }
            Some("function_call_output") => {
                let call_id = entry
                    .payload
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let output = entry
                    .payload
                    .get("output")
                    .and_then(|v| v.as_str())
                    .map_or_else(|| pretty_json(&entry.payload.get("output")), String::from);
                let meta = tool_meta.get(call_id);
                maybe_step = Some(tool_result_step(
                    call_id,
                    &output,
                    meta.map(|m| m.name.as_str()),
                    meta.map(|m| m.input_pretty.as_str()),
                ));
            }
            _ => {}
        }
        if let Some(mut step) = maybe_step {
            step.timestamp_ms = ts;
            let is_assistant_message = payload_type == Some("message")
                && entry.payload.get("role").and_then(|v| v.as_str()) == Some("assistant");
            steps.push(step);
            if is_assistant_message {
                let idx = steps.len() - 1;
                let model = entry.payload.get("model").and_then(|v| v.as_str());
                let usage = extract_codex_usage(&entry.payload);
                attach_usage_to_first(&mut steps, idx, model, &usage);
            }
        }
    }
    compute_durations(&mut steps);
    Ok(steps)
}

/// Codex payload usage shape mirrors OpenAI Responses API conventions.
/// Accepts either snake_case (`input_tokens`) or legacy camelCase
/// (`promptTokens`) — Codex has used both across its versions.
fn extract_codex_usage(payload: &serde_json::Value) -> Usage {
    let Some(usage_obj) = payload.get("usage") else {
        return Usage::default();
    };
    let get_u64 = |keys: &[&str]| -> Option<u64> {
        for k in keys {
            if let Some(v) = usage_obj.get(*k).and_then(|v| v.as_u64()) {
                return Some(v);
            }
        }
        None
    };
    Usage {
        tokens_in: get_u64(&["input_tokens", "prompt_tokens", "promptTokens"]),
        tokens_out: get_u64(&["output_tokens", "completion_tokens", "completionTokens"]),
        cache_read: get_u64(&["cached_tokens", "prompt_cache_read_tokens"]),
        cache_create: None,
    }
}

#[derive(Debug, Clone)]
struct ToolMeta {
    name: String,
    input_pretty: String,
}

fn collect_tool_meta(entries: &[Entry]) -> HashMap<String, ToolMeta> {
    let mut map = HashMap::new();
    for entry in entries {
        if entry.kind != "response_item" {
            continue;
        }
        if entry.payload.get("type").and_then(|t| t.as_str()) != Some("function_call") {
            continue;
        }
        let Some(call_id) = entry.payload.get("call_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = entry
            .payload
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)")
            .to_string();
        let input_pretty = prettify_codex_arguments(&entry.payload);
        map.insert(call_id.to_string(), ToolMeta { name, input_pretty });
    }
    map
}

// Codex stores function_call arguments as a serialized JSON string inside
// the `arguments` field. Try to re-parse and pretty-print; fall back to the
// raw string if that fails.
fn prettify_codex_arguments(payload: &serde_json::Value) -> String {
    let raw = payload
        .get("arguments")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if raw.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(v) => timeline::pretty_json(&v),
        Err(_) => raw.to_string(),
    }
}

// A codex message payload has `content: [{type: input_text|output_text, text: "..."}, ...]`.
// Concatenate all text fragments into one string for the step detail.
fn extract_message_text(payload: &serde_json::Value) -> String {
    let Some(items) = payload.get("content").and_then(|c| c.as_array()) else {
        return String::new();
    };
    items
        .iter()
        .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
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
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"session_meta","payload":{"id":"s1","cwd":"/tmp"}}
{"timestamp":"2024-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hello"}]}}
{"timestamp":"2024-01-01T00:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi there"}]}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("hello"));
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert!(steps[1].detail.contains("hi there"));
    }

    #[test]
    fn pairs_function_call_with_function_call_output() {
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"function_call","call_id":"call_abc","name":"exec_command","arguments":"{\"cmd\":\"ls\"}"}}
{"timestamp":"2024-01-01T00:00:01Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call_abc","output":"file1\nfile2"}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::ToolUse);
        assert!(steps[0].detail.contains("exec_command"));
        assert!(steps[0].detail.contains("\"cmd\""));
        assert!(steps[0].detail.contains("\"ls\""));
        assert_eq!(steps[1].kind, StepKind::ToolResult);
        assert!(steps[1].label.contains("exec_command"));
        assert!(steps[1].detail.contains("Tool: exec_command"));
        assert!(steps[1].detail.contains("Input:"));
        assert!(steps[1].detail.contains("Result:"));
        assert!(steps[1].detail.contains("file1"));
    }

    #[test]
    fn skips_developer_role_messages() {
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"system policies..."}]}}
{"timestamp":"2024-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"real question"}]}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("real question"));
    }

    #[test]
    fn skips_reasoning_entries() {
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"reasoning","summary":[],"content":null}}
{"timestamp":"2024-01-01T00:00:01Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::AssistantText);
    }

    #[test]
    fn parses_usage_and_model_on_assistant_message() {
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}],"model":"gpt-5","usage":{"input_tokens":120,"output_tokens":60,"cached_tokens":40}}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].model.as_deref(), Some("gpt-5"));
        assert_eq!(steps[0].tokens_in, Some(120));
        assert_eq!(steps[0].tokens_out, Some(60));
        assert_eq!(steps[0].cache_read, Some(40));
    }

    #[test]
    fn legacy_camelcase_usage_fields_parse() {
        // Older Codex rollouts used `promptTokens` / `completionTokens` —
        // cover the fallback path in extract_codex_usage.
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"x"}],"usage":{"promptTokens":10,"completionTokens":20}}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps[0].tokens_in, Some(10));
        assert_eq!(steps[0].tokens_out, Some(20));
    }

    #[test]
    fn user_message_does_not_get_usage() {
        // Usage attaches only to assistant messages; a user message with a
        // usage field (unusual but not impossible) should be ignored.
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}],"usage":{"input_tokens":5}}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps[0].tokens_in, None);
    }

    #[test]
    fn skips_non_response_item_entries() {
        let jsonl = r#"{"timestamp":"2024-01-01T00:00:00Z","type":"session_meta","payload":{"id":"s1"}}
{"timestamp":"2024-01-01T00:00:01Z","type":"event_msg","payload":{"type":"task_started"}}
{"timestamp":"2024-01-01T00:00:02Z","type":"turn_context","payload":{}}
{"timestamp":"2024-01-01T00:00:03Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}
"#;
        let f = write_file(jsonl);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::UserText);
    }
}
