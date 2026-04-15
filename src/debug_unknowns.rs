//! Format-drift diagnostics: scan a session file and report any entry types
//! or content-item types the typed parsers don't recognize.
//!
//! Used when the `--debug-unknowns` CLI flag is set. The cost is one extra
//! `serde_json::Value` parse per line — only runs with the flag.
//!
//! Output is intentionally terse and machine-greppable: one section per
//! format, sorted alphabetically by tag, with the first three line numbers
//! where each unknown was seen.

use crate::format::Format;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const SAMPLE_LIMIT: usize = 3;

#[derive(Debug, Default)]
pub struct UnknownReport {
    pub format: Option<Format>,
    pub path: PathBuf,
    pub total_lines: usize,
    pub unknown_top_level: BTreeMap<String, Vec<usize>>,
    pub unknown_payload_types: BTreeMap<String, Vec<usize>>,
    pub unknown_content_item_types: BTreeMap<String, Vec<usize>>,
}

impl UnknownReport {
    pub fn is_clean(&self) -> bool {
        self.unknown_top_level.is_empty()
            && self.unknown_payload_types.is_empty()
            && self.unknown_content_item_types.is_empty()
    }

    pub fn print<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let fmt_label = self
            .format
            .map(|f| f.to_string())
            .unwrap_or_else(|| "(unknown)".into());
        writeln!(
            w,
            "[debug-unknowns] format={} path={} lines={}",
            fmt_label,
            self.path.display(),
            self.total_lines
        )?;
        if self.is_clean() {
            writeln!(w, "  no unknown entry types or fields detected")?;
            return Ok(());
        }
        print_section(w, "unknown top-level type", &self.unknown_top_level)?;
        print_section(w, "unknown payload type", &self.unknown_payload_types)?;
        print_section(
            w,
            "unknown content-item type",
            &self.unknown_content_item_types,
        )?;
        Ok(())
    }
}

fn print_section<W: Write>(
    w: &mut W,
    label: &str,
    map: &BTreeMap<String, Vec<usize>>,
) -> io::Result<()> {
    if map.is_empty() {
        return Ok(());
    }
    for (tag, lines) in map {
        writeln!(
            w,
            "  {label}={tag:?} count={} first_lines={:?}",
            lines.len(),
            &lines[..lines.len().min(SAMPLE_LIMIT)]
        )?;
    }
    Ok(())
}

fn record(map: &mut BTreeMap<String, Vec<usize>>, tag: &str, line: usize) {
    map.entry(tag.to_string()).or_default().push(line);
}

pub fn scan(format: Format, path: &Path) -> Result<UnknownReport> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading session file: {}", path.display()))?;
    let mut report = UnknownReport {
        format: Some(format),
        path: path.to_path_buf(),
        ..UnknownReport::default()
    };
    match format {
        Format::ClaudeCode => scan_claude_code(&content, &mut report),
        Format::Codex => scan_codex(&content, &mut report),
        Format::Gemini => scan_gemini(&content, &mut report)?,
        Format::Generic => scan_generic(&content, &mut report)?,
    }
    Ok(report)
}

const CLAUDE_KNOWN_TOP: &[&str] = &["user", "assistant"];
const CLAUDE_KNOWN_USER_ITEMS: &[&str] = &["text", "tool_result"];
const CLAUDE_KNOWN_ASSISTANT_ITEMS: &[&str] = &["text", "tool_use"];

fn scan_claude_code(content: &str, report: &mut UnknownReport) {
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        report.total_lines += 1;
        let line_num = i + 1;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            record(&mut report.unknown_top_level, "<malformed-json>", line_num);
            continue;
        };
        let Some(ty) = v.get("type").and_then(|t| t.as_str()) else {
            record(&mut report.unknown_top_level, "<no-type-field>", line_num);
            continue;
        };
        if !CLAUDE_KNOWN_TOP.contains(&ty) {
            record(&mut report.unknown_top_level, ty, line_num);
            continue;
        }
        let known_items = if ty == "user" {
            CLAUDE_KNOWN_USER_ITEMS
        } else {
            CLAUDE_KNOWN_ASSISTANT_ITEMS
        };
        if let Some(items) = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            for item in items {
                if let Some(item_ty) = item.get("type").and_then(|t| t.as_str())
                    && !known_items.contains(&item_ty)
                {
                    record(&mut report.unknown_content_item_types, item_ty, line_num);
                }
            }
        }
    }
}

const CODEX_KNOWN_TOP: &[&str] = &["session_meta", "event_msg", "response_item", "turn_context"];
const CODEX_KNOWN_PAYLOAD: &[&str] = &["message", "function_call", "function_call_output"];

fn scan_codex(content: &str, report: &mut UnknownReport) {
    for (i, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        report.total_lines += 1;
        let line_num = i + 1;
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            record(&mut report.unknown_top_level, "<malformed-json>", line_num);
            continue;
        };
        let Some(ty) = v.get("type").and_then(|t| t.as_str()) else {
            record(&mut report.unknown_top_level, "<no-type-field>", line_num);
            continue;
        };
        if !CODEX_KNOWN_TOP.contains(&ty) {
            record(&mut report.unknown_top_level, ty, line_num);
            continue;
        }
        // For response_item entries, also track unrecognized payload.type values.
        // Other top-level kinds (session_meta, event_msg, turn_context) are
        // intentionally skipped — agx doesn't render them, so payload variation
        // is not interesting.
        if ty == "response_item"
            && let Some(payload_ty) = v
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str())
            && !CODEX_KNOWN_PAYLOAD.contains(&payload_ty)
        {
            record(&mut report.unknown_payload_types, payload_ty, line_num);
        }
    }
}

