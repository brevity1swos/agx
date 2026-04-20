//! Vercel AI SDK trace parser. Reads the single-JSON result objects that
//! `generateText` / `streamText` produce when stringified — the shape
//! most backends actually save to disk.
//!
//! The AI SDK has telemetry that can export to OpenTelemetry (covered by
//! `otel_json.rs`); this module handles the complement — the cases where a
//! user serializes the raw `GenerateTextResult` / `StreamTextResult`
//! directly. That shape is idiosyncratic enough (camelCase tool fields,
//! nested `steps[]`, `finishReason` at top level) that the Generic
//! OpenAI-compatible parser loses fidelity on it.
//!
//! Mapping (current scope):
//!
//! - Top-level `messages[0]` with `role: "user"` → user text step
//! - `steps[]` when present → walked in order, each step becomes a
//!   sub-sequence of (assistant text, tool_uses, tool_results)
//! - Top-level `toolCalls` / `toolResults` / `text` when `steps` is absent
//!   → treated as a single implicit step (covers the single-turn case)
//! - Tool calls: `toolCallId` + `toolName` + `args` (args is a JSON object,
//!   not a serialized string the way OpenAI does it)
//! - Tool results: `toolCallId` + `toolName` + `args` + `result` (result is
//!   string or object)
//! - Usage: `promptTokens` / `completionTokens` / `cachedInputTokens`
//!   / `cacheCreationInputTokens`, with `inputTokens` / `outputTokens`
//!   fallback for AI SDK v5+ which renamed fields
//! - Model: `response.modelId` on each step (most accurate for multi-step
//!   where different models might be used), with top-level `modelId` or
//!   `response.modelId` as fallback
//!
//! Not yet scoped:
//! - `useChat` / React UI message format where each message has a `parts`
//!   array with `type: "text"` or `type: "tool-invocation"` items — a
//!   different serialization idiom; will land when a fixture comes in
//! - `experimental_providerMetadata` — too provider-specific to surface
//! - Partial streaming traces — agx reads finished saves

use crate::timeline::{
    Step, Usage, assistant_text_step, attach_usage_to_first, compute_durations, parse_iso_ms,
    pretty_json, tool_result_step, tool_use_step, user_text_step,
};
use anyhow::{Context, Result};
use std::path::Path;

pub fn load(path: &Path) -> Result<Vec<Step>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading Vercel AI SDK session: {}", path.display()))?;
    let root: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing Vercel AI SDK session: {}", path.display()))?;

    let mut steps = Vec::new();

    // User turn — typically lives in `messages[0]` with role=user.
    if let Some(user) = extract_user_prompt(&root) {
        let trimmed = user.trim();
        if !trimmed.is_empty() {
            steps.push(user_text_step(trimmed));
        }
    }

    // Multi-step: walk `steps[]` so each step's usage / model / tool pair
    // can attach correctly. Single-step: treat the root object itself as
    // one step.
    match root.get("steps").and_then(|v| v.as_array()) {
        Some(step_array) => {
            for step in step_array {
                append_step(step, &root, &mut steps);
            }
        }
        None => append_step(&root, &root, &mut steps),
    }

    compute_durations(&mut steps);
    Ok(steps)
}

