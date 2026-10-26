//! LangChain / LangSmith trace parser. Reads the single-JSON export shape
//! that LangSmith produces via the "export run" button and that LangChain's
//! tracer emits when writing a full run tree to a file.
//!
//! The canonical unit is a `Run` — a dict with `id`, `run_type`, `inputs`,
//! `outputs`, `start_time`, `end_time`, `parent_run_id`, and (for nested
//! trees) `child_runs`. Runs form a tree via `child_runs`; agx flattens it
//! to chronological order sorted by `start_time` before emitting steps.
//!
//! Mapping (current scope):
//!
//! - Root chain run → user step pulled from its `inputs.input` / `.question`
//!   / `.messages` (first human entry), if present
//! - `chat_model` / `llm` run → assistant text from
//!   `outputs.generations[0][0].message.data.content`; tool_use steps from
//!   `outputs.generations[0][0].message.data.tool_calls`; usage + model
//!   from `outputs.llm_output.token_usage` + `.model_name`, with a fallback
//!   to `extra.invocation_params.model`
//! - `tool` run → tool_result closing the matching pending chat_model
//!   tool_use (by `tool_call_id` when recorded, else FIFO by name), reusing
//!   that use's id. Unmatched runs emit a paired tool_use + tool_result
//!   using the run's `name`, `inputs` as the call arguments,
//!   `outputs.output` (or the raw `outputs`) as the result, and the run
//!   `id` as the call identifier so the label is stable across reruns.
//! - `chain` / `retriever` / `parser` inner runs → skipped; agx walks
//!   into their children without emitting a step for the wrapper itself.
//!
//! Scope limits (deferred to Phase 2.3 extensions):
//! - LangChain tracer v1 `.log` JSONL (`post` / `patch` events) — different
//!   wire shape, handle if users contribute fixtures
//! - `astream_events` JSONL from LangChain 0.2+ — typically not persisted
//! - Retriever / parser run types on the timeline — the info lives in
//!   neighboring chat_model runs anyway
//! - Multi-turn user-message deduplication — root-inputs extraction avoids
//!   re-emitting the prompt in subsequent chat_model runs; if a parser
//!   test shows duplicates, add a seen-content set.

use crate::timeline::{
    Step, Usage, assistant_text_step, attach_usage_to_first, compute_durations, null_as_default,
    parse_iso_ms, pretty_json, tool_result_step, tool_use_step, user_text_step,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

// String / Vec fields use `null_as_default` because LangSmith exports
// write explicit `null` for empty values rather than omitting the key.
#[derive(Debug, Deserialize)]
struct Run {
    #[serde(default, deserialize_with = "null_as_default")]
    id: String,
    #[serde(default, deserialize_with = "null_as_default")]
    name: String,
    #[serde(default, rename = "run_type", deserialize_with = "null_as_default")]
    run_type: String,
    #[serde(default)]
    start_time: Option<String>,
    #[serde(default)]
    inputs: serde_json::Value,
    #[serde(default)]
    outputs: serde_json::Value,
    #[serde(default)]
    extra: serde_json::Value,
    #[serde(default, deserialize_with = "null_as_default")]
    child_runs: Vec<Run>,
}

pub fn load(path: &Path) -> Result<Vec<Step>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading LangChain export: {}", path.display()))?;
    let root: Run = serde_json::from_str(&content)
        .with_context(|| format!("parsing LangChain export as Run tree: {}", path.display()))?;

    let mut steps = Vec::new();

    // Emit the root user turn exactly once. Inner chat_model runs carry the
    // same user content in their inputs (alongside prior assistant / tool
    // messages) — emitting from there would double-count.
    if let Some(user_text) = extract_user_input(&root)
        && !user_text.trim().is_empty()
    {
        let mut s = user_text_step(&user_text);
        s.timestamp_ms = root.start_time.as_deref().and_then(parse_iso_ms);
        steps.push(s);
    }

    // Walk the full tree, collect every run, sort by start_time, then emit
    // assistant / tool steps. Sorting is preferred over a depth-first walk
    // because LangSmith's child_runs order isn't always chronological when
    // a parent awaits multiple children.
    let mut flat: Vec<&Run> = Vec::new();
    collect_runs(&root, &mut flat);
    flat.sort_by_key(|r| r.start_time.as_deref().and_then(parse_iso_ms).unwrap_or(0));
    let mut pending: Vec<PendingUse> = Vec::new();
    for run in flat {
        let ts = run.start_time.as_deref().and_then(parse_iso_ms);
        match run.run_type.as_str() {
            "chat_model" | "llm" => append_chat_model_steps(run, ts, &mut steps, &mut pending),
            "tool" => append_tool_steps(run, ts, &mut steps, &mut pending),
            _ => {} // chain / retriever / parser / other — skip
        }
    }
    compute_durations(&mut steps);
    Ok(steps)
}

