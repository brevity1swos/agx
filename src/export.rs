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

use crate::annotations::Annotations;
use crate::timeline::{SessionTotals, Step, StepKind};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct ExportJson<'a> {
    totals: &'a SessionTotals,
    steps: &'a [Step],
    /// Per-step notes, indexed by 0-based step index. Only present when
    /// the session has at least one annotation — absent (null) otherwise
    /// to keep the common-case JSON small.
    #[serde(skip_serializing_if = "Option::is_none")]
    annotations: Option<Vec<AnnotationLine>>,
}

/// JSON-friendly projection of a single note. Stable schema — downstream
/// consumers (Phase 7 library mode) can rely on these field names.
#[derive(Debug, Serialize)]
struct AnnotationLine {
    step_index: usize,
    text: String,
    created_at_ms: u64,
    updated_at_ms: u64,
}

/// Serialize a session to stable-schema JSON. Pretty-printed for readability;
/// callers that want compact output can `jq -c`. When `annotations` is
/// `Some` and non-empty, adds an `annotations` array at the top level;
/// omits the field entirely when there are no notes.
pub fn json(
    steps: &[Step],
    totals: &SessionTotals,
    annotations: Option<&Annotations>,
) -> Result<String> {
    let annotations_field = annotations.filter(|a| !a.is_empty()).map(|a| {
        a.iter()
            .map(|(idx, note)| AnnotationLine {
                step_index: idx,
                text: note.text.clone(),
                created_at_ms: note.created_at_ms,
                updated_at_ms: note.updated_at_ms,
            })
            .collect()
    });
    let payload = ExportJson {
        totals,
        steps,
        annotations: annotations_field,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

/// Render a session as a Markdown transcript. One H2 section per step, with
/// metadata listed under the header and detail inside a code fence. Totals
/// live in a short front-matter block at the top. When `annotations` is
/// provided, a blockquote with the note text is emitted below the meta
/// line for any step that has one.
pub fn markdown(
    steps: &[Step],
    totals: &SessionTotals,
    no_cost: bool,
    annotations: Option<&Annotations>,
) -> String {
    let mut out = String::with_capacity(8 * 1024);
    out.push_str("# agx session transcript\n\n");
    out.push_str(&totals_header(totals, no_cost));
    // Summary line mentioning annotation count (terse — skip entirely
    // when there are none so the common case stays clean).
    if let Some(a) = annotations
        && !a.is_empty()
    {
        out.push_str(&format!("annotations: {}\n", a.iter().count()));
    }
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
        // Blockquote the annotation text (if any). Line-by-line so
        // multi-line notes render as one quote block instead of one
        // long paragraph.
        if let Some(a) = annotations
            && let Some(note) = a.get(i)
        {
            out.push_str("> **note**: ");
            for (j, line) in note.text.lines().enumerate() {
                if j > 0 {
                    out.push_str("\n> ");
                }
                out.push_str(line);
            }
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
/// green/assistant, yellow/tool_use, magenta/tool_result. When
/// `annotations` is provided, steps with a note render the text in a
/// magenta-bordered `<div class="note">` below the meta line.
pub fn html(
    steps: &[Step],
    totals: &SessionTotals,
    no_cost: bool,
    annotations: Option<&Annotations>,
) -> String {
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
         .note { border-left: 3px solid #ba68c8; padding: 0.4rem 0.6rem; \
                 margin: 0 0 0.5rem 0; background: #1e1a22; color: #e0d0e8; \
                 font-size: 0.9rem; }\n\
         .note::before { content: \"note: \"; color: #ba68c8; font-weight: bold; }\n\
         pre { white-space: pre-wrap; word-break: break-word; margin: 0; \
               color: #d0d0d0; }\n\
         </style>\n\
         </head><body>\n",
    );
    out.push_str("<h1>agx session transcript</h1>\n<div class=\"totals\">\n");
    out.push_str(&html_escape(&totals_header(totals, no_cost)).replace('\n', "<br>\n"));
    if let Some(a) = annotations
        && !a.is_empty()
    {
        out.push_str(&format!("<br>annotations: {}\n", a.iter().count()));
    }
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
        if let Some(a) = annotations
            && let Some(note) = a.get(i)
        {
            out.push_str(&format!(
                "<div class=\"note\">{}</div>\n",
                html_escape(&note.text).replace('\n', "<br>\n")
            ));
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
        let out = json(&steps, &totals, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["totals"]["tokens_in"], 100);
        assert_eq!(v["steps"].as_array().unwrap().len(), 3);
        assert_eq!(v["steps"][0]["kind"], "user_text");
        assert_eq!(v["steps"][1]["kind"], "assistant_text");
        assert_eq!(v["steps"][2]["kind"], "tool_use");
        // Absent annotations → the field is omitted entirely from the
        // output so the common case stays small.
        assert!(
            v.get("annotations").is_none(),
            "expected no annotations field when none supplied"
        );
    }

    #[test]
    fn json_preserves_model_and_tokens_on_first_assistant_step() {
        let (steps, totals) = sample();
        let out = json(&steps, &totals, None).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["steps"][1]["model"], "claude-opus-4-6");
        assert_eq!(v["steps"][1]["tokens_in"], 100);
        assert_eq!(v["steps"][1]["tokens_out"], 50);
        assert_eq!(v["steps"][2]["tokens_in"], serde_json::Value::Null);
    }

    #[test]
    fn json_emits_annotations_array_when_present() {
        let (steps, totals) = sample();
        let mut ann = Annotations::default();
        ann.set(0, "user asked here");
        ann.set(2, "tool call under review");
        let out = json(&steps, &totals, Some(&ann)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let arr = v["annotations"].as_array().expect("annotations array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["step_index"], 0);
        assert_eq!(arr[0]["text"], "user asked here");
        assert_eq!(arr[1]["step_index"], 2);
        assert_eq!(arr[1]["text"], "tool call under review");
    }

    #[test]
    fn json_omits_annotations_when_supplied_but_empty() {
        let (steps, totals) = sample();
        let empty = Annotations::default();
        let out = json(&steps, &totals, Some(&empty)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v.get("annotations").is_none());
    }

    #[test]
    fn markdown_has_section_per_step() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, false, None);
        assert!(out.contains("# agx session transcript"));
        // One H2 per step
        assert_eq!(out.matches("\n## ").count(), 3);
        assert!(out.contains("step 1"));
        assert!(out.contains("step 3"));
    }

    #[test]
    fn markdown_includes_cost_line_when_cost_available() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, false, None);
        assert!(out.contains("**cost**: $"));
        assert!(out.contains("estimated cost:"));
    }

    #[test]
    fn markdown_no_cost_suppresses_cost_but_keeps_tokens() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, true, None);
        assert!(!out.contains("**cost**:"));
        assert!(!out.contains("estimated cost:"));
        assert!(out.contains("**tokens**:"));
    }

    #[test]
    fn markdown_surfaces_annotations_as_blockquote() {
        let (steps, totals) = sample();
        let mut ann = Annotations::default();
        ann.set(1, "revisit this");
        let out = markdown(&steps, &totals, false, Some(&ann));
        assert!(out.contains("annotations: 1"));
        assert!(out.contains("> **note**: revisit this"));
    }

    #[test]
    fn markdown_without_annotations_has_no_note_blockquote() {
        let (steps, totals) = sample();
        let out = markdown(&steps, &totals, false, None);
        assert!(!out.contains("> **note**"));
        assert!(!out.contains("annotations:"));
    }

    #[test]
    fn markdown_preserves_multiline_annotation_text() {
        let (steps, totals) = sample();
        let mut ann = Annotations::default();
        ann.set(0, "line one\nline two");
        let out = markdown(&steps, &totals, false, Some(&ann));
        // Multi-line notes should render one `> ` prefix per line.
        assert!(out.contains("> **note**: line one\n> line two"));
    }

    #[test]
    fn html_is_self_contained_no_external_assets() {
        let (steps, totals) = sample();
        let out = html(&steps, &totals, false, None);
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
        let out = html(&[s], &totals, false, None);
        assert!(
            !out.contains("<script>alert"),
            "unescaped script tag leaked: {out}"
        );
        assert!(out.contains("&lt;script&gt;"));
    }

    #[test]
    fn html_color_classes_match_step_kinds() {
        let (steps, totals) = sample();
        let out = html(&steps, &totals, false, None);
        assert!(out.contains("user-text"));
        assert!(out.contains("assistant-text"));
        assert!(out.contains("tool-use"));
    }

    #[test]
    fn html_surfaces_annotation_div_and_escapes_content() {
        let (steps, totals) = sample();
        let mut ann = Annotations::default();
        ann.set(0, "<b>revisit</b>");
        let out = html(&steps, &totals, false, Some(&ann));
        assert!(out.contains("class=\"note\""));
        assert!(out.contains("annotations: 1"));
        // Note text must be escaped like every other source of string input.
        assert!(out.contains("&lt;b&gt;revisit&lt;/b&gt;"));
        assert!(!out.contains("<b>revisit</b>"));
    }
}