const GEMINI_KNOWN_MSG_TYPES: &[&str] = &["user", "gemini"];

fn scan_gemini(content: &str, report: &mut UnknownReport) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(content)
        .with_context(|| "parsing Gemini session as JSON for drift scan")?;
    let Some(messages) = v.get("messages").and_then(|m| m.as_array()) else {
        return Ok(());
    };
    for (i, msg) in messages.iter().enumerate() {
        report.total_lines += 1;
        // Use 1-indexed message position as a stand-in for line number
        let msg_idx = i + 1;
        if let Some(ty) = msg.get("type").and_then(|t| t.as_str())
            && !GEMINI_KNOWN_MSG_TYPES.contains(&ty)
        {
            record(&mut report.unknown_top_level, ty, msg_idx);
        }
    }
    Ok(())
}

const GENERIC_KNOWN_ROLES: &[&str] = &["user", "assistant", "tool", "system"];

fn scan_generic(content: &str, report: &mut UnknownReport) -> Result<()> {
    let v: serde_json::Value = serde_json::from_str(content)
        .with_context(|| "parsing generic session as JSON for drift scan")?;
    let Some(messages) = v.get("messages").and_then(|m| m.as_array()) else {
        return Ok(());
    };
    for (i, msg) in messages.iter().enumerate() {
        report.total_lines += 1;
        let msg_idx = i + 1;
        if let Some(role) = msg.get("role").and_then(|r| r.as_str())
            && !GENERIC_KNOWN_ROLES.contains(&role)
        {
            record(&mut report.unknown_top_level, role, msg_idx);
        }
    }
    Ok(())
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
    fn claude_clean_session_reports_no_unknowns() {
        let jsonl = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"hi"}}
{"type":"assistant","uuid":"a1","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}
"#;
        let f = write_file(jsonl);
        let report = scan(Format::ClaudeCode, f.path()).unwrap();
        assert_eq!(report.total_lines, 2);
        assert!(report.is_clean());
    }

    #[test]
    fn claude_unknown_top_level_type_recorded() {
        let jsonl = r#"{"type":"user","uuid":"u1","message":{"role":"user","content":"hi"}}
{"type":"summary","summary":"…"}
{"type":"summary","summary":"another"}
"#;
        let f = write_file(jsonl);
        let report = scan(Format::ClaudeCode, f.path()).unwrap();
        assert_eq!(
            report.unknown_top_level.get("summary").unwrap(),
            &vec![2, 3]
        );
    }

    #[test]
    fn claude_unknown_content_item_recorded() {
        let jsonl = r#"{"type":"assistant","uuid":"a1","message":{"role":"assistant","content":[{"type":"thinking","content":"…"}]}}
"#;
        let f = write_file(jsonl);
        let report = scan(Format::ClaudeCode, f.path()).unwrap();
        assert_eq!(
            report.unknown_content_item_types.get("thinking").unwrap(),
            &vec![1]
        );
    }

    #[test]
    fn codex_unknown_payload_type_recorded() {
        let jsonl = r#"{"type":"response_item","payload":{"type":"reasoning"}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[]}}
"#;
        let f = write_file(jsonl);
        let report = scan(Format::Codex, f.path()).unwrap();
        assert_eq!(
            report.unknown_payload_types.get("reasoning").unwrap(),
            &vec![1]
        );
    }

    #[test]
    fn codex_known_top_levels_not_reported() {
        let jsonl = r#"{"type":"session_meta","payload":{}}
{"type":"event_msg","payload":{}}
{"type":"turn_context","payload":{}}
"#;
        let f = write_file(jsonl);
        let report = scan(Format::Codex, f.path()).unwrap();
        assert!(report.is_clean());
    }

    #[test]
    fn gemini_unknown_message_type_recorded() {
        let json = r#"{"sessionId":"s1","messages":[
            {"type":"user","content":"hi"},
            {"type":"info","content":"…"},
            {"type":"system","content":"…"}
        ]}"#;
        let f = write_file(json);
        let report = scan(Format::Gemini, f.path()).unwrap();
        assert_eq!(report.unknown_top_level.get("info").unwrap(), &vec![2]);
        assert_eq!(report.unknown_top_level.get("system").unwrap(), &vec![3]);
    }

    #[test]
    fn generic_unknown_role_recorded() {
        let json = r#"{"messages":[
            {"role":"user","content":"hi"},
            {"role":"developer","content":"…"}
        ]}"#;
        let f = write_file(json);
        let report = scan(Format::Generic, f.path()).unwrap();
        assert_eq!(report.unknown_top_level.get("developer").unwrap(), &vec![2]);
    }

    #[test]
    fn report_print_clean_session() {
        let report = UnknownReport {
            format: Some(Format::ClaudeCode),
            path: PathBuf::from("/tmp/x"),
            total_lines: 5,
            ..UnknownReport::default()
        };
        let mut out = Vec::new();
        report.print(&mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("no unknown"));
        assert!(s.contains("lines=5"));
    }

    #[test]
    fn report_print_with_unknowns_shows_first_lines() {
        let mut report = UnknownReport {
            format: Some(Format::Codex),
            path: PathBuf::from("/tmp/x"),
            total_lines: 10,
            ..UnknownReport::default()
        };
        record(&mut report.unknown_payload_types, "reasoning", 3);
        record(&mut report.unknown_payload_types, "reasoning", 7);
        let mut out = Vec::new();
        report.print(&mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("reasoning"));
        assert!(s.contains("count=2"));
    }
}
