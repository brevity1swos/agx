//! OpenTelemetry GenAI parser — reads OTLP-JSON trace exports produced by
//! `otel-desktop-viewer`, `otel-cli export`, and direct `application/json`
//! OTLP endpoints. Maps OTel GenAI semantic-convention attributes into the
//! shared `timeline::Step` model.
//!
//! Coverage is deliberately focused on the attribute shape most
//! instrumentation libraries emit today:
//!
//! - `gen_ai.operation.name` drives span classification
//!   (`chat` / `text_completion` / `execute_tool`)
//! - `gen_ai.prompt.{N}.role` / `.content` produce user / system steps
//! - `gen_ai.completion.{N}.role` / `.content` produce assistant steps
//! - `gen_ai.tool.name` / `.call.id` / `.call.arguments` / `.call.result`
//!   on an `execute_tool` span produce a paired tool_use + tool_result
//! - `gen_ai.request.model` + `gen_ai.usage.input_tokens` / `.output_tokens`
//!   / `.cache_read_tokens` / `.cache_creation_tokens` attach to the first
//!   step emitted from the span (same convention as other agx parsers)
//!
//! The parser intentionally ignores spans with no `gen_ai.*` attributes —
//! generic HTTP/DB spans in the same trace don't belong in an agent
//! timeline. It also ignores OpenInference attributes (`llm.*`) today;
//! they'll be added in a follow-up if fixtures land in `tests/corpus/`.

use crate::timeline::{
    Step, Usage, assistant_text_step, attach_usage_to_first, compute_durations, tool_result_step,
    tool_use_step, user_text_step,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default, rename = "resourceSpans")]
    resource_spans: Vec<ResourceSpans>,
}

#[derive(Debug, Deserialize)]
struct ResourceSpans {
    #[serde(default, rename = "scopeSpans")]
    scope_spans: Vec<ScopeSpans>,
}

#[derive(Debug, Deserialize)]
struct ScopeSpans {
    #[serde(default)]
    spans: Vec<Span>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)] // `name` is parsed for future use as a step label fallback
struct Span {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "startTimeUnixNano")]
    start_time_unix_nano: Option<String>,
    #[serde(default)]
    attributes: Vec<KeyValue>,
}

#[derive(Debug, Deserialize)]
struct KeyValue {
    key: String,
    value: serde_json::Value,
}

pub fn load(path: &Path) -> Result<Vec<Step>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading OTel JSON file: {}", path.display()))?;
    let envelope: Envelope = serde_json::from_str(&content)
        .with_context(|| format!("parsing OTel JSON: {}", path.display()))?;

    // Flatten every span into a single chronologically-ordered list. We
    // sort by startTimeUnixNano so multiple ResourceSpans / ScopeSpans in
    // one file merge cleanly.
    let mut all_spans: Vec<(&Span, u64)> = Vec::new();
    for rs in &envelope.resource_spans {
        for ss in &rs.scope_spans {
            for span in &ss.spans {
                let ts_ns = span
                    .start_time_unix_nano
                    .as_deref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                all_spans.push((span, ts_ns));
            }
        }
    }
    all_spans.sort_by_key(|(_, ts)| *ts);

    let mut steps: Vec<Step> = Vec::new();
    for (span, ts_ns) in all_spans {
        let attrs = index_attributes(&span.attributes);
        append_span(&attrs, ts_ns, &mut steps);
    }
    compute_durations(&mut steps);
    Ok(steps)
}

/// Convert OTel's AnyValue shape to a plain `serde_json::Value` keyed by
/// attribute name. Numeric values in OTLP JSON come as strings under
/// `intValue` (JSON's int64 gap); normalize them to JSON numbers where
/// possible so downstream code can just ask `as_u64()` / `as_str()`.
fn index_attributes(attrs: &[KeyValue]) -> HashMap<&str, serde_json::Value> {
    let mut out: HashMap<&str, serde_json::Value> = HashMap::new();
    for kv in attrs {
        if let Some(v) = unwrap_any_value(&kv.value) {
            out.insert(kv.key.as_str(), v);
        }
    }
    out
}