fn collect_runs<'a>(run: &'a Run, out: &mut Vec<&'a Run>) {
    out.push(run);
    for child in &run.child_runs {
        collect_runs(child, out);
    }
}

/// Extract the human-visible prompt from a root chain run. LangChain
/// projects use wildly different field names — try the common ones in
/// order, then fall back to scanning `inputs.messages` for a human message.
fn extract_user_input(run: &Run) -> Option<String> {
    for key in ["input", "question", "query", "prompt"] {
        if let Some(s) = run.inputs.get(key).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    // `inputs.messages` shape: `[[{type: "human", data: {content: "..."}}, ...]]`
    // (outer list = batches, inner list = messages). Pull first human
    // message from the first batch.
    let batches = run.inputs.get("messages")?.as_array()?;
    for batch in batches {
        let msgs = batch.as_array()?;
        for m in msgs {
            if m.get("type").and_then(|v| v.as_str()) == Some("human")
                && let Some(content) = m
                    .get("data")
                    .and_then(|d| d.get("content"))
                    .and_then(|v| v.as_str())
            {
                return Some(content.to_string());
            }
        }
    }
    None
}

/// A `tool_use` emitted from a chat_model's `tool_calls[]` that no `tool`
/// run has closed yet. Kept in emission order so name-only matching is
/// FIFO across parallel calls.
struct PendingUse {
    name: String,
    tool_call_id: String,
}

fn append_chat_model_steps(
    run: &Run,
    ts: Option<u64>,
    steps: &mut Vec<Step>,
    pending: &mut Vec<PendingUse>,
) {
    let first_idx = steps.len();
    // `generation` rather than `gen` — `gen` is a reserved keyword in
    // Rust 2024.
    let Some(generation) = run
        .outputs
        .get("generations")
        .and_then(|v| v.as_array())
        .and_then(|outer| outer.first())
        .and_then(|inner| inner.as_array())
        .and_then(|arr| arr.first())
    else {
        return;
    };
    let msg_data = generation.get("message").and_then(|m| m.get("data"));

    if let Some(text) = msg_data
        .and_then(|d| d.get("content"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        let mut s = assistant_text_step(text);
        s.timestamp_ms = ts;
        steps.push(s);
    }

    // Tool calls attached to the assistant message (modern tool-calling
    // shape — older LangChain versions put them under `additional_kwargs`).
    if let Some(tool_calls) = msg_data
        .and_then(|d| d.get("tool_calls"))
        .and_then(|v| v.as_array())
    {
        for tc in tool_calls {
            let name = tc
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("(unknown)");
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let args = tc.get("args").cloned().unwrap_or(serde_json::Value::Null);
            let mut s = tool_use_step(id, name, &pretty_json(&args));
            s.timestamp_ms = ts;
            steps.push(s);
            pending.push(PendingUse {
                name: name.to_string(),
                tool_call_id: id.to_string(),
            });
        }
    }

    // Attach model + usage to the first step emitted from this run, matching
    // every other parser's anchor convention.
    if steps.len() > first_idx {
        let usage = extract_usage(&run.outputs);
        let model = extract_model(&run.outputs, &run.extra);
        attach_usage_to_first(steps, first_idx, model.as_deref(), &usage);
    }
}

fn append_tool_steps(
    run: &Run,
    ts: Option<u64>,
    steps: &mut Vec<Step>,
    pending: &mut Vec<PendingUse>,
) {
    // Name comes from the run itself, not `inputs` (LangChain convention).
    let name = if run.name.is_empty() {
        "(unknown)"
    } else {
        run.name.as_str()
    };
    let input_pretty = extract_tool_input(&run.inputs);
    let result = extract_tool_output(&run.outputs);

    // LangChain convention: a `tool` run is the *execution* of a tool_call
    // a prior chat_model already emitted as a `tool_use` (from its
    // `outputs.generations[0][0].message.data.tool_calls`). If an unclosed
    // tool_use matches — by explicit tool_call_id when the run carries one,
    // else the oldest pending use with the same name — emit only the
    // tool_result under that use's id so the pair closes. If nothing
    // matches (some agent architectures skip the tool_use), emit both so
    // the call is still visible in the timeline.
    let explicit_id = extract_tool_call_id(run);
    let matched = explicit_id
        .and_then(|id| pending.iter().position(|p| p.tool_call_id == id))
        .or_else(|| pending.iter().position(|p| p.name == name))
        .map(|i| pending.remove(i));
    let call_id = match &matched {
        Some(p) if !p.tool_call_id.is_empty() => p.tool_call_id.as_str(),
        _ => run.id.as_str(),
    };
    if matched.is_none() {
        let mut use_step = tool_use_step(call_id, name, &input_pretty);
        use_step.timestamp_ms = ts;
        steps.push(use_step);
    }

    let mut res_step = tool_result_step(call_id, &result, Some(name), Some(&input_pretty));
    res_step.timestamp_ms = ts;
    steps.push(res_step);
}

/// The LLM-assigned tool_call id a `tool` run executes, when the tracer
/// recorded it. LangChain versions disagree on where it lives.
fn extract_tool_call_id(run: &Run) -> Option<&str> {
    [
        run.inputs.get("tool_call_id"),
        run.extra.get("tool_call_id"),
        run.extra
            .get("metadata")
            .and_then(|m| m.get("tool_call_id")),
    ]
    .into_iter()
    .flatten()
    .find_map(|v| v.as_str().filter(|s| !s.is_empty()))
}

fn extract_tool_input(inputs: &serde_json::Value) -> String {
    // LangChain wraps tool inputs several ways: `{input: {...}}`, `{args: {...}}`,
    // or the arg map directly. Prefer the nested form, fall back to the raw
    // object.
    if let Some(inner) = inputs.get("input") {
        return pretty_json(inner);
    }
    if let Some(inner) = inputs.get("args") {
        return pretty_json(inner);
    }
    pretty_json(inputs)
}

fn extract_tool_output(outputs: &serde_json::Value) -> String {
    // Same story on the output side: `{output: "..."}`, or bare string, or
    // bare object.
    if let Some(s) = outputs.get("output").and_then(|v| v.as_str()) {
        return s.to_string();
    }
    if let Some(v) = outputs.get("output") {
        return pretty_json(v);
    }
    if let Some(s) = outputs.as_str() {
        return s.to_string();
    }
    if outputs.is_null() {
        return String::new();
    }
    pretty_json(outputs)
}

fn extract_usage(outputs: &serde_json::Value) -> Usage {
    let usage = outputs.get("llm_output").and_then(|v| v.get("token_usage"));
    let get = |obj: Option<&serde_json::Value>, keys: &[&str]| -> Option<u64> {
        let obj = obj?;
        for k in keys {
            if let Some(n) = obj.get(*k).and_then(|v| v.as_u64()) {
                return Some(n);
            }
        }
        None
    };
    Usage {
        tokens_in: get(usage, &["prompt_tokens", "input_tokens"]),
        tokens_out: get(usage, &["completion_tokens", "output_tokens"]),
        cache_read: get(usage, &["prompt_cache_read", "cache_read_tokens"]),
        cache_create: None,
    }
}

fn extract_model(outputs: &serde_json::Value, extra: &serde_json::Value) -> Option<String> {
    // Primary: outputs.llm_output.model_name (set by ChatOpenAI,
    // ChatAnthropic, etc.)
    if let Some(m) = outputs
        .get("llm_output")
        .and_then(|v| v.get("model_name"))
        .and_then(|v| v.as_str())
    {
        return Some(m.to_string());
    }
    // Fallback: extra.invocation_params.model_name / .model
    let params = extra.get("invocation_params")?;
    for k in ["model_name", "model"] {
        if let Some(s) = params.get(k).and_then(|v| v.as_str()) {
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
        let steps = load(Path::new("../../assets/sample_langchain_export.json")).unwrap();
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
    fn attaches_model_and_usage_to_first_chat_model_step() {
        let steps = load(Path::new("../../assets/sample_langchain_export.json")).unwrap();
        // First assistant step = steps[1] in fixture order. Model + tokens
        // of the first chat_model run attach here (per the shared anchor
        // convention), not to the subsequent tool_use step from the same run.
        assert_eq!(steps[1].model.as_deref(), Some("gpt-5"));
        assert_eq!(steps[1].tokens_in, Some(120));
        assert_eq!(steps[1].tokens_out, Some(45));
        assert_eq!(steps[2].tokens_in, None);
    }

    #[test]
    fn second_chat_model_run_carries_its_own_usage() {
        let steps = load(Path::new("../../assets/sample_langchain_export.json")).unwrap();
        // Second assistant step is the last one emitted; second chat_model's
        // usage attaches there (anchor convention).
        let last = steps.last().unwrap();
        assert_eq!(last.kind, StepKind::AssistantText);
        assert_eq!(last.tokens_in, Some(180));
        assert_eq!(last.tokens_out, Some(30));
    }

    #[test]
    fn root_user_input_pulled_from_inputs_input_field() {
        let json = r#"{
            "id": "r1",
            "name": "chain",
            "run_type": "chain",
            "start_time": "2024-01-01T00:00:00Z",
            "inputs": {"input": "hello there"},
            "outputs": {},
            "child_runs": []
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].detail.contains("hello there"));
    }

    #[test]
    fn root_user_input_falls_back_to_messages_array() {
        // No `inputs.input` — pull from the human message in
        // `inputs.messages[0]`.
        let json = r#"{
            "id": "r1",
            "run_type": "chain",
            "start_time": "2024-01-01T00:00:00Z",
            "inputs": {
                "messages": [[
                    {"type": "system", "data": {"content": "be brief"}},
                    {"type": "human", "data": {"content": "hi"}}
                ]]
            },
            "outputs": {},
            "child_runs": []
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].detail.contains("hi"));
    }

    #[test]
    fn chain_runs_without_chat_or_tool_children_produce_no_steps() {
        let json = r#"{
            "id": "r1",
            "run_type": "chain",
            "inputs": {},
            "outputs": {},
            "child_runs": [
                {"id": "r2", "run_type": "parser", "inputs": {}, "outputs": {}, "child_runs": []},
                {"id": "r3", "run_type": "retriever", "inputs": {}, "outputs": {}, "child_runs": []}
            ]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert!(steps.is_empty());
    }

    #[test]
    fn tool_run_emits_paired_use_and_result() {
        let json = r#"{
            "id": "r1",
            "run_type": "chain",
            "inputs": {"input": "do a thing"},
            "child_runs": [{
                "id": "tool_abc",
                "name": "search",
                "run_type": "tool",
                "start_time": "2024-01-01T00:00:01Z",
                "inputs": {"input": {"q": "test"}},
                "outputs": {"output": "found 3 results"}
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[1].kind, StepKind::ToolUse);
        assert!(steps[1].label.contains("search"));
        assert!(steps[1].detail.contains("test"));
        assert_eq!(steps[2].kind, StepKind::ToolResult);
        assert!(steps[2].detail.contains("found 3 results"));
    }

    fn parallel_calls_json(tool_runs: &str) -> String {
        format!(
            r#"{{
            "id": "r1",
            "run_type": "chain",
            "inputs": {{"input": "do two things"}},
            "child_runs": [{{
                "id": "llm1",
                "run_type": "chat_model",
                "start_time": "2024-01-01T00:00:01Z",
                "outputs": {{"generations": [[{{"message": {{"data": {{
                    "content": "",
                    "tool_calls": [
                        {{"id": "call_A", "name": "search", "args": {{"q": "a"}}}},
                        {{"id": "call_B", "name": "fetch", "args": {{"u": "b"}}}}
                    ]
                }}}}}}]]}}
            }}, {tool_runs}]
        }}"#
        )
    }

    #[test]
    fn parallel_tool_calls_pair_with_pending_uses_by_name() {
        let json = parallel_calls_json(
            r#"{"id": "run_A", "name": "search", "run_type": "tool",
                "start_time": "2024-01-01T00:00:02Z",
                "inputs": {"input": {"q": "a"}}, "outputs": {"output": "ra"}},
               {"id": "run_B", "name": "fetch", "run_type": "tool",
                "start_time": "2024-01-01T00:00:03Z",
                "inputs": {"input": {"u": "b"}}, "outputs": {"output": "rb"}}"#,
        );
        let f = write_file(&json);
        let steps = load(f.path()).unwrap();
        let kinds: Vec<_> = steps.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                StepKind::UserText,
                StepKind::ToolUse,
                StepKind::ToolUse,
                StepKind::ToolResult,
                StepKind::ToolResult,
            ]
        );
        assert_eq!(steps[3].tool_name.as_deref(), Some("search"));
        assert_eq!(steps[3].tool_call_id.as_deref(), Some("call_A"));
        assert_eq!(steps[4].tool_name.as_deref(), Some("fetch"));
        assert_eq!(steps[4].tool_call_id.as_deref(), Some("call_B"));
    }

    #[test]
    fn tool_run_prefers_tool_call_id_over_name_when_present() {
        // Two pending uses with the same name; the tool runs arrive in
        // reverse order but carry explicit tool_call_ids.
        let json = r#"{
            "id": "r1",
            "run_type": "chain",
            "inputs": {"input": "q"},
            "child_runs": [{
                "id": "llm1",
                "run_type": "chat_model",
                "start_time": "2024-01-01T00:00:01Z",
                "outputs": {"generations": [[{"message": {"data": {
                    "content": "",
                    "tool_calls": [
                        {"id": "call_1", "name": "search", "args": {}},
                        {"id": "call_2", "name": "search", "args": {}}
                    ]
                }}}]]}
            }, {
                "id": "run_2", "name": "search", "run_type": "tool",
                "start_time": "2024-01-01T00:00:02Z",
                "inputs": {"input": "x", "tool_call_id": "call_2"},
                "outputs": {"output": "second"}
            }, {
                "id": "run_1", "name": "search", "run_type": "tool",
                "start_time": "2024-01-01T00:00:03Z",
                "inputs": {"input": "y"},
                "extra": {"metadata": {"tool_call_id": "call_1"}},
                "outputs": {"output": "first"}
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[3].tool_call_id.as_deref(), Some("call_2"));
        assert!(steps[3].detail.contains("second"));
        assert_eq!(steps[4].tool_call_id.as_deref(), Some("call_1"));
        assert!(steps[4].detail.contains("first"));
    }

    #[test]
    fn tool_run_without_pending_use_still_emits_both() {
        // A pending use for `search` must not absorb an unrelated tool run.
        let json = parallel_calls_json(
            r#"{"id": "run_X", "name": "other", "run_type": "tool",
                "start_time": "2024-01-01T00:00:02Z",
                "inputs": {"input": {}}, "outputs": {"output": "rx"}}"#,
        );
        let f = write_file(&json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[3].kind, StepKind::ToolUse);
        assert_eq!(steps[3].tool_call_id.as_deref(), Some("run_X"));
        assert_eq!(steps[4].kind, StepKind::ToolResult);
        assert_eq!(steps[4].tool_call_id.as_deref(), Some("run_X"));
    }

    #[test]
    fn model_pulled_from_invocation_params_when_llm_output_missing() {
        let json = r#"{
            "id": "r1",
            "run_type": "chain",
            "inputs": {"input": "q"},
            "child_runs": [{
                "id": "r2",
                "name": "ChatOpenAI",
                "run_type": "chat_model",
                "start_time": "2024-01-01T00:00:01Z",
                "inputs": {},
                "outputs": {
                    "generations": [[{"message": {"data": {"content": "a"}}}]]
                },
                "extra": {"invocation_params": {"model_name": "gpt-5-mini"}}
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].model.as_deref(), Some("gpt-5-mini"));
    }

    #[test]
    fn input_token_fallback_key_works() {
        // Some providers use `input_tokens` / `output_tokens` instead of the
        // OpenAI naming — Anthropic-backed LangChain clients do this.
        let json = r#"{
            "id": "r1",
            "run_type": "chain",
            "inputs": {"input": "q"},
            "child_runs": [{
                "id": "r2",
                "run_type": "chat_model",
                "start_time": "2024-01-01T00:00:01Z",
                "outputs": {
                    "generations": [[{"message": {"data": {"content": "a"}}}]],
                    "llm_output": {
                        "token_usage": {"input_tokens": 10, "output_tokens": 20},
                        "model_name": "claude-sonnet-4-6"
                    }
                }
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps[1].tokens_in, Some(10));
        assert_eq!(steps[1].tokens_out, Some(20));
    }

    #[test]
    fn null_fields_are_tolerated() {
        // LangSmith exports write `null` for empty child_runs / names
        // rather than omitting the key.
        let json = r#"{
            "id": null,
            "name": null,
            "run_type": "chain",
            "inputs": {"input": "q"},
            "child_runs": [{
                "id": "t1",
                "name": "search",
                "run_type": "tool",
                "start_time": "2024-01-01T00:00:01Z",
                "inputs": {"input": "x"},
                "outputs": {"output": "y"},
                "child_runs": null
            }]
        }"#;
        let f = write_file(json);
        let steps = load(f.path()).unwrap();
        assert_eq!(steps.len(), 3);
    }
}
