//! Phase 5.4 — experimental tool-call replay. Lives in the bin
//! crate because replay is interactive UX (confirm prompts, live
//! output), not pure library logic.
//!
//! # Gates
//!
//! Three independent checks must all pass before a shell replay
//! actually runs:
//!
//! 1. `--experimental-replay` — announces intent at launch; users
//!    on a stock build cannot accidentally trigger replay.
//! 2. `--allow-shell-replay` — tool-kind gate. A future MCP or
//!    API backend would have its own `--allow-*-replay` flag.
//! 3. Per-invocation confirm (`y` in the TUI) — even with both
//!    flags, every `R` press asks before executing.
//!
//! The triple gate mirrors the ROADMAP Phase 5.4 spec and the
//! "this is where we leave safe-viewer territory" principle.
//!
//! # Side-channel logging
//!
//! Every replay attempt appends one JSON line to a sidecar file
//! `<session>.replay.log` next to the session. The original session
//! is NEVER touched — agx-core's read-only posture is absolute.
//! The log file is append-only; reviewers can `tail -f` it during
//! a session or `jq` it afterward.

use agx_core::timeline::{Step, StepKind};
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Runtime configuration derived from the top-level CLI flags.
/// Default is all-off — `--experimental-replay` is the opt-in.
#[derive(Debug, Clone, Copy, Default)]
pub struct ReplayConfig {
    pub enabled: bool,
    pub allow_shell: bool,
}

/// What happens when the TUI asks whether a step can be replayed.
/// The TUI uses this to decide whether to render a confirm prompt,
/// an install hint, or a flag-missing message.
#[derive(Debug)]
pub enum ReplayIntent {
    /// Flags allow it — show the confirm prompt in the status bar.
    NeedsConfirm { input: String },
    /// Step isn't replayable (not a shell tool_use, or no known
    /// backend matches). Renders as a status-bar note.
    NotReplayable { reason: &'static str },
    /// Flags block it — renders a hint directing the user at the
    /// flag they need.
    FlagMissing { hint: &'static str },
}

/// Decide whether the current step can be replayed given the
/// active config. Pure — no side effects, suitable for unit tests.
#[must_use]
pub fn classify(step: &Step, cfg: &ReplayConfig) -> ReplayIntent {
    if !cfg.enabled {
        return ReplayIntent::FlagMissing {
            hint: "replay requires `--experimental-replay` at launch",
        };
    }
    if step.kind != StepKind::ToolUse {
        return ReplayIntent::NotReplayable {
            reason: "replay only applies to tool_use steps",
        };
    }
    // v1 supports Bash-like shell replays only. API / MCP backends
    // land in follow-ups and will extend this classifier.
    let tool_name = step.tool_name.as_deref().unwrap_or("");
    let is_shell = matches!(tool_name, "Bash" | "bash" | "shell" | "Shell");
    if !is_shell {
        return ReplayIntent::NotReplayable {
            reason: "v1 replays Bash-like tools only (MCP / API backends pending)",
        };
    }
    if !cfg.allow_shell {
        return ReplayIntent::FlagMissing {
            hint: "shell replay requires `--allow-shell-replay` at launch",
        };
    }
    let input = extract_shell_command(&step.detail).unwrap_or_default();
    ReplayIntent::NeedsConfirm { input }
}

/// Pull the shell command out of a tool_use step's detail. The
/// step-constructor format is deterministic:
/// `Tool: Bash\nID: <id>\n\nInput:\n<input JSON>`.
/// The input is pretty-printed JSON containing the `command` field.
fn extract_shell_command(detail: &str) -> Option<String> {
    // Find the Input: section marker and parse the remainder as
    // JSON. Tolerate either a trailing blank line or end-of-string.
    let after_input = detail.split_once("\nInput:\n")?.1;
    let end = after_input
        .find("\n\nResult:\n")
        .unwrap_or(after_input.len());
    let input_json = &after_input[..end];
    let v: serde_json::Value = serde_json::from_str(input_json.trim()).ok()?;
    // Most agent CLIs name the shell command `command` on Bash
    // tools; fall back to `cmd` (Gemini's shape).
    v.get("command")
        .or_else(|| v.get("cmd"))
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

/// Execute a confirmed shell replay. Returns stdout / stderr /
/// exit code plus wall-clock duration. Does not print anything —
/// the caller (TUI) owns the display.
#[derive(Debug)]
pub struct ReplayOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
}

pub fn execute_shell(input: &str) -> Result<ReplayOutput> {
    let start = std::time::Instant::now();
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(input)
        .output()
        .with_context(|| "spawning /bin/sh for replay")?;
    let duration_ms = start.elapsed().as_millis();
    Ok(ReplayOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
        duration_ms,
    })
}

/// Persistent log entry appended to `<session>.replay.log` after
/// every replay. Schema is stable so downstream tooling can
/// `jq` the file.
#[derive(Debug, Serialize)]
struct ReplayLogEntry<'a> {
    ts_ms: u128,
    step_index: usize,
    tool_name: &'a str,
    tool_call_id: &'a str,
    input: &'a str,
    exit_code: Option<i32>,
    duration_ms: u128,
    stdout: &'a str,
    stderr: &'a str,
}

