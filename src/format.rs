use anyhow::{Context, Result, anyhow};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    ClaudeCode,
    Codex,
    Gemini,
    Generic,
    Langchain,
    OtelJson,
    OtelProto,
    VercelAi,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Format::ClaudeCode => "Claude Code",
            Format::Codex => "Codex CLI",
            Format::Gemini => "Gemini CLI",
            Format::Generic => "Generic conversation",
            Format::Langchain => "LangChain / LangSmith",
            Format::OtelJson => "OpenTelemetry GenAI (JSON)",
            Format::OtelProto => "OpenTelemetry GenAI (protobuf)",
            Format::VercelAi => "Vercel AI SDK",
        };
        f.write_str(s)
    }
}

/// Detect the format of a session file by inspecting its content shape.
/// Content-based only — no file-extension sniffing, because agx tools in
/// the wild all use vanilla `.json` / `.jsonl` extensions regardless of
/// which agent CLI produced them.
///
/// Probe order (most specific first, so ambiguous shapes land on the
/// right parser):
///
/// - Non-UTF-8 bytes → [`Format::OtelProto`] (binary OTLP)
/// - Single JSON with `resourceSpans` → [`Format::OtelJson`]
/// - Single JSON with `run_type` + `inputs`/`outputs` → [`Format::Langchain`]
/// - Single JSON with `finishReason` / `steps[].stepType` / camelCase `toolCallId` → [`Format::VercelAi`]
/// - Single JSON with `sessionId` + `messages` → [`Format::Gemini`]
/// - Single JSON with bare `messages` → [`Format::Generic`]
/// - JSONL first-line `type` in `session_meta` / `event_msg` / `response_item` / `turn_context` → [`Format::Codex`]
/// - Anything else → [`Format::ClaudeCode`]
///
/// The ordering matters. For example, a Vercel AI SDK save has
/// `messages` at the top level (which would otherwise match Generic),
/// so the Vercel-specific markers (`finishReason` / `stepType` /
/// camelCase `toolCallId`) are checked first. Same story for LangChain
/// exports that happen to include a `messages` field under `inputs`.
pub fn detect(path: &Path) -> Result<Format> {
    // Read bytes first so we can distinguish text formats from binary OTLP.
    // `read_to_string` used to be enough when all supported formats were
    // UTF-8 JSON/JSONL, but Phase 2.2 (binary OTLP) requires us to
    // gracefully route non-UTF-8 content to the protobuf parser.
    let bytes =
        std::fs::read(path).with_context(|| format!("reading session file: {}", path.display()))?;
    if bytes.is_empty() {
        return Err(anyhow!("session file is empty"));
    }

    let Ok(content) = std::str::from_utf8(&bytes) else {
        // Not UTF-8 → must be binary. Only binary format agx handles today
        // is OTLP protobuf. We route to OtelProto regardless of whether the
        // `otel-proto` feature is enabled at build time; the load dispatch
        // in main.rs produces a helpful rebuild-with-feature error when the
        // feature is off, which is a better failure mode than silently
        // mis-claiming "not json".
        return Ok(Format::OtelProto);
    };

    // Single JSON object: OTel GenAI (resourceSpans), LangSmith/LangChain
    // export (run_type at top level), Vercel AI SDK (finishReason or
    // steps[].stepType), Gemini (sessionId + messages), or Generic
    // (messages with role). Vercel is checked before Generic because its
    // outer shape (`messages[]` with role=user) would otherwise match
    // Generic — the Vercel-specific markers (`finishReason` / `stepType`)
    // disambiguate.
    if content.trim_start().starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(content)
    {
        if v.get("resourceSpans").is_some() {
            return Ok(Format::OtelJson);
        }
        if v.get("run_type").is_some() && (v.get("inputs").is_some() || v.get("outputs").is_some())
        {
            return Ok(Format::Langchain);
        }
        if is_vercel_ai(&v) {
            return Ok(Format::VercelAi);
        }
        if v.get("sessionId").is_some() && v.get("messages").is_some() {
            return Ok(Format::Gemini);
        }
        if v.get("messages").is_some() {
            return Ok(Format::Generic);
        }
    }

    // JSONL: inspect the first non-empty line's `type` field
    let first = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow!("session file is empty"))?;
    let entry: serde_json::Value = serde_json::from_str(first)
        .with_context(|| "could not parse first line of session file as JSON")?;
    let ty = entry
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("first entry has no `type` field"))?;
    match ty {
        "session_meta" | "event_msg" | "response_item" | "turn_context" => Ok(Format::Codex),
        _ => Ok(Format::ClaudeCode),
    }
}

