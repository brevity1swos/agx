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
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Maximum bytes captured per stream (stdout / stderr) before we
/// stop reading. A runaway `yes` or `cat /dev/zero` would otherwise
/// OOM the TUI; capping the read ensures bounded memory use
/// regardless of what the replayed command emits. Once a cap is
/// hit, the pipe closes and the child receives SIGPIPE on its
/// next write — the timeout below catches any child that ignores
/// SIGPIPE.
const MAX_CAPTURE_BYTES: usize = 4 * 1024 * 1024;

/// Wall-clock deadline for a replay. A stuck `sleep 999999` or
/// a netcat listener would otherwise freeze the TUI indefinitely.
/// Chosen to be long enough for real build / test invocations but
/// short enough to not feel like a hang.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Poll interval while waiting for the child. 50ms is a good
/// balance between responsiveness and wasted wakeups.
const POLL_INTERVAL_MS: u64 = 50;

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
    match extract_shell_command(&step.detail) {
        Some(input) if !input.is_empty() => ReplayIntent::NeedsConfirm { input },
        _ => ReplayIntent::NotReplayable {
            reason: "could not extract shell command from step",
        },
    }
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
///
/// `timed_out` and the `*_truncated` flags surface the two
/// backpressure mechanisms (wall-clock deadline + per-stream byte
/// cap) so the TUI can render a clear marker instead of silently
/// hiding a truncated buffer.
#[derive(Debug)]
pub struct ReplayOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub fn execute_shell(input: &str) -> Result<ReplayOutput> {
    execute_shell_with_limits(input, DEFAULT_TIMEOUT_SECS, MAX_CAPTURE_BYTES)
}

/// Same as [`execute_shell`] but with caller-supplied limits. Kept
/// `pub(crate)` so tests can run with tiny caps / deadlines
/// without waiting 30s per test.
pub(crate) fn execute_shell_with_limits(
    input: &str,
    timeout_secs: u64,
    max_capture_bytes: usize,
) -> Result<ReplayOutput> {
    let start = std::time::Instant::now();
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning /bin/sh for replay")?;

    // Read each stream on its own thread with a byte cap. `take(N)`
    // wraps the ChildStdout in `io::Take`; when that wrapper is
    // dropped the underlying pipe closes, which sends SIGPIPE to
    // the child on the next write. For a child that ignores
    // SIGPIPE the deadline-kill below is the fallback.
    let stdout_h = child.stdout.take().context("replay child stdout handle")?;
    let stderr_h = child.stderr.take().context("replay child stderr handle")?;
    let cap = max_capture_bytes;
    let t_out = thread::spawn(move || read_capped(stdout_h, cap));
    let t_err = thread::spawn(move || read_capped(stderr_h, cap));

    // Poll `try_wait` with a short sleep until the child exits or
    // we hit the wall-clock deadline. `std::process::Child` doesn't
    // have `wait_timeout` on stable, so this is the portable form.
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut timed_out = false;
    let exit_code = loop {
        match child.try_wait().context("polling replay child")? {
            Some(status) => break status.code(),
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                timed_out = true;
                let status = child.wait().context("waiting for killed replay child")?;
                break status.code();
            }
            None => thread::sleep(Duration::from_millis(POLL_INTERVAL_MS)),
        }
    };

    let (out_bytes, stdout_truncated) = t_out.join().unwrap_or_else(|_| (Vec::new(), false));
    let (err_bytes, stderr_truncated) = t_err.join().unwrap_or_else(|_| (Vec::new(), false));
    let duration_ms = start.elapsed().as_millis();

    Ok(ReplayOutput {
        stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
        stderr: String::from_utf8_lossy(&err_bytes).into_owned(),
        exit_code,
        duration_ms,
        timed_out,
        stdout_truncated,
        stderr_truncated,
    })
}

/// Drain a child-owned byte stream with a hard cap. Returns
/// `(bytes, truncated)`. On `truncated = true` we keep reading
/// (discarding) after the cap so the child's pipe doesn't block
/// on a full kernel buffer — the thread exits on EOF or error,
/// whichever comes first.
fn read_capped<R: Read>(mut stream: R, cap: usize) -> (Vec<u8>, bool) {
    let mut buf = Vec::with_capacity(std::cmp::min(cap, 8192));
    let mut total = 0usize;
    let mut scratch = [0u8; 4096];
    let mut truncated = false;
    loop {
        let remaining = cap.saturating_sub(total);
        let want = if remaining == 0 {
            scratch.len()
        } else {
            remaining.min(scratch.len())
        };
        match stream.read(&mut scratch[..want]) {
            Ok(0) => return (buf, truncated),
            Ok(n) if remaining == 0 => {
                // Past the cap — discard. `truncated` already set.
                let _ = n;
            }
            Ok(n) => {
                buf.extend_from_slice(&scratch[..n]);
                total += n;
                if total >= cap {
                    truncated = true;
                }
            }
            Err(_) => return (buf, truncated),
        }
    }
}