fn unwrap_any_value(v: &serde_json::Value) -> Option<serde_json::Value> {
    let obj = v.as_object()?;
    if let Some(s) = obj.get("stringValue").and_then(|x| x.as_str()) {
        return Some(serde_json::Value::String(s.to_string()));
    }
    if let Some(i) = obj.get("intValue") {
        // OTLP encodes int64 as a string to preserve precision over JSON;
        // parse back to a JSON number when it fits.
        if let Some(s) = i.as_str()
            && let Ok(n) = s.parse::<u64>()
        {
            return Some(serde_json::Value::Number(n.into()));
        }
        if let Some(n) = i.as_u64() {
            return Some(serde_json::Value::Number(n.into()));
        }
    }
    if let Some(b) = obj.get("boolValue").and_then(serde_json::Value::as_bool) {
        return Some(serde_json::Value::Bool(b));
    }
    if let Some(d) = obj.get("doubleValue").and_then(serde_json::Value::as_f64) {
        return serde_json::Number::from_f64(d).map(serde_json::Value::Number);
    }
    None
}

fn append_span(attrs: &HashMap<&str, serde_json::Value>, ts_ns: u64, steps: &mut Vec<Step>) {
    let Some(op) = get_str(attrs, "gen_ai.operation.name") else {
        // No GenAI marker — skip (generic HTTP/DB spans don't belong on an
        // agent timeline).
        return;
    };
    let first_idx = steps.len();
    let ts_ms = ts_ns / 1_000_000;

    match op {
        "execute_tool" => append_tool_span(attrs, ts_ms, steps),
        // chat / text_completion / generate_content all share the flat
        // prompt/completion attribute layout.
        _ => append_llm_span(attrs, ts_ms, steps),
    }

    // Attach model + usage to the first step emitted from this span. Same
    // convention as the other parsers — avoids double-counting in corpus
    // sums when one span produces multiple steps.
    if steps.len() > first_idx {
        let usage = Usage {
            tokens_in: get_u64(attrs, "gen_ai.usage.input_tokens")
                .or_else(|| get_u64(attrs, "gen_ai.usage.prompt_tokens")),
            tokens_out: get_u64(attrs, "gen_ai.usage.output_tokens")
                .or_else(|| get_u64(attrs, "gen_ai.usage.completion_tokens")),
            cache_read: get_u64(attrs, "gen_ai.usage.cache_read_tokens"),
            cache_create: get_u64(attrs, "gen_ai.usage.cache_creation_tokens"),
        };
        let model = get_str(attrs, "gen_ai.request.model")
            .or_else(|| get_str(attrs, "gen_ai.response.model"));
        attach_usage_to_first(steps, first_idx, model, &usage);
    }
}

fn append_llm_span(attrs: &HashMap<&str, serde_json::Value>, ts_ms: u64, steps: &mut Vec<Step>) {
    // Walk gen_ai.prompt.{N}.* in numeric order, then completion.{N}.*
    for (role, content) in indexed_messages(attrs, "gen_ai.prompt") {
        let text = content.trim();
        if text.is_empty() {
            continue;
        }
        // Skip system prompts on the timeline — agx mirrors the other
        // parsers' behavior (system messages are configuration, not turns).
        match role.as_str() {
            "user" => {
                let mut s = user_text_step(text);
                s.timestamp_ms = Some(ts_ms);
                steps.push(s);
            }
            "assistant" => {
                let mut s = assistant_text_step(text);
                s.timestamp_ms = Some(ts_ms);
                steps.push(s);
            }
            _ => {}
        }
    }
    for (role, content) in indexed_messages(attrs, "gen_ai.completion") {
        let text = content.trim();
        if text.is_empty() {
            continue;
        }
        if role == "assistant" || role == "model" {
            let mut s = assistant_text_step(text);
            s.timestamp_ms = Some(ts_ms);
            steps.push(s);
        }
    }
}

fn append_tool_span(attrs: &HashMap<&str, serde_json::Value>, ts_ms: u64, steps: &mut Vec<Step>) {
    let name = get_str(attrs, "gen_ai.tool.name").unwrap_or("(unknown)");
    let id = get_str(attrs, "gen_ai.tool.call.id").unwrap_or("");
    let input = get_str(attrs, "gen_ai.tool.call.arguments").unwrap_or("");
    let mut use_step = tool_use_step(id, name, input);
    use_step.timestamp_ms = Some(ts_ms);
    steps.push(use_step);

    // When the instrumentation flattens the tool result into the same span
    // (common for OpenLLMetry), emit a paired tool_result immediately.
    // Newer GenAI semconv emits the result on a separate span event; that
    // path becomes relevant when we add event parsing (TODO).
    if let Some(result) = get_str(attrs, "gen_ai.tool.call.result") {
        let mut res_step = tool_result_step(id, result, Some(name), Some(input));
        res_step.timestamp_ms = Some(ts_ms);
        steps.push(res_step);
    }
}

