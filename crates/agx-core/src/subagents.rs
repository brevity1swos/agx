//! Claude Code subagent discovery. A session that delegates to subagents
//! (the `Agent` / `Task` tool) keeps each subagent's transcript in its own
//! file next to the parent, not inline:
//!
//! ```text
//! <project>/<session-id>.jsonl                          parent transcript
//! <project>/<session-id>/subagents/agent-<id>.jsonl     subagent transcript
//! <project>/<session-id>/subagents/agent-<id>.meta.json sidecar metadata
//! ```
//!
//! The subagent transcript's first entry has `parentUuid: null`, so nothing
//! inside it points back at the parent. The links are:
//!
//! - `meta.json` `toolUseId` = the id of the `tool_use` that spawned the
//!   subagent. That call lives in the parent transcript, or in another
//!   subagent's transcript for nested delegation (`spawnDepth` > 1).
//! - The final report comes back to the spawner as an `attachment` entry
//!   with `origin.kind == "peer"`, `origin.from == <agent id>`, and
//!   `handback: true`; the report text is `handbackReportText`.
//!
//! This layout is undocumented Claude Code internals (observed on 2.1.x).
//! Every field is optional here so drift degrades to missing links rather
//! than a failed load.

use crate::claims::{self, ClaimCheck};
use crate::session;
use crate::timeline::{self, Step};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// The `agent-<id>.meta.json` sidecar. Only the fields agx reads.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    agent_type: Option<String>,
    description: Option<String>,
    tool_use_id: Option<String>,
    spawn_depth: Option<u32>,
    worktree_branch: Option<String>,
    finished: Option<bool>,
}

/// Which transcript holds the `tool_use` that spawned a subagent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Spawner {
    /// The parent session.
    Session,
    /// Another subagent (nested delegation).
    Subagent { agent_id: String },
    /// `toolUseId` missing, or no transcript contains a matching call.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Subagent {
    pub agent_id: String,
    pub path: PathBuf,
    pub agent_type: Option<String>,
    pub description: Option<String>,
    pub tool_use_id: Option<String>,
    pub spawn_depth: Option<u32>,
    pub worktree_branch: Option<String>,
    pub finished: Option<bool>,
    pub spawned_by: Spawner,
    /// 0-based index of the spawning `tool_use` step in the spawner's
    /// timeline (the same indexing `--jump-to` uses).
    pub spawn_step: Option<usize>,
    /// Hand-back reports delivered to the spawner, in file order. More than
    /// one when the subagent was continued after its first report.
    pub reports: Vec<String>,
    /// The final report's "tests pass" claim checked against this
    /// subagent's own test runs. `None` when the report makes no such claim.
    pub claim_check: Option<ClaimCheck>,
    pub step_count: usize,
    /// Set when the transcript failed to parse; `steps` is then empty.
    pub load_error: Option<String>,
    #[serde(skip)]
    pub steps: Vec<Step>,
}

#[derive(Debug)]
pub struct SessionTree {
    pub session: PathBuf,
    pub steps: Vec<Step>,
    /// Ordered by spawner, then spawn step; unlinked subagents last.
    pub subagents: Vec<Subagent>,
}

/// `<dir>/<stem>.jsonl` → `<dir>/<stem>/subagents`.
pub fn subagents_dir(session: &Path) -> Option<PathBuf> {
    let stem = session.file_stem()?;
    Some(session.with_file_name(stem).join("subagents"))
}

/// Load a Claude Code session and every subagent transcript stored beside
/// it, linking each subagent to the step that spawned it and to the
/// reports it handed back. A session without subagents yields an empty
/// `subagents` list. A subagent file that fails to parse is kept with
/// `load_error` set instead of failing the whole tree.
pub fn load_tree(session_path: &Path) -> Result<SessionTree> {
    let steps = timeline::build(&session::load(session_path)?);
    let mut subagents = match subagents_dir(session_path) {
        Some(dir) if dir.is_dir() => discover(&dir)?,
        _ => Vec::new(),
    };

    // tool_call_id → (spawner, step index), across every transcript.
    let mut calls: HashMap<&str, (Spawner, usize)> = HashMap::new();
    index_tool_uses(&steps, &Spawner::Session, &mut calls);
    for sub in &subagents {
        let spawner = Spawner::Subagent {
            agent_id: sub.agent_id.clone(),
        };
        index_tool_uses(&sub.steps, &spawner, &mut calls);
    }
    let links: Vec<(Spawner, Option<usize>)> = subagents
        .iter()
        .map(|sub| {
            sub.tool_use_id
                .as_deref()
                .and_then(|id| calls.get(id))
                .map_or((Spawner::Unknown, None), |(s, i)| (s.clone(), Some(*i)))
        })
        .collect();

    // Reports land in the spawner's transcript, so scan all of them.
    let mut reports = scan_handbacks(session_path)?;
    for sub in &subagents {
        for (from, text) in scan_handbacks(&sub.path).unwrap_or_default() {
            reports.entry(from).or_default().extend(text);
        }
    }

    for (sub, (spawner, step)) in subagents.iter_mut().zip(links) {
        sub.spawned_by = spawner;
        sub.spawn_step = step;
        sub.reports = reports.remove(&sub.agent_id).unwrap_or_default();
        sub.claim_check = sub
            .reports
            .last()
            .and_then(|report| claims::check(report, &sub.steps));
    }
    subagents.sort_by(|a, b| {
        let key = |s: &Subagent| (s.spawned_by == Spawner::Unknown, s.spawn_step);
        key(a)
            .cmp(&key(b))
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });

    Ok(SessionTree {
        session: session_path.to_path_buf(),
        steps,
        subagents,
    })
}

