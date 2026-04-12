use anyhow::{Context, Result, anyhow};
use std::fmt;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    ClaudeCode,
    Codex,
    Gemini,
    Generic,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Format::ClaudeCode => "Claude Code",
            Format::Codex => "Codex CLI",
            Format::Gemini => "Gemini CLI",
            Format::Generic => "Generic conversation",
        };
        f.write_str(s)
    }
}

pub fn detect(path: &Path) -> Result<Format> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading session file: {}", path.display()))?;

    // Single JSON object: could be Gemini (sessionId + messages) or Generic (messages with role)
    if content.trim_start().starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&content)
    {
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
}