/// Scan attrs for keys matching `{prefix}.{N}.role` / `.content` and return
/// `(role, content)` pairs in numeric order. Missing / empty entries are
/// skipped. Supports arbitrary indices — most instrumentation emits 0 / 1
/// but some flatten whole conversations into a single span.
fn indexed_messages(
    attrs: &HashMap<&str, serde_json::Value>,
    prefix: &str,
) -> Vec<(String, String)> {
    // Build (index → (role, content)) then sort by index.
    let mut by_index: HashMap<u32, (Option<String>, Option<String>)> = HashMap::new();
    let role_suffix = ".role";
    let content_suffix = ".content";
    for (&key, val) in attrs {
        let Some(rest) = key.strip_prefix(prefix) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('.') else {
            continue;
        };
        let (idx_part, suffix) = match rest.find('.') {
            Some(p) => rest.split_at(p),
            None => continue,
        };
        let Ok(idx) = idx_part.parse::<u32>() else {
            continue;
        };
        let entry = by_index.entry(idx).or_default();
        if suffix == role_suffix
            && let Some(s) = val.as_str()
        {
            entry.0 = Some(s.to_string());
        } else if suffix == content_suffix
            && let Some(s) = val.as_str()
        {
            entry.1 = Some(s.to_string());
        }
    }
    let mut indexed: Vec<(u32, String, String)> = by_index
        .into_iter()
        .filter_map(|(idx, (role, content))| {
            let role = role?;
            let content = content?;
            Some((idx, role, content))
        })
        .collect();
    indexed.sort_by_key(|(idx, _, _)| *idx);
    indexed.into_iter().map(|(_, r, c)| (r, c)).collect()
}

fn get_str<'a>(attrs: &'a HashMap<&str, serde_json::Value>, key: &str) -> Option<&'a str> {
    attrs.get(key).and_then(|v| v.as_str())
}