fn discover(dir: &Path) -> Result<Vec<Subagent>> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(agent_id) = name
            .strip_prefix("agent-")
            .and_then(|n| n.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let meta = read_meta(&path.with_extension("meta.json"));
        let (steps, load_error) = match session::load(&path) {
            Ok(entries) => (timeline::build(&entries), None),
            Err(e) => (Vec::new(), Some(format!("{e:#}"))),
        };
        out.push(Subagent {
            agent_id: agent_id.to_string(),
            agent_type: meta.agent_type,
            description: meta.description,
            tool_use_id: meta.tool_use_id,
            spawn_depth: meta.spawn_depth,
            worktree_branch: meta.worktree_branch,
            finished: meta.finished,
            spawned_by: Spawner::Unknown,
            spawn_step: None,
            reports: Vec::new(),
            claim_check: None,
            step_count: steps.len(),
            load_error,
            steps,
            path,
        });
    }
    Ok(out)
}

/// A missing or malformed sidecar yields an empty `Meta`: the transcript is
/// still worth showing, just without its links.
fn read_meta(path: &Path) -> Meta {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn index_tool_uses<'a>(
    steps: &'a [Step],
    spawner: &Spawner,
    calls: &mut HashMap<&'a str, (Spawner, usize)>,
) {
    for (i, step) in steps.iter().enumerate() {
        if step.kind == timeline::StepKind::ToolUse
            && let Some(id) = step.tool_call_id.as_deref()
        {
            calls.entry(id).or_insert_with(|| (spawner.clone(), i));
        }
    }
}

/// Collect hand-back reports from one transcript, keyed by sender agent id.
/// Lines without the `"handback"` marker are skipped before JSON parsing,
/// so a large session costs one substring scan per line.
fn scan_handbacks(path: &Path) -> Result<HashMap<String, Vec<String>>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        if !buf.windows(10).any(|w| w == b"\"handback\"") {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&buf) else {
            continue;
        };
        // A report reaching an idle spawner is a `user` entry with a
        // top-level `origin`; one reaching a busy spawner is queued and
        // injected as an `attachment`. The `queue-operation` lines that
        // carry the same text in between are skipped so nothing counts twice.
        let origin = match value["type"].as_str() {
            Some("user") => &value["origin"],
            Some("attachment") => &value["attachment"]["origin"],
            _ => continue,
        };
        if origin["kind"] != "peer" || origin["handback"] != true {
            continue;
        }
        let Some(from) = origin["from"].as_str() else {
            continue;
        };
        let text = match origin["handbackReportText"].as_str() {
            Some(text) => text.to_string(),
            None => report_from_body(origin["body"].as_str().unwrap_or_default()),
        };
        out.entry(from.to_string()).or_default().push(text);
    }
    Ok(out)
}