fn extract_user_prompt(root: &serde_json::Value) -> Option<String> {
    // Plain `prompt: "..."` shape (generateText with a string prompt)
    if let Some(s) = root.get("prompt").and_then(|v| v.as_str()) {
        return Some(s.to_string());
    }
    // Canonical: first user message in `messages[]`
    let messages = root.get("messages")?.as_array()?;
    for m in messages {
        if m.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        // content: string (classic OpenAI shape)
        if let Some(s) = m.get("content").and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
        // content: array of {type, text} parts (AI SDK v5 message parts)
        if let Some(parts) = m.get("content").and_then(|v| v.as_array()) {
            let text = concat_text_parts(parts);
            if !text.is_empty() {
                return Some(text);
            }
        }
        // `parts` at message level (useChat UI shape)
        if let Some(parts) = m.get("parts").and_then(|v| v.as_array()) {
            let text = concat_text_parts(parts);
            if !text.is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn concat_text_parts(parts: &[serde_json::Value]) -> String {
    parts
        .iter()
        .filter(|p| {
            // Accept `{type: "text", text: "..."}` or bare `{text: "..."}`.
            matches!(p.get("type").and_then(|v| v.as_str()), Some("text") | None)
        })
        .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("")
}

fn append_step(step: &serde_json::Value, root: &serde_json::Value, steps: &mut Vec<Step>) {
    let first_idx = steps.len();
    let ts = step
        .get("response")
        .and_then(|r| r.get("timestamp"))
        .and_then(|v| v.as_str())
        .and_then(parse_iso_ms);

    if let Some(text) = step
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let mut s = assistant_text_step(text);
        s.timestamp_ms = ts;
        steps.push(s);
    }

    if let Some(calls) = step.get("toolCalls").and_then(|v| v.as_array()) {
        for tc in calls {
            let name = tc
                .get("toolName")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            let id = tc.get("toolCallId").and_then(|v| v.as_str()).unwrap_or("");
            let args = tc.get("args").cloned().unwrap_or(serde_json::Value::Null);
            let mut s = tool_use_step(id, name, &pretty_json(&args));
            s.timestamp_ms = ts;
            steps.push(s);
        }
    }

    if let Some(results) = step.get("toolResults").and_then(|v| v.as_array()) {
        for tr in results {
            let name = tr
                .get("toolName")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            let id = tr.get("toolCallId").and_then(|v| v.as_str()).unwrap_or("");
            let args = tr.get("args").cloned().unwrap_or(serde_json::Value::Null);
            let result_val = tr.get("result");
            let result_text = match result_val {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => pretty_json(v),
                None => String::new(),
            };
            let mut s = tool_result_step(id, &result_text, Some(name), Some(&pretty_json(&args)));
            s.timestamp_ms = ts;
            steps.push(s);
        }
    }

    // Attach model + usage to the first step emitted from this step — the
    // shared anchor convention across all parsers.
    //
    // Usage: step-level ONLY. Root-level usage is an aggregate of all
    // steps in multi-step files; falling back to it would double-count.
    // Single-step files pass root as `step`, so step-level extraction
    // already covers that case.
    //
    // Model: step-level wins, root-level fallback. Model is scalar, so
    // root fallback can't double-count; it just carries the top-level
    // `response.modelId` into steps that don't repeat it.
    if steps.len() > first_idx {
        let usage = extract_usage(step).unwrap_or_default();
        let model = extract_model(step).or_else(|| extract_model(root));
        attach_usage_to_first(steps, first_idx, model.as_deref(), &usage);
    }
}

/// Extract usage from a step-or-root object. Returns `None` when the
/// `usage` field is absent or every counter is zero — AI SDK emits
/// `{promptTokens:0,completionTokens:0,totalTokens:0}` on tool-result-only
/// steps where no LLM call happened; treating that as usage would attach
/// misleading zero-token rows to the detail pane.
fn extract_usage(obj: &serde_json::Value) -> Option<Usage> {
    let u = obj.get("usage")?;
    let get = |keys: &[&str]| -> Option<u64> {
        for k in keys {
            if let Some(n) = u.get(*k).and_then(|v| v.as_u64()) {
                return Some(n);
            }
        }
        None
    };
    let usage = Usage {
        // AI SDK v5+ renamed to inputTokens/outputTokens; v4 uses
        // promptTokens/completionTokens. Accept both.
        tokens_in: get(&["promptTokens", "inputTokens"]),
        tokens_out: get(&["completionTokens", "outputTokens"]),
        cache_read: get(&["cachedInputTokens", "cacheReadInputTokens"]),
        cache_create: get(&["cacheCreationInputTokens"]),
    };
    // All-zero is sentinel for "no LLM call on this step". Treat as None
    // so downstream display / corpus sums don't pretend it's a real
    // measurement.
    let all_zero = [
        usage.tokens_in,
        usage.tokens_out,
        usage.cache_read,
        usage.cache_create,
    ]
    .iter()
    .all(|v| matches!(v, Some(0) | None));
    if all_zero {
        return None;
    }
    Some(usage)
}

fn extract_model(obj: &serde_json::Value) -> Option<String> {
    // Primary: response.modelId (set by every provider adapter in the AI SDK)
    if let Some(m) = obj
        .get("response")
        .and_then(|r| r.get("modelId"))
        .and_then(|v| v.as_str())
    {
        return Some(m.to_string());
    }
    for k in ["modelId", "model"] {
        if let Some(s) = obj.get(k).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    None
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
    fn parses_fixture_end_to_end() {
        let steps = load(Path::new("../../assets/sample_vercel_ai_session.json")).unwrap();
        // Fixture walk: user → assistant text → tool_use → tool_result → assistant text
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("List files"));
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert!(steps[1].detail.contains("list_dir tool"));
        assert_eq!(steps[2].kind, StepKind::ToolUse);
        assert!(steps[2].label.contains("list_dir"));
        assert_eq!(steps[3].kind, StepKind::ToolResult);
        assert!(steps[3].detail.contains("README.md"));
        assert_eq!(steps[4].kind, StepKind::AssistantText);
    }

    #[test]
    fn first_step_usage_attaches_to_first_assistant_text() {
        let steps = load(Path::new("../../assets/sample_vercel_ai_session.json")).unwrap();
        // steps[1] is the first chat_model's assistant text. Step-0 usage
        // (120/45) attaches there per the shared anchor rule.
        assert_eq!(steps[1].model.as_deref(), Some("gpt-5"));
        assert_eq!(steps[1].tokens_in, Some(120));
        assert_eq!(steps[1].tokens_out, Some(45));
    }

    #[test]
    fn tool_result_step_from_zero_usage_step_carries_model_but_no_tokens() {
        // Step 1 in the fixture has usage {0, 0, 0} — we treat that as
        // "no LLM on this step" and DO attach nothing (no double-counting,
        // no misleading zero-token row). The tool_result step itself comes
        // from steps[3] in the output, with no usage attached.
        let steps = load(Path::new("../../assets/sample_vercel_ai_session.json")).unwrap();
        assert_eq!(steps[3].tokens_in, None);
        assert_eq!(steps[3].tokens_out, None);
    }

    #[test]
    fn third_step_usage_attaches_to_final_assistant_text() {
        let steps = load(Path::new("../../assets/sample_vercel_ai_session.json")).unwrap();
        let last = steps.last().unwrap();
        assert_eq!(last.kind, StepKind::AssistantText);
        assert_eq!(last.tokens_in, Some(180));
        assert_eq!(last.tokens_out, Some(30));
    }

    #[test]
    fn single_step_no_steps_array_is_treated_as_one_step() {
        // No `steps` array — a plain `generateText` one-shot result.
        // toolCalls and toolResults at top level feed the single step.
        let json = r#"{
            "text": "ok",
            "finishReason": "stop",
            "usage": {"promptTokens": 10, "completionTokens": 5},
            "response": {"modelId": "gpt-5"},
            "messages": [{"role": "user", "content": "hi"}]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("hi"));
        assert_eq!(steps[1].kind, StepKind::AssistantText);
        assert_eq!(steps[1].tokens_in, Some(10));
        assert_eq!(steps[1].tokens_out, Some(5));
        assert_eq!(steps[1].model.as_deref(), Some("gpt-5"));
    }

    #[test]
    fn v5_input_output_token_names_work() {
        // v5 renamed the counters — make sure the alias path works.
        let json = r#"{
            "text": "ok",
            "usage": {"inputTokens": 42, "outputTokens": 17},
            "response": {"modelId": "gpt-5"},
            "messages": [{"role": "user", "content": "q"}]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps[1].tokens_in, Some(42));
        assert_eq!(steps[1].tokens_out, Some(17));
    }

    #[test]
    fn prompt_string_pulled_as_user_turn() {
        let json = r#"{
            "prompt": "write fibonacci",
            "text": "def fib",
            "usage": {"promptTokens": 1, "completionTokens": 1},
            "response": {"modelId": "gpt-5"}
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("write fibonacci"));
    }

    #[test]
    fn content_array_parts_for_user_message() {
        // AI SDK v5 user message with content: [{type, text}] parts.
        let json = r#"{
            "text": "ok",
            "usage": {"promptTokens": 1, "completionTokens": 1},
            "response": {"modelId": "gpt-5"},
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "text", "text": " world"}
                ]
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps[0].kind, StepKind::UserText);
        assert!(steps[0].detail.contains("hello world"));
    }

    #[test]
    fn tool_call_shape_preserves_args_as_object() {
        let json = r#"{
            "text": "ok",
            "toolCalls": [{
                "type": "tool-call",
                "toolCallId": "call_1",
                "toolName": "search",
                "args": {"q": "rust", "limit": 3}
            }],
            "usage": {"promptTokens": 1, "completionTokens": 1},
            "response": {"modelId": "gpt-5"},
            "messages": [{"role": "user", "content": "q"}]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        // user, assistant text, tool_use — 3 steps
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[2].kind, StepKind::ToolUse);
        assert!(steps[2].detail.contains("\"q\""));
        assert!(steps[2].detail.contains("\"rust\""));
        assert!(steps[2].detail.contains("\"limit\""));
    }

    #[test]
    fn usage_with_all_zeros_does_not_attach() {
        // Tool-result-only step shape: usage object present but every
        // counter is zero. Should be treated as "no LLM call" and NOT
        // attach tokens to the resulting step (would be misleading).
        let json = r#"{
            "steps": [{
                "stepType": "tool-result",
                "text": "",
                "toolResults": [{
                    "toolCallId": "call_1",
                    "toolName": "search",
                    "args": {},
                    "result": "nothing"
                }],
                "usage": {"promptTokens": 0, "completionTokens": 0, "totalTokens": 0},
                "response": {"modelId": "gpt-5"}
            }],
            "messages": [{"role": "user", "content": "q"}]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        // Only the tool_result step after the user turn.
        assert_eq!(steps[1].kind, StepKind::ToolResult);
        assert_eq!(steps[1].tokens_in, None);
        assert_eq!(steps[1].tokens_out, None);
        // Model still set via fallback — that's fine, zero-usage doesn't
        // delete model information.
        assert_eq!(steps[1].model.as_deref(), Some("gpt-5"));
    }
}