/// Append one replay log line. Sidecar file, session file is
/// never touched. Best-effort — a write failure surfaces via
/// `Err` but the caller decides whether to present it.
pub fn log_replay(
    session_path: &Path,
    step_index: usize,
    step: &Step,
    input: &str,
    output: &ReplayOutput,
) -> Result<()> {
    let mut sidecar = session_path.as_os_str().to_os_string();
    sidecar.push(".replay.log");
    let path: std::path::PathBuf = sidecar.into();
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let entry = ReplayLogEntry {
        ts_ms,
        step_index,
        tool_name: step.tool_name.as_deref().unwrap_or(""),
        tool_call_id: step.tool_call_id.as_deref().unwrap_or(""),
        input,
        exit_code: output.exit_code,
        duration_ms: output.duration_ms,
        stdout: &output.stdout,
        stderr: &output.stderr,
    };
    let line = serde_json::to_string(&entry)?;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agx_core::timeline::{tool_use_step, user_text_step};

    #[test]
    fn classify_refuses_without_experimental_flag() {
        let step = tool_use_step("t1", "Bash", "{\"command\":\"ls\"}");
        let cfg = ReplayConfig::default();
        assert!(matches!(
            classify(&step, &cfg),
            ReplayIntent::FlagMissing { .. }
        ));
    }

    #[test]
    fn classify_refuses_shell_without_allow_shell_flag() {
        let step = tool_use_step("t1", "Bash", "{\"command\":\"ls\"}");
        let cfg = ReplayConfig {
            enabled: true,
            allow_shell: false,
        };
        match classify(&step, &cfg) {
            ReplayIntent::FlagMissing { hint } => {
                assert!(hint.contains("--allow-shell-replay"));
            }
            other => panic!("expected FlagMissing, got {other:?}"),
        }
    }

    #[test]
    fn classify_rejects_non_tool_use_steps() {
        let step = user_text_step("hi");
        let cfg = ReplayConfig {
            enabled: true,
            allow_shell: true,
        };
        assert!(matches!(
            classify(&step, &cfg),
            ReplayIntent::NotReplayable { .. }
        ));
    }

    #[test]
    fn classify_rejects_non_shell_tools() {
        let step = tool_use_step("t1", "Read", "{\"file_path\":\"/x\"}");
        let cfg = ReplayConfig {
            enabled: true,
            allow_shell: true,
        };
        assert!(matches!(
            classify(&step, &cfg),
            ReplayIntent::NotReplayable { .. }
        ));
    }

    #[test]
    fn classify_accepts_shell_with_all_gates() {
        let step = tool_use_step("t1", "Bash", "{\"command\":\"ls -la\"}");
        let cfg = ReplayConfig {
            enabled: true,
            allow_shell: true,
        };
        match classify(&step, &cfg) {
            ReplayIntent::NeedsConfirm { input } => assert_eq!(input, "ls -la"),
            other => panic!("expected NeedsConfirm, got {other:?}"),
        }
    }

    #[test]
    fn classify_falls_back_to_cmd_field() {
        // Gemini uses `cmd` instead of `command`.
        let step = tool_use_step("t1", "Bash", "{\"cmd\":\"echo hi\"}");
        let cfg = ReplayConfig {
            enabled: true,
            allow_shell: true,
        };
        match classify(&step, &cfg) {
            ReplayIntent::NeedsConfirm { input } => assert_eq!(input, "echo hi"),
            other => panic!("expected NeedsConfirm, got {other:?}"),
        }
    }

    #[test]
    fn execute_shell_captures_exit_and_stdout() {
        let out = execute_shell("echo hello && false").expect("shell runs");
        assert_eq!(out.exit_code, Some(1));
        assert!(out.stdout.contains("hello"));
    }

    #[test]
    fn log_replay_appends_jsonl_sidecar() {
        use tempfile::NamedTempFile;
        let session = NamedTempFile::new().unwrap();
        let step = tool_use_step("t1", "Bash", "{\"command\":\"echo hi\"}");
        let output = ReplayOutput {
            stdout: "hi\n".into(),
            stderr: String::new(),
            exit_code: Some(0),
            duration_ms: 42,
        };
        log_replay(session.path(), 7, &step, "echo hi", &output).unwrap();
        let mut log_path = session.path().as_os_str().to_os_string();
        log_path.push(".replay.log");
        let content = std::fs::read_to_string::<std::path::PathBuf>(log_path.into()).unwrap();
        assert!(content.contains("\"step_index\":7"));
        assert!(content.contains("\"exit_code\":0"));
        assert!(content.contains("\"stdout\":\"hi\\n\""));
    }
}
