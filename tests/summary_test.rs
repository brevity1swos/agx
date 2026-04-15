//! End-to-end tests for the `--summary` non-interactive mode.
//!
//! These tests guard against drift between the README's documented output
//! and the actual CLI output. Uses `std::process::Command` directly rather
//! than pulling in `assert_cmd` as a dev-dep.

use std::path::PathBuf;
use std::process::Command;

fn agx_bin() -> PathBuf {
    // `cargo test` sets CARGO_BIN_EXE_<name> for each binary target.
    PathBuf::from(env!("CARGO_BIN_EXE_agx"))
}

fn run_summary(fixture: &str) -> String {
    let output = Command::new(agx_bin())
        .arg("--summary")
        .arg(fixture)
        .output()
        .expect("failed to run agx");
    assert!(
        output.status.success(),
        "agx --summary {fixture} exited with status {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("agx stdout was not UTF-8")
}

#[test]
fn summary_claude_code_fixture_has_expected_shape() {
    let out = run_summary("assets/sample_session.jsonl");
    assert!(
        out.starts_with("Loaded Claude Code session from"),
        "unexpected first line: {out}"
    );
    assert!(out.contains("timeline steps:"), "no step count line in: {out}");
    assert!(out.contains("user,"), "missing user count in: {out}");
    assert!(out.contains("assistant,"), "missing assistant count in: {out}");
    assert!(out.contains("tool_uses,"), "missing tool_uses count in: {out}");
    assert!(out.contains("tool_results"), "missing tool_results count in: {out}");
    assert!(out.contains("First 20:"), "missing first-steps header in: {out}");
}

#[test]
fn summary_codex_fixture_detects_format() {
    let out = run_summary("assets/sample_codex_session.jsonl");
    assert!(
        out.starts_with("Loaded Codex CLI session from"),
        "format label drift: {out}"
    );
}

#[test]
fn summary_gemini_fixture_detects_format() {
    let out = run_summary("assets/sample_gemini_session.json");
    assert!(
        out.starts_with("Loaded Gemini CLI session from"),
        "format label drift: {out}"
    );
}

#[test]
fn summary_generic_fixture_detects_format() {
    let out = run_summary("assets/sample_generic_session.json");
    assert!(
        out.starts_with("Loaded Generic conversation session from"),
        "format label drift: {out}"
    );
}

#[test]
fn summary_exit_code_nonzero_on_missing_file() {
    let output = Command::new(agx_bin())
        .arg("--summary")
        .arg("assets/does_not_exist.jsonl")
        .output()
        .expect("failed to run agx");
    assert!(!output.status.success(), "should fail on missing file");
}

#[test]
fn debug_unknowns_runs_without_error_and_prints_to_stderr() {
    let output = Command::new(agx_bin())
        .arg("--debug-unknowns")
        .arg("--summary")
        .arg("assets/sample_session.jsonl")
        .output()
        .expect("failed to run agx");
    assert!(output.status.success(), "agx exited non-zero");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[debug-unknowns]"),
        "debug report not on stderr: {stderr}"
    );
    // stdout should still contain the normal --summary output
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Loaded Claude Code"), "summary missing: {stdout}");
}