/// Older Claude Code versions (seen on 2.1.284) carry only `body`: a
/// harness preamble, then the report with every line indented two spaces.
/// Returns the body unchanged when the marker is missing.
fn report_from_body(body: &str) -> String {
    const MARKER: &str = "The report follows:\n";
    let Some(at) = body.find(MARKER) else {
        return body.to_string();
    };
    body[at + MARKER.len()..]
        .lines()
        .map(|line| line.strip_prefix("  ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn user(uuid: &str, text: &str) -> String {
        format!(
            r#"{{"type":"user","uuid":"{uuid}","parentUuid":null,"message":{{"role":"user","content":"{text}"}}}}"#
        )
    }

    fn agent_call(uuid: &str, tool_use_id: &str) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","parentUuid":null,"message":{{"role":"assistant","content":[{{"type":"tool_use","id":"{tool_use_id}","name":"Agent","input":{{"description":"d","prompt":"p"}}}}]}}}}"#
        )
    }

    fn handback(from: &str, report: &str) -> String {
        format!(
            r#"{{"type":"attachment","attachment":{{"type":"queued_command","origin":{{"kind":"peer","from":"{from}","handback":true,"handbackReportText":"{report}"}}}}}}"#
        )
    }

    /// Delivery to an idle spawner: a `user` entry with a top-level origin.
    fn handback_as_user(from: &str, report: &str) -> String {
        format!(
            r#"{{"type":"user","uuid":"h-{from}","parentUuid":null,"isMeta":true,"origin":{{"kind":"peer","from":"{from}","handback":true,"handbackReportText":"{report}"}},"message":{{"role":"user","content":"<agent-message from=\"{from}\">"}}}}"#
        )
    }

    fn write(path: &Path, lines: &[String]) {
        fs::write(path, lines.join("\n") + "\n").unwrap();
    }

    /// Parent spawns `aaa` (step 1); `aaa` spawns `bbb` (its step 1);
    /// `ccc` has no meta sidecar. Each report lands in its spawner.
    fn fixture() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join("s1.jsonl");
        write(
            &session,
            &[
                user("u1", "go"),
                agent_call("a1", "toolu_A"),
                handback("aaa", "aaa done"),
            ],
        );
        let dir = tmp.path().join("s1").join("subagents");
        fs::create_dir_all(&dir).unwrap();
        write(
            &dir.join("agent-aaa.jsonl"),
            &[
                user("x1", "task"),
                agent_call("x2", "toolu_B"),
                handback("bbb", "bbb done"),
            ],
        );
        fs::write(
            dir.join("agent-aaa.meta.json"),
            r#"{"agentType":"general-purpose","description":"outer","toolUseId":"toolu_A","spawnDepth":1,"finished":true}"#,
        )
        .unwrap();
        write(&dir.join("agent-bbb.jsonl"), &[user("y1", "inner task")]);
        fs::write(
            dir.join("agent-bbb.meta.json"),
            r#"{"toolUseId":"toolu_B","spawnDepth":2}"#,
        )
        .unwrap();
        write(&dir.join("agent-ccc.jsonl"), &[user("z1", "orphan")]);
        (tmp, session)
    }

    fn find<'a>(tree: &'a SessionTree, id: &str) -> &'a Subagent {
        tree.subagents.iter().find(|s| s.agent_id == id).unwrap()
    }

    #[test]
    fn links_top_level_subagent_to_parent_step() {
        let (_tmp, session) = fixture();
        let tree = load_tree(&session).unwrap();
        let aaa = find(&tree, "aaa");
        assert_eq!(aaa.spawned_by, Spawner::Session);
        assert_eq!(aaa.spawn_step, Some(1));
        assert_eq!(aaa.agent_type.as_deref(), Some("general-purpose"));
        assert_eq!(aaa.finished, Some(true));
        assert_eq!(aaa.reports, vec!["aaa done".to_string()]);
    }

    #[test]
    fn links_nested_subagent_to_its_spawning_subagent() {
        let (_tmp, session) = fixture();
        let tree = load_tree(&session).unwrap();
        let bbb = find(&tree, "bbb");
        assert_eq!(
            bbb.spawned_by,
            Spawner::Subagent {
                agent_id: "aaa".into()
            }
        );
        assert_eq!(bbb.spawn_step, Some(1));
        assert_eq!(bbb.spawn_depth, Some(2));
        assert_eq!(bbb.reports, vec!["bbb done".to_string()]);
    }

    #[test]
    fn subagent_without_meta_is_kept_unlinked_and_sorted_last() {
        let (_tmp, session) = fixture();
        let tree = load_tree(&session).unwrap();
        let last = tree.subagents.last().unwrap();
        assert_eq!(last.agent_id, "ccc");
        assert_eq!(last.spawned_by, Spawner::Unknown);
        assert_eq!(last.step_count, 1);
    }

    #[test]
    fn session_without_subagents_dir_has_empty_tree() {
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join("solo.jsonl");
        write(&session, &[user("u1", "hi")]);
        let tree = load_tree(&session).unwrap();
        assert!(tree.subagents.is_empty());
        assert_eq!(tree.steps.len(), 1);
    }

    #[test]
    fn unparseable_subagent_records_load_error() {
        let (tmp, session) = fixture();
        fs::write(
            tmp.path().join("s1/subagents/agent-bad.jsonl"),
            "{not json\n",
        )
        .unwrap();
        let tree = load_tree(&session).unwrap();
        let bad = find(&tree, "bad");
        assert!(bad.load_error.is_some());
        assert_eq!(bad.step_count, 0);
    }

    #[test]
    fn reads_user_entry_handback_and_skips_queue_copy() {
        let tmp = TempDir::new().unwrap();
        let session = tmp.path().join("s.jsonl");
        // The queue-operation line repeats the hand-back marker; it must
        // not produce a second report.
        let queued = r#"{"type":"queue-operation","operation":"enqueue","content":"x","origin":{"kind":"peer","from":"aaa","handback":true,"handbackReportText":"dup"}}"#;
        write(
            &session,
            &[queued.to_string(), handback_as_user("aaa", "idle report")],
        );
        let reports = scan_handbacks(&session).unwrap();
        assert_eq!(reports["aaa"], vec!["idle report".to_string()]);
    }

    #[test]
    fn report_from_body_strips_preamble_and_indent() {
        let body = "[Subagent hand-back] preamble. The report follows:\n  line one\n  \n  - item";
        assert_eq!(report_from_body(body), "line one\n\n- item");
        assert_eq!(report_from_body("no marker"), "no marker");
    }

    #[test]
    fn subagents_dir_strips_extension() {
        assert_eq!(
            subagents_dir(Path::new("/p/abc.jsonl")),
            Some(PathBuf::from("/p/abc/subagents"))
        );
    }
}
