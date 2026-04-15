//! Session export — produces Markdown, HTML, or JSON representations of a
//! parsed timeline. Used by the `--export md|html|json` flag.
//!
//! All three writers take the same inputs (steps + totals + no_cost flag)
//! and return `String` — callers print to stdout or redirect to a file.
//! No I/O happens inside this module.
//!
//! Schema stability: the JSON exporter's output is the reserved programmatic
//! interface between agx and downstream consumers (Phase 7 library mode).
//! Field renames or removals count as breaking changes.

use crate::timeline::{SessionTotals, Step, StepKind};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ExportJson<'a> {
    totals: &'a SessionTotals,
    steps: &'a [Step],
}

/// Serialize a session to stable-schema JSON. Pretty-printed for readability;
/// callers that want compact output can `jq -c`.
pub fn json(steps: &[Step], totals: &SessionTotals) -> Result<String> {
    let payload = ExportJson { totals, steps };
    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Render a session as a Markdown transcript. One H2 section per step, with
/// metadata listed under the header and detail inside a code fence. Totals
/// live in a short front-matter block at the top.
pub fn markdown(steps: &[Step], totals: &SessionTotals, no_cost: bool) -> String {
    let mut out = String::with_capacity(8 * 1024);
    out.push_str("# agx session transcript\n\n");
    out.push_str(&totals_header(totals, no_cost));
    out.push('\n');
    for (i, step) in steps.iter().enumerate() {
        let (kind_label, kind_prefix) = md_kind(step.kind);
        out.push_str(&format!(
            "## {} — step {} {}\n\n",
            kind_prefix,
            i + 1,
            kind_label
        ));
        let mut meta: Vec<String> = Vec::new();
        if let Some(ms) = step.duration_ms {
            meta.push(format!(
                "**Δ**: {}",
                crate::timeline::format_duration_ms(ms)
            ));
        }
        if let Some(m) = &step.model {
            meta.push(format!("**model**: `{m}`"));
        }
        if step.tokens_in.is_some() || step.tokens_out.is_some() {
            meta.push(format!(
                "**tokens**: in={} out={} cache_read={} cache_create={}",
                step.tokens_in.unwrap_or(0),
                step.tokens_out.unwrap_or(0),
                step.cache_read.unwrap_or(0),
                step.cache_create.unwrap_or(0),
            ));
        }
        if !no_cost && let Some(c) = step.cost_usd() {
            meta.push(format!("**cost**: ${c:.4}"));
        }
        if !meta.is_empty() {
            out.push_str(&meta.join("  ·  "));
            out.push_str("\n\n");
        }
        out.push_str("```\n");
        out.push_str(&step.detail);
        if !step.detail.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("```\n\n");
    }
    out
}

/// Render a session as self-contained HTML with inline CSS, no JS, no
/// external assets. Color palette mirrors the TUI — cyan/user,
/// green/assistant, yellow/tool_use, magenta/tool_result.
pub fn html(steps: &[Step], totals: &SessionTotals, no_cost: bool) -> String {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str(
        "<!DOCTYPE html>\n<html lang=\"en\"><head>\n\
         <meta charset=\"utf-8\">\n\
         <title>agx session</title>\n\
         <style>\n\
         body { font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; \
               background: #0f0f12; color: #e0e0e0; margin: 0; padding: 2rem; }\n\
         h1 { color: #fff; margin: 0 0 0.5rem 0; font-size: 1.3rem; }\n\
         .totals { color: #b0b0b0; border-bottom: 1px solid #333; \
                   padding-bottom: 1rem; margin-bottom: 1.5rem; }\n\
         .step { margin: 1rem 0; padding: 0.75rem 1rem; border-left: 3px solid #444; \
                 background: #16161a; }\n\
         .step.user-text { border-left-color: #4dd0e1; }\n\
         .step.assistant-text { border-left-color: #81c784; }\n\
         .step.tool-use { border-left-color: #ffd54f; }\n\
         .step.tool-result { border-left-color: #ba68c8; }\n\
         .step h2 { margin: 0 0 0.5rem 0; font-size: 1rem; color: #ccc; }\n\
         .meta { color: #888; font-size: 0.85rem; margin-bottom: 0.5rem; }\n\
         .meta code { color: #b0b0b0; }\n\
         pre { white-space: pre-wrap; word-break: break-word; margin: 0; \
               color: #d0d0d0; }\n\
         </style>\n\
         </head><body>\n",
    );
    out.push_str("<h1>agx session transcript</h1>\n<div class=\"totals\">\n");
    out.push_str(&html_escape(&totals_header(totals, no_cost)).replace('\n', "<br>\n"));
    out.push_str("</div>\n");
    for (i, step) in steps.iter().enumerate() {
        let (kind_label, kind_class) = html_kind(step.kind);
        out.push_str(&format!(
            "<div class=\"step {kind_class}\"><h2>step {} — {kind_label}</h2>\n",
            i + 1
        ));
        let mut meta: Vec<String> = Vec::new();
        if let Some(ms) = step.duration_ms {
            meta.push(format!("Δ {}", crate::timeline::format_duration_ms(ms)));
        }
        if let Some(m) = &step.model {
            meta.push(format!("model <code>{}</code>", html_escape(m)));
        }
        if step.tokens_in.is_some() || step.tokens_out.is_some() {
            meta.push(format!(
                "tokens in={} out={} cache_r={} cache_c={}",
                step.tokens_in.unwrap_or(0),
                step.tokens_out.unwrap_or(0),
                step.cache_read.unwrap_or(0),
                step.cache_create.unwrap_or(0),
            ));
        }
        if !no_cost && let Some(c) = step.cost_usd() {
            meta.push(format!("cost ${c:.4}"));
        }
        if !meta.is_empty() {
            out.push_str(&format!("<div class=\"meta\">{}</div>\n", meta.join(" · ")));
        }
        out.push_str(&format!(
            "<pre>{}</pre>\n</div>\n",
            html_escape(&step.detail)
        ));
    }
    out.push_str("</body></html>\n");
    out
}

// ---------- helpers ----------

fn totals_header(totals: &SessionTotals, no_cost: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if totals.has_tokens() {
        lines.push(format!(
            "tokens — in: {}, out: {}, cache_read: {}, cache_create: {}",
            totals.tokens_in, totals.tokens_out, totals.cache_read, totals.cache_create,
        ));
    }
    if !totals.unique_models.is_empty() {
        lines.push(format!("models: {}", totals.unique_models.join(", ")));
    }
    if !no_cost {
        match totals.cost_usd {
            Some(c) => lines.push(format!("estimated cost: ${c:.4} USD")),
            None if totals.has_tokens() => {
                lines.push("estimated cost: (no pricing entry for model)".into());
            }
            None => {}
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn md_kind(kind: StepKind) -> (&'static str, &'static str) {
    // ASCII-only prefixes per the project's terminal-native / no-emoji
    // principle. See ROADMAP.md Phase 4.3 annotations subplan.
    match kind {
        StepKind::UserText => ("(user)", "[user]"),
        StepKind::AssistantText => ("(assistant)", "[asst]"),
        StepKind::ToolUse => ("(tool_use)", "[tool]"),
        StepKind::ToolResult => ("(tool_result)", "[result]"),
    }
}

fn html_kind(kind: StepKind) -> (&'static str, &'static str) {
    match kind {
        StepKind::UserText => ("user", "user-text"),
        StepKind::AssistantText => ("assistant", "assistant-text"),
        StepKind::ToolUse => ("tool_use", "tool-use"),
        StepKind::ToolResult => ("tool_result", "tool-result"),
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{
        assistant_text_step, compute_session_totals, tool_use_step, user_text_step,
    };

    fn sample() -> (Vec<Step>, SessionTotals) {
        let mut a = assistant_text_step("hi there");
        a.model = Some("claude-opus-4-6".into());
        a.tokens_in = Some(100);
        a.tokens_out = Some(50);
        let steps = vec![
            user_text_step("hello"),
            a,
            tool_use_step("t1", "Read", "{}"),
        ];
        let totals = compute_session_totals(&steps);
        (steps, totals)
    }

    #[test]
    fn json_round_trips_through_value() {
        let (steps, totals) = sample();
        let out = json(&steps, &totals).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["totals"]["tokens_in"], 100);
        assert_eq!(v["steps"].as_array().unwrap().len(), 3);
        assert_eq!(v["steps"][0]["kind"], "user_text");
        assert_eq!(v["steps"][1]["kind"], "assistant_text");
        assert_eq!(v["steps"][2]["kind"], "tool_use");
    }

    #[test]
    fn json_preserves_model_and_tokens_on_first_assistant_step() {
        let (steps, totals) = sample();
        let out = json(&steps, &totals).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["steps"][1]["model"], "claude-opus-4-6");
        assert_eq!(v["steps"][1]["tokens_in"], 100);
        assert_eq!(v["steps"][1]["tokens_out"], 50);
        assert_eq!(v["steps"][2]["tokens_in"], serde_json::Value::Null);
    }

    #[test]
    fn markdown_has_section_per_step() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, false);
        assert!(out.contains("# agx session transcript"));
        // One H2 per step
        assert_eq!(out.matches("\n## ").count(), 3);
        assert!(out.contains("step 1"));
        assert!(out.contains("step 3"));
    }

    #[test]
    fn markdown_includes_cost_line_when_cost_available() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, false);
        assert!(out.contains("**cost**: $"));
        assert!(out.contains("estimated cost:"));
    }

    #[test]
    fn markdown_no_cost_suppresses_cost_but_keeps_tokens() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, true);
        assert!(!out.contains("**cost**:"));
        assert!(!out.contains("estimated cost:"));
        assert!(out.contains("**tokens**:"));
    }

    #[test]
    fn html_is_self_contained_no_external_assets() {
        let (steps, totals) = sample();
        let out = html(&steps, &totals, false);
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<style>"));
        assert!(!out.contains("<script"), "HTML export must not include JS");
        assert!(!out.contains("<link"), "no external stylesheets");
        assert!(out.contains("</html>"));
    }

    #[test]
    fn html_escapes_step_detail() {
        let mut s = user_text_step("<script>alert(1)</script>");
        s.detail = "<script>alert(1)</script>".into();
        let totals = compute_session_totals(&[s.clone()]);
        let out = html(&[s], &totals, false);
        assert!(
            !out.contains("<script>alert"),
            "unescaped script tag leaked: {out}"
        );
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn html_color_classes_match_step_kinds() {
        let (steps, totals) = sample();
        let out = html(&steps, &totals, false);
        assert!(out.contains("user-text"));
        assert!(out.contains("assistant-text"));
        assert!(out.contains("tool-use"));
    }
}
