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
    // Line-stream via BufReader (same rationale as session.rs). Codex
    // rollouts in the wild routinely grow past 10MB of JSONL; holding
    // the whole file as a `String` just to call `.lines()` on it was
    // pure waste.
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    let file = File::open(path)
        .with_context(|| format!("opening codex session file: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries: Vec<Entry> = Vec::with_capacity(1024);
    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("reading line {} of codex session", i + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str::<Entry>(&line)
            .with_context(|| format!("parsing line {} of codex session", i + 1))?;
        entries.push(entry);
    }

    let tool_meta = collect_tool_meta(&entries);
    let mut steps = Vec::new();
    // Current Codex rollouts carry the model on `turn_context` entries and
    // usage on `event_msg` / `token_count` events rather than on the
    // messages themselves. `anchor` is the first model-output step since
    // usage was last attached; `prev_total` turns the cumulative
    // `total_token_usage` into per-response deltas (Codex repeats
    // token_count events, so `last_token_usage` alone double-counts).
    let mut current_model: Option<String> = None;
    let mut anchor: Option<usize> = None;
    let mut prev_total = CodexTotals::default();
    let mut pending = CodexTotals::default();
    for entry in &entries {
        match entry.kind.as_str() {
            "turn_context" => {
                if let Some(m) = entry.payload.get("model").and_then(|v| v.as_str()) {
                    current_model = Some(m.to_string());
                }
                continue;
            }
            "event_msg" => {
                if let Some(total) = token_count_totals(&entry.payload) {
                    pending = pending.plus(&total.minus(&prev_total));
                    prev_total = total;
                    if let Some(idx) = anchor.take()
                        && !pending.is_zero()
                    {
                        let model = steps
                            .get(idx)
                            .and_then(|s: &Step| s.model.clone())
                            .or_else(|| current_model.clone());
                        attach_usage_to_first(&mut steps, idx, model.as_deref(), &pending.usage());
                        pending = CodexTotals::default();
                    }
                }
                continue;
            }
            "response_item" => {}
            _ => continue,
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
            let is_model_output = is_assistant_message || payload_type == Some("function_call");
            steps.push(step);
            let idx = steps.len() - 1;
            if is_model_output && anchor.is_none() {
                anchor = Some(idx);
            }
            if is_assistant_message {
                // Older rollouts put usage (and sometimes the model) on the
                // message payload itself.
                let model = entry
                    .payload
                    .get("model")
                    .and_then(|v| v.as_str())
                    .or(current_model.as_deref());
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
    // OpenAI's input count includes the cached share.
    Usage::from_cache_inclusive(
        get_u64(&["input_tokens", "prompt_tokens", "promptTokens"]),
        get_u64(&[
            "cached_input_tokens",
            "cached_tokens",
            "prompt_cache_read_tokens",
        ]),
        get_u64(&["output_tokens", "completion_tokens", "completionTokens"]),
    )
}

/// Cumulative counters from a `token_count` event's `total_token_usage`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CodexTotals {
    input: u64,
    cached: u64,
    output: u64,
}

impl CodexTotals {
    fn minus(&self, prev: &Self) -> Self {
        CodexTotals {
            input: self.input.saturating_sub(prev.input),
            cached: self.cached.saturating_sub(prev.cached),
            output: self.output.saturating_sub(prev.output),
        }
    }

    fn plus(&self, other: &Self) -> Self {
        CodexTotals {
            input: self.input + other.input,
            cached: self.cached + other.cached,
            output: self.output + other.output,
        }
    }

    fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    fn usage(&self) -> Usage {
        Usage::from_cache_inclusive(Some(self.input), Some(self.cached), Some(self.output))
    }
}

/// `total_token_usage` from an `event_msg` of type `token_count`. `None`
/// for other events and for token_count events with `info: null` (Codex
/// emits those for rate-limit-only updates).
fn token_count_totals(payload: &serde_json::Value) -> Option<CodexTotals> {
    if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
        return None;
    }
    let total = payload.get("info")?.get("total_token_usage")?;
    let get = |k: &str| {
        total
            .get(k)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    Some(CodexTotals {
        input: get("input_tokens"),
        cached: get("cached_input_tokens"),
        output: get("output_tokens"),
    })
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
        // input_tokens includes the 40 cached; agx stores uncached input.
        assert_eq!(steps[0].tokens_in, Some(80));
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

    #[test]
    fn attaches_token_count_usage_and_turn_context_model() {
        // Current rollout shape: model on turn_context, cumulative usage on
        // token_count events. The second token_count repeats the first
        // (Codex does this) and must not add tokens; `info: null` events
        // are rate-limit-only.
        let tc = |input: u64, cached: u64, output: u64| {
            format!(
                r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output}}}}}}}}}"#
            )
        };
        let jsonl = [
            r#"{"type":"turn_context","payload":{"model":"gpt-5.4"}}"#.to_string(),
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"q"}]}}"#.to_string(),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":null}}"#.to_string(),
            r#"{"type":"response_item","payload":{"type":"function_call","call_id":"c1","name":"shell","arguments":"{}"}}"#.to_string(),
            tc(1000, 400, 50),
            tc(1000, 400, 50),
            r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#.to_string(),
            r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"done"}]}}"#.to_string(),
            tc(2500, 1400, 80),
        ]
        .join("\n");
        let f = write_file(&jsonl);
        let steps = load(f.path()).unwrap();
        // user, tool_use, tool_result, assistant
        assert_eq!(steps.len(), 4);
        let first = &steps[1];
        assert_eq!(first.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(
            (first.tokens_in, first.cache_read, first.tokens_out),
            (Some(600), Some(400), Some(50))
        );
        let second = &steps[3];
        assert_eq!(second.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(
            (second.tokens_in, second.cache_read, second.tokens_out),
            (Some(500), Some(1000), Some(30))
        );
        let totals = crate::timeline::compute_session_totals(&steps);
        assert_eq!(totals.tokens_in + totals.cache_read, 2500);
        assert!(totals.cost_usd.is_some());
    }
}