fn get_u64(attrs: &HashMap<&str, serde_json::Value>, key: &str) -> Option<u64> {
    attrs.get(key).and_then(|v| v.as_u64())
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

    const MINIMAL_CHAT: &str = r#"{
        "resourceSpans": [{
            "scopeSpans": [{
                "spans": [{
                    "name": "chat",
                    "startTimeUnixNano": "1000000000",
                    "attributes": [
                        {"key": "gen_ai.operation.name", "value": {"stringValue": "chat"}},
                        {"key": "gen_ai.request.model", "value": {"stringValue": "gpt-5"}},
                        {"key": "gen_ai.usage.input_tokens", "value": {"intValue": "100"}},
                        {"key": "gen_ai.usage.output_tokens", "value": {"intValue": "50"}},
                        {"key": "gen_ai.prompt.0.role", "value": {"stringValue": "user"}},
                        {"key": "gen_ai.prompt.0.content", "value": {"stringValue": "hello"}},
                        {"key": "gen_ai.completion.0.role", "value": {"stringValue": "assistant"}},
                        {"key": "gen_ai.completion.0.content", "value": {"stringValue": "hi"}}
                    ]
                }]
            }]
        }]
    }"#;

    #[test]
    fn parses_prompt_and_completion_into_two_steps() {
        let f = write_file(MINIMAL_CHAT);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("hello"));
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert!(steps[1].detail.contains("hi"));
    }

    #[test]
    fn attaches_model_and_usage_to_first_step_of_span() {
        let f = write_file(MINIMAL_CHAT);
        let steps = load(f.path()).unwrap();
        // Convention: usage attaches to the FIRST step emitted from the
        // span (user prompt in this case). Corpus sums don't double-count.
        assert_eq!(steps[0].model.as_deref(), Some("gpt-5"));
        assert_eq!(steps[0].tokens_in, Some(100));
        assert_eq!(steps[0].tokens_out, Some(50));
        assert_eq!(steps[1].model, None);
        assert_eq!(steps[1].tokens_in, None);
    }

    #[test]
    fn system_role_prompts_are_dropped() {
        let json = r#"{
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [{
                        "startTimeUnixNano": "1000000000",
                        "attributes": [
                            {"key": "gen_ai.operation.name", "value": {"stringValue": "chat"}},
                            {"key": "gen_ai.prompt.0.role", "value": {"stringValue": "system"}},
                            {"key": "gen_ai.prompt.0.content", "value": {"stringValue": "you are helpful"}},
                            {"key": "gen_ai.prompt.1.role", "value": {"stringValue": "user"}},
                            {"key": "gen_ai.prompt.1.content", "value": {"stringValue": "real question"}}
                        ]
                    }]
                }]
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("real question"));
    }

    #[test]
    fn execute_tool_span_produces_paired_use_and_result() {
        let json = r#"{
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [{
                        "startTimeUnixNano": "1000000000",
                        "attributes": [
                            {"key": "gen_ai.operation.name", "value": {"stringValue": "execute_tool"}},
                            {"key": "gen_ai.tool.name", "value": {"stringValue": "list_dir"}},
                            {"key": "gen_ai.tool.call.id", "value": {"stringValue": "call_x"}},
                            {"key": "gen_ai.tool.call.arguments", "value": {"stringValue": "{\"p\":\".\"}"}},
                            {"key": "gen_ai.tool.call.result", "value": {"stringValue": "a\nb\n"}}
                        ]
                    }]
                }]
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::ToolUse);
        assert!(steps[0].label.contains("list_dir"));
        assert_eq!(steps[1].kind, StepKind::ToolResult);
        assert!(steps[1].detail.contains("a\nb"));
    }

    #[test]
    fn spans_without_genai_marker_are_ignored() {
        // A generic HTTP span in the same trace must not produce timeline
        // steps. This is what lets agx coexist with non-AI OTel spans
        // without being noisy.
        let json = r#"{
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [
                        {
                            "name": "HTTP GET",
                            "startTimeUnixNano": "1000000000",
                            "attributes": [
                                {"key": "http.method", "value": {"stringValue": "GET"}}
                            ]
                        },
                        {
                            "name": "chat",
                            "startTimeUnixNano": "2000000000",
                            "attributes": [
                                {"key": "gen_ai.operation.name", "value": {"stringValue": "chat"}},
                                {"key": "gen_ai.prompt.0.role", "value": {"stringValue": "user"}},
                                {"key": "gen_ai.prompt.0.content", "value": {"stringValue": "hi"}}
                            ]
                        }
                    ]
                }]
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].kind, StepKind::UserText);
    }

    #[test]
    fn spans_sorted_by_start_time_across_resource_and_scope_boundaries() {
        // Two ResourceSpans in arbitrary order; agx must interleave by
        // startTimeUnixNano so the timeline is chronological.
        let json = r#"{
            "resourceSpans": [
                {"scopeSpans": [{"spans": [{
                    "startTimeUnixNano": "3000000000",
                    "attributes": [
                        {"key": "gen_ai.operation.name", "value": {"stringValue": "chat"}},
                        {"key": "gen_ai.prompt.0.role", "value": {"stringValue": "user"}},
                        {"key": "gen_ai.prompt.0.content", "value": {"stringValue": "third"}}
                    ]
                }]}]},
                {"scopeSpans": [{"spans": [{
                    "startTimeUnixNano": "1000000000",
                    "attributes": [
                        {"key": "gen_ai.operation.name", "value": {"stringValue": "chat"}},
                        {"key": "gen_ai.prompt.0.role", "value": {"stringValue": "user"}},
                        {"key": "gen_ai.prompt.0.content", "value": {"stringValue": "first"}}
                    ]
                }]}]}
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert!(steps[0].detail.contains("first"));
        assert!(steps[1].detail.contains("third"));
    }

    #[test]
    fn full_fixture_parses_without_error() {
        let steps = load(Path::new("assets/sample_otel_json_traces.json")).unwrap();
        // Fixture: user, assistant text, tool_use, tool_result, assistant text
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert_eq!(steps[2].kind, StepKind::ToolUse);
        assert_eq!(steps[3].kind, StepKind::ToolResult);
        assert_eq!(steps[4].kind, StepKind::AssistantText);
        // Model/tokens on the first span's first step (user)
        assert_eq!(steps[0].model.as_deref(), Some("gpt-5"));
        assert_eq!(steps[0].tokens_in, Some(120));
    }
}