/// Persistent log entry appended to `<session>.replay.log` after
/// every replay. Schema is stable so downstream tooling can
/// `jq` the file. Fields added after launch (`timed_out`,
/// `stdout_truncated`, `stderr_truncated`) are additive — old
/// consumers that ignore unknown fields keep working.
#[derive(Debug, Serialize)]
struct ReplayLogEntry<'a> {
    ts_ms: u128,
    step_index: usize,
    tool_name: &'a str,
    tool_call_id: &'a str,
    input: &'a str,
    exit_code: Option<i32>,
    duration_ms: u128,
    timed_out: bool,
    stdout_truncated: bool,
    stderr_truncated: bool,
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
        timed_out: output.timed_out,
        stdout_truncated: output.stdout_truncated,
        stderr_truncated: output.stderr_truncated,
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
    fn classify_refuses_malformed_input() {
        // A Bash step whose `Input:` section isn't valid JSON
        // must surface as NotReplayable, not as NeedsConfirm
        // with an empty string (which would otherwise spawn
        // `/bin/sh -c ""` and log a misleading exit=0 entry).
        let step = tool_use_step("t1", "Bash", "not-json-at-all");
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
    fn classify_refuses_empty_command_string() {
        // Defensive: a well-formed JSON with an empty command
        // field must also be rejected, same reason as above.
        let step = tool_use_step("t1", "Bash", "{\"command\":\"\"}");
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
            timed_out: false,
            stdout_truncated: false,
            stderr_truncated: false,
        };
        log_replay(session.path(), 7, &step, "echo hi", &output).unwrap();
        let mut log_path = session.path().as_os_str().to_os_string();
        log_path.push(".replay.log");
        let content = std::fs::read_to_string::<std::path::PathBuf>(log_path.into()).unwrap();
        assert!(content.contains("\"step_index\":7"));
        assert!(content.contains("\"exit_code\":0"));
        assert!(content.contains("\"stdout\":\"hi\\n\""));
        assert!(content.contains("\"timed_out\":false"));
        assert!(content.contains("\"stdout_truncated\":false"));
    }

    #[test]
    fn execute_shell_caps_stdout_at_the_limit() {
        // A tight cap (64 bytes) paired with a 2KB write proves the
        // reader drops bytes past the cap and flags `stdout_truncated`.
        // The child keeps writing — we need `read_capped` to keep
        // draining so the child doesn't block on a full pipe.
        //
        // Uses awk's POSIX BEGIN block rather than bash brace
        // expansion (`{1..2000}`) so the test passes under dash —
        // `/bin/sh` on Debian/Ubuntu CI runners doesn't expand
        // braces.
        let out = execute_shell_with_limits("awk 'BEGIN{for(i=0;i<2000;i++)printf \"x\"}'", 10, 64)
            .expect("shell runs");
        assert_eq!(out.stdout.len(), 64, "stdout capped to exactly 64B");
        assert!(out.stdout_truncated, "truncation flag set");
        assert!(!out.timed_out, "did not time out");
    }

    #[test]
    fn execute_shell_caps_stderr_at_the_limit() {
        // Symmetric coverage for the stderr cap. Same awk shape,
        // redirected to fd 2.
        let out =
            execute_shell_with_limits("awk 'BEGIN{for(i=0;i<2000;i++)printf \"x\"}' 1>&2", 10, 64)
                .expect("shell runs");
        assert_eq!(out.stderr.len(), 64, "stderr capped to exactly 64B");
        assert!(out.stderr_truncated, "stderr truncation flag set");
        assert!(!out.stdout_truncated, "stdout untouched");
    }

    #[test]
    fn execute_shell_times_out_on_long_running() {
        // `sleep 60` would finish well past the 1s deadline. The
        // replay should kill it, set `timed_out`, and still return.
        let start = std::time::Instant::now();
        let out = execute_shell_with_limits("sleep 60", 1, 1024).expect("shell runs");
        let elapsed = start.elapsed().as_secs();
        assert!(out.timed_out, "timeout flag set");
        assert!(
            elapsed < 5,
            "killed near the deadline, not 60s: elapsed={elapsed}s"
        );
    }

    #[test]
    fn log_replay_records_timeout_flag() {
        use tempfile::NamedTempFile;
        let session = NamedTempFile::new().unwrap();
        let step = tool_use_step("t1", "Bash", "{\"command\":\"sleep 999\"}");
        let output = ReplayOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            duration_ms: 1000,
            timed_out: true,
            stdout_truncated: false,
            stderr_truncated: false,
        };
        log_replay(session.path(), 3, &step, "sleep 999", &output).unwrap();
        let mut log_path = session.path().as_os_str().to_os_string();
        log_path.push(".replay.log");
        let content = std::fs::read_to_string::<std::path::PathBuf>(log_path.into()).unwrap();
        assert!(
            content.contains("\"timed_out\":true"),
            "timeout flag in sidecar log"
        );
    }
}