/// Heuristics for Vercel AI SDK `generateText` / `streamText` saved
/// traces. Any of these is sufficient — they're all specific enough to the
/// SDK that false positives are rare.
fn is_vercel_ai(v: &serde_json::Value) -> bool {
    // `finishReason` at the top level is a definitive SDK marker.
    if v.get("finishReason").is_some() {
        return true;
    }
    // `steps: [{stepType: ...}]` is the multi-step result shape.
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array())
        && steps.iter().any(|s| s.get("stepType").is_some())
    {
        return true;
    }
    // CamelCase toolCall fields — distinguishes from generic OpenAI
    // (`tool_calls[0].id`, `.function.name`) which uses snake_case.
    if let Some(calls) = v.get("toolCalls").and_then(|c| c.as_array())
        && calls
            .iter()
            .any(|c| c.get("toolCallId").is_some() && c.get("toolName").is_some())
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn detects_claude_code_by_first_line_type() {
        let f = write_file(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2024-01-01T00:00:00Z","message":{"role":"user","content":"hi"}}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::ClaudeCode);
    }

    #[test]
    fn detects_codex_by_session_meta_first_line() {
        let f = write_file(
            r#"{"timestamp":"2024-01-01T00:00:00Z","type":"session_meta","payload":{"id":"s1","cwd":"/tmp","originator":"codex-tui"}}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::Codex);
    }

    #[test]
    fn detects_codex_by_response_item_first_line() {
        let f = write_file(
            r#"{"timestamp":"2024-01-01T00:00:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::Codex);
    }

    #[test]
    fn detects_otel_json_by_resource_spans_key() {
        // Minimal OTLP-JSON: any top-level object with `resourceSpans` is
        // unambiguously OTel, independent of what's inside.
        let f = write_file(r#"{"resourceSpans":[]}"#);
        assert_eq!(detect(f.path()).unwrap(), Format::OtelJson);
    }

    #[test]
    fn detects_generic_by_bare_messages_only() {
        // Pure OpenAI-compatible conversation: `messages` but none of the
        // format-specific markers that Vercel / Gemini / LangChain need.
        let f = write_file(
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::Generic);
    }

    #[test]
    fn langchain_requires_inputs_or_outputs_alongside_run_type() {
        // A single `run_type` field without inputs/outputs is probably a
        // partial or unrelated object — fall through rather than misroute
        // to LangChain. Adding a bare `messages` so something catches.
        let f = write_file(r#"{"run_type":"chain","messages":[{"role":"user","content":"hi"}]}"#);
        assert_eq!(detect(f.path()).unwrap(), Format::Generic);
    }

    #[test]
    fn detects_gemini_by_single_json_object_with_sessionid() {
        let f = write_file(
            r#"{"sessionId":"s1","projectHash":"abc","startTime":"2024-01-01T00:00:00Z","lastUpdated":"2024-01-01T00:00:01Z","messages":[],"kind":"main"}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::Gemini);
    }

    #[test]
    fn empty_file_errors() {
        let f = write_file("");
        assert!(detect(f.path()).is_err());
    }

    #[test]
    fn invalid_first_line_errors() {
        let f = write_file("not json\n");
        assert!(detect(f.path()).is_err());
    }

    #[test]
    fn detects_langchain_by_run_type_top_level_key() {
        let f = write_file(
            r#"{"id":"r1","name":"chain","run_type":"chain","start_time":"2024-01-01T00:00:00Z","inputs":{"input":"hi"},"outputs":{"output":"hello"},"child_runs":[]}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::Langchain);
    }

    #[test]
    fn detects_vercel_ai_by_finish_reason_top_level() {
        let f = write_file(
            r#"{"text":"ok","finishReason":"stop","usage":{"promptTokens":1,"completionTokens":1},"messages":[{"role":"user","content":"q"}]}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::VercelAi);
    }

    #[test]
    fn detects_vercel_ai_by_step_type() {
        let f = write_file(
            r#"{"steps":[{"stepType":"initial","text":"hi"}],"messages":[{"role":"user","content":"q"}]}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::VercelAi);
    }

    #[test]
    fn detects_vercel_ai_by_camelcase_tool_call_fields() {
        let f = write_file(
            r#"{"toolCalls":[{"toolCallId":"c1","toolName":"x","args":{}}],"messages":[{"role":"user","content":"q"}]}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::VercelAi);
    }

    #[test]
    fn generic_messages_without_vercel_markers_still_detect_as_generic() {
        let f = write_file(
            r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#,
        );
        assert_eq!(detect(f.path()).unwrap(), Format::Generic);
    }

    #[test]
    fn non_utf8_file_routes_to_otel_proto() {
        // Binary bytes (0x80+ is invalid as a UTF-8 lead byte in positions
        // where a leading byte is required). Simulates a .pb file from an
        // OTLP exporter. Detection routes to OtelProto even when the
        // feature is off — main.rs's dispatch owns the "feature disabled"
        // error rather than detection.
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&[0x0a, 0x80, 0xff, 0xfe]).unwrap();
        assert_eq!(detect(f.path()).unwrap(), Format::OtelProto);
    }
}
