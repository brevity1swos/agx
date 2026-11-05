//! Check a subagent's "tests pass" claim against its own transcript.
//!
//! A subagent's hand-back report is prose; the transcript holds what it
//! actually ran. This module finds the report sentence that claims tests
//! pass, finds the last test run in the transcript by parsing runner
//! summary lines out of tool results, and compares the two:
//!
//! - **contradicted** — the last run had failures.
//! - **stale** — files were edited (Write / Edit / MultiEdit /
//!   NotebookEdit) after the last run, so it doesn't cover the final code.
//! - **unverified** — no parseable test run in the transcript, a last
//!   run that executed no tests (a filter that matched nothing), or a
//!   filtered last run behind a claim about all tests.
//! - **supported** — the last run passed and nothing was edited after it.
//!
//! Only the exit summary is trusted, never `is_error`: piping a test
//! command through `tail` hides its exit code, so a failing run is often
//! recorded as a successful tool call. Recognized summaries: cargo
//! (`test result:`), pytest (`=== N passed … ===`), jest (`Tests: …`),
//! vitest (`Tests  … (N)`). Edits made through Bash (`sed -i`, heredocs)
//! are not detected.

use crate::timeline::{Step, StepKind};
use serde::Serialize;

const EDIT_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

const SHELL_TOOLS: &[&str] = &["Bash", "PowerShell"];

/// Edits to these files can't change test outcomes, so they don't make a
/// run stale.
const DOC_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "rst", "adoc"];

/// Runner invocations, used to flag test commands whose output had no
/// recognizable summary. Bare `vitest` / `jest` would also match package
/// names in `npm view` and grep output, so only invocation forms are listed.
const TEST_COMMANDS: &[&str] = &[
    "cargo test",
    "cargo nextest",
    "pytest",
    "npm test",
    "npm run test",
    "pnpm test",
    "yarn test",
    "npx vitest",
    "vitest run",
    "npx jest",
    "go test",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestRun {
    /// 0-based step index of the tool result holding the summary.
    pub step: usize,
    pub passed: u64,
    pub failed: u64,
    /// The test-name filter the run's command passed (`cargo test foo`,
    /// `pytest -k foo`, `go test -run Foo`, `jest -t foo`), if any.
    pub filter: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Supported,
    Contradicted,
    Stale,
    Unverified,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaimCheck {
    /// The report sentence that claims tests pass.
    pub claim: String,
    /// A count stated in that sentence ("17 tests"), if any.
    pub claimed_count: Option<u64>,
    pub verdict: Verdict,
    pub last_run: Option<TestRun>,
    /// File-edit tool calls after `last_run`, not counting doc files.
    pub edits_after_run: usize,
    /// Test commands after `last_run` (or anywhere, without one) whose
    /// output carried no recognizable summary — e.g. redirected to a file.
    /// Nonzero means the verdict may rest on an earlier run than the last.
    pub later_unparsed_runs: usize,
}

/// Check `report` against `steps`. `None` when the report makes no
/// "tests pass" claim.
pub fn check(report: &str, steps: &[Step]) -> Option<ClaimCheck> {
    let claim = find_pass_claim(report)?;
    let claimed_count = claimed_count(&claim);

    let mut last_run: Option<TestRun> = None;
    let mut unparsed = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        // Only shell output is evidence: a Read of an old log would pass
        // for a run, and the hand-back call's input quotes the report.
        let from_shell = step
            .tool_name
            .as_deref()
            .is_some_and(|n| SHELL_TOOLS.contains(&n));
        if step.kind != StepKind::ToolResult || !from_shell {
            continue;
        }
        let (input, output) = split_result_detail(&step.detail);
        match parse_summary(&output) {
            Some((passed, failed)) => {
                last_run = Some(TestRun {
                    step: i,
                    passed,
                    failed,
                    filter: test_filter(input),
                });
            }
            None if invokes_test_command(input) => unparsed.push(i),
            None => {}
        }
    }
    let after = last_run.as_ref().map_or(0, |run| run.step);
    let later_unparsed_runs = unparsed.iter().filter(|&&i| i >= after).count();

    let edits_after_run = last_run.as_ref().map_or(0, |run| {
        steps[run.step..].iter().filter(|s| is_code_edit(s)).count()
    });

    let verdict = match &last_run {
        None => Verdict::Unverified,
        Some(run) if run.failed > 0 => Verdict::Contradicted,
        // "0 passed; 0 failed" passes vacuously, e.g. a filter with a typo.
        Some(run) if run.passed == 0 => Verdict::Unverified,
        Some(_) if edits_after_run > 0 => Verdict::Stale,
        Some(run) if run.filter.is_some() && claims_all_tests(&claim) => Verdict::Unverified,
        Some(_) => Verdict::Supported,
    };

    Some(ClaimCheck {
        claim,
        claimed_count,
        verdict,
        last_run,
        edits_after_run,
        later_unparsed_runs,
    })
}

/// Whether a shell call's `command` runs a test suite: a runner from
/// [`TEST_COMMANDS`] must start a command segment (after `&&`, `;`, `|`, or
/// a newline, past any `VAR=value` prefixes). A runner named inside a PR
/// body or commit message doesn't count.
fn invokes_test_command(input: &str) -> bool {
    !runner_invocations(&shell_command(input)).is_empty()
}

/// The first test-name filter among the runner invocations in a shell
/// call's command. Package and path selection (`-p x`, `pytest tests/`)
/// is not a filter; only arguments that pick tests by name are.
fn test_filter(input: &str) -> Option<String> {
    runner_invocations(&shell_command(input))
        .into_iter()
        .find_map(|(runner, args)| match runner {
            "cargo test" | "cargo nextest" => cargo_filter(runner, &args),
            "pytest" => flag_value(&args, &["-k"]).or_else(|| {
                args.iter()
                    .find(|a| a.contains("::"))
                    .map(|a| a.to_string())
            }),
            "go test" => flag_value(&args, &["-run", "--run"]),
            _ => flag_value(&args, &["-t", "--testNamePattern"]),
        })
}

/// The `command` string of a shell call's `Input:` JSON.
fn shell_command(input: &str) -> String {
    input
        .split_once("Input:\n")
        .and_then(|(_, json)| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v["command"].as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Each command segment that starts with a runner from [`TEST_COMMANDS`],
/// as (runner, the words after it). Redirections (`> out.log`, `2>`) are
/// dropped from the words so their targets don't read as filters.
fn runner_invocations(command: &str) -> Vec<(&'static str, Vec<&str>)> {
    command
        .split(['\n', ';', '|', '&'])
        .filter_map(|segment| {
            let words: Vec<&str> = segment
                .split_whitespace()
                .skip_while(|w| w.contains('=') && !w.starts_with('-'))
                .collect();
            let joined = words.join(" ");
            let runner = TEST_COMMANDS.iter().find(|c| joined.starts_with(*c))?;
            let mut args = Vec::new();
            let mut rest = words[runner.split(' ').count()..].iter();
            while let Some(&word) = rest.next() {
                if word.contains(['>', '<']) {
                    if word.ends_with(['>', '<']) {
                        rest.next();
                    }
                    continue;
                }
                args.push(word);
            }
            Some((*runner, args))
        })
        .collect()
}

/// cargo test / nextest: a bare word picks tests by name, before or after
/// `--`; so does a nextest filterset (`-E`). Flags that take a value are
/// skipped together with it.
fn cargo_filter(runner: &str, args: &[&str]) -> Option<String> {
    const VALUE_FLAGS: &[&str] = &[
        "-p",
        "--package",
        "--exclude",
        "-F",
        "--features",
        "--bin",
        "--test",
        "--example",
        "--bench",
        "--manifest-path",
        "-j",
        "--jobs",
        "--target",
        "--target-dir",
        "--profile",
        "--cargo-profile",
        "--color",
        "--message-format",
        "--config",
        "-Z",
        "--skip",
        "--test-threads",
        "--format",
        "--logfile",
    ];
    if let Some(expr) = flag_value(args, &["-E", "--filterset", "--filter-expr"]) {
        return Some(expr);
    }
    let mut words = args.iter();
    if runner == "cargo nextest" && args.first() == Some(&"run") {
        words.next();
    }
    while let Some(&word) = words.next() {
        if VALUE_FLAGS.contains(&word) {
            words.next();
        } else if !word.starts_with('-') {
            return Some(word.to_string());
        }
    }
    None
}

/// The value of the first of `flags` in `args`, as `-k expr` or `-k=expr`.
fn flag_value(args: &[&str], flags: &[&str]) -> Option<String> {
    args.iter().enumerate().find_map(|(i, arg)| {
        flags.iter().find_map(|flag| {
            if arg == flag {
                args.get(i + 1).map(|v| v.to_string())
            } else {
                arg.strip_prefix(flag)
                    .and_then(|rest| rest.strip_prefix('='))
                    .map(str::to_string)
            }
        })
    })
}

/// Whether a claim covers the whole suite ("all pass", "the full suite")
/// rather than the tests a filter would select.
fn claims_all_tests(claim: &str) -> bool {
    let lower = claim.to_lowercase();
    ["all ", "full ", "entire ", "whole ", "every "]
        .iter()
        .any(|w| lower.contains(w))
}

/// An edit-tool call whose target isn't a doc file. A call whose input
/// can't be read counts as a code edit, so the check errs toward "stale".
fn is_code_edit(step: &Step) -> bool {
    if step.kind != StepKind::ToolUse
        || !step
            .tool_name
            .as_deref()
            .is_some_and(|n| EDIT_TOOLS.contains(&n))
    {
        return false;
    }
    let path = step
        .detail
        .split_once("\nInput:\n")
        .and_then(|(_, input)| serde_json::from_str::<serde_json::Value>(input).ok())
        .and_then(|input| {
            let path = input
                .get("file_path")
                .or_else(|| input.get("notebook_path"));
            path.and_then(|p| p.as_str()).map(str::to_string)
        });
    let is_doc = path.is_some_and(|p| {
        std::path::Path::new(&p)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| DOC_EXTENSIONS.contains(&e.to_lowercase().as_str()))
    });
    !is_doc
}

/// First sentence that pairs a test word with a pass word and carries no
/// disqualifier. "I wrote the test first and saw it fail" is not a pass
/// claim; "`cargo test` (17 tests) all pass" is. Disqualifiers cover
/// negation ("no tests exist") and "passed to" in the sense of handing an
/// argument over.
fn find_pass_claim(report: &str) -> Option<String> {
    const TEST_WORDS: &[&str] = &["test", "suite"];
    const PASS_WORDS: &[&str] = &["pass", "green"];
    const DISQUALIFIERS: &[&str] = &[
        "fail",
        "not ",
        "n't",
        "never",
        "unable",
        "skipped",
        "skip ",
        "except",
        "no test",
        "pass to ",
        "passes to ",
        "passed to ",
        "passing to ",
    ];
    sentences(report).find_map(|s| {
        let lower = s.to_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
        (has(TEST_WORDS) && has(PASS_WORDS) && !has(DISQUALIFIERS)).then(|| s.trim().to_string())
    })
}

/// Split on line breaks and on ". " so "(17 tests) all pass. For each fix"
/// yields two sentences while `v0.2.1` and `main.rs` stay whole.
fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .flat_map(|line| line.split(". "))
        .filter(|s| !s.trim().is_empty())
}

/// A number directly before "test" / "tests" / "passed" in the claim.
fn claimed_count(claim: &str) -> Option<u64> {
    let words: Vec<&str> = claim
        .split(|c: char| c.is_whitespace() || "(),;:`".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    words.windows(2).find_map(|pair| {
        let next = pair[1].to_lowercase();
        let counted = ["test", "tests", "passed"].contains(&next.as_str());
        if counted { pair[0].parse().ok() } else { None }
    })
}

/// Split a tool-result step's detail into (input, output). The detail is
/// `Tool: …\nID: …\n\n[Input:\n…\n\n]Result:\n<output>`. Results stored as
/// a content-item array are pretty-printed JSON; their text is decoded so
/// summary lines aren't hidden behind escaped `\n`.
fn split_result_detail(detail: &str) -> (&str, String) {
    let Some((input, output)) = detail.split_once("\nResult:\n") else {
        return ("", String::new());
    };
    let decoded = output
        .trim_start()
        .starts_with('[')
        .then(|| serde_json::from_str::<Vec<serde_json::Value>>(output).ok())
        .flatten()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        });
    (input, decoded.unwrap_or_else(|| output.to_string()))
}

/// Summed (passed, failed) over every summary line in one output. Cargo
/// prints one `test result:` line per test binary, so a single run has
/// several; they add up to the run's total. A leading count from
/// `| sort | uniq -c` ("   3 test result: ok. 0 passed…") multiplies.
fn parse_summary(output: &str) -> Option<(u64, u64)> {
    let mut found = false;
    let (mut passed, mut failed) = (0, 0);
    for line in output.lines() {
        let line = line.trim();
        let cargo = line.split_once("test result: ").and_then(|(prefix, rest)| {
            let times = match prefix.trim() {
                "" => 1,
                n => n.parse::<u64>().ok()?,
            };
            let (p, f) = count_pairs(rest);
            Some((p * times, f * times))
        });
        let counts = cargo
            // pytest: "===== 2 failed, 10 passed in 1.23s ====="
            .or_else(|| {
                (line.starts_with('=') && line.contains(" in "))
                    .then(|| count_pairs(line))
                    .filter(|(p, f)| p + f > 0)
            })
            // jest: "Tests:       1 failed, 10 passed, 11 total"
            .or_else(|| line.strip_prefix("Tests:").map(count_pairs))
            // vitest: "Tests  1 failed | 9 passed (10)"
            .or_else(|| line.strip_prefix("Tests ").map(count_pairs));
        if let Some((p, f)) = counts {
            found = true;
            passed += p;
            failed += f;
        }
    }
    found.then_some((passed, failed))
}

/// Sum `<n> passed` and `<n> failed` / `<n> error(s)` pairs in a line.
fn count_pairs(text: &str) -> (u64, u64) {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || ",;|()".contains(c))
        .filter(|w| !w.is_empty())
        .collect();
    let (mut passed, mut failed) = (0, 0);
    for pair in words.windows(2) {
        let Ok(n) = pair[0].parse::<u64>() else {
            continue;
        };
        match pair[1].trim_end_matches('.') {
            "passed" => passed += n,
            "failed" | "error" | "errors" => failed += n,
            _ => {}
        }
    }
    (passed, failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline::{tool_result_step, tool_use_step};

    fn bash(id: &str, command: &str, output: &str) -> [Step; 2] {
        let input = format!("{{\"command\": \"{command}\"}}");
        [
            tool_use_step(id, "Bash", &input),
            tool_result_step(id, output, Some("Bash"), Some(&input)),
        ]
    }

    const CARGO_OK: &str = "running 17 tests\ntest result: ok. 17 passed; 0 failed; 0 ignored\n";
    const CARGO_FAIL: &str = "test result: FAILED. 15 passed; 2 failed; 0 ignored\n";
    const REPORT: &str = "All fixes committed. `cargo test -p x` (17 tests) all pass. I wrote the test first and saw it fail.";

    #[test]
    fn supported_when_last_run_passes() {
        let steps: Vec<Step> = [
            bash("a", "cargo test", CARGO_FAIL),
            bash("b", "cargo test", CARGO_OK),
        ]
        .concat();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Supported);
        assert_eq!(c.claimed_count, Some(17));
        assert_eq!(c.last_run.unwrap().passed, 17);
    }

    #[test]
    fn contradicted_when_last_run_fails() {
        let steps: Vec<Step> = [
            bash("a", "cargo test", CARGO_OK),
            bash("b", "cargo test", CARGO_FAIL),
        ]
        .concat();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Contradicted);
        assert_eq!(c.last_run.unwrap().failed, 2);
    }

    #[test]
    fn stale_when_files_edited_after_last_run() {
        let mut steps: Vec<Step> = bash("a", "cargo test", CARGO_OK).to_vec();
        steps.push(tool_use_step("e", "Edit", "{}"));
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Stale);
        assert_eq!(c.edits_after_run, 1);
    }

    #[test]
    fn doc_edits_after_last_run_are_not_stale() {
        let mut steps: Vec<Step> = bash("a", "cargo test", CARGO_OK).to_vec();
        steps.push(tool_use_step(
            "e",
            "Edit",
            "{\n  \"file_path\": \"/repo/docs/guide.md\"\n}",
        ));
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Supported);
        assert_eq!(c.edits_after_run, 0);
    }

    #[test]
    fn unverified_without_any_test_run() {
        let steps: Vec<Step> = bash("a", "ls", "src\n").to_vec();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Unverified);
        assert_eq!(c.later_unparsed_runs, 0);
    }

    #[test]
    fn counts_test_commands_with_unparseable_output() {
        let steps: Vec<Step> = bash("a", "cargo test", "error: could not compile\n").to_vec();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Unverified);
        assert_eq!(c.later_unparsed_runs, 1);
    }

    #[test]
    fn counts_only_unparsed_runs_after_the_last_parsed_one() {
        let steps: Vec<Step> = [
            bash("a", "npm test > early.out", ""),
            bash("b", "cargo test", CARGO_OK),
            bash("c", "npm test > late.out", ""),
            bash("d", "npm view vitest version", "4.1.0\n"),
        ]
        .concat();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.later_unparsed_runs, 1);
    }

    #[test]
    fn unverified_when_last_run_executed_no_tests() {
        let empty = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 31 filtered out\n";
        let steps: Vec<Step> = [
            bash("a", "cargo test", CARGO_OK),
            bash("b", "cargo test parse_sumary", empty),
        ]
        .concat();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Unverified);
        assert_eq!(c.last_run.unwrap().filter.as_deref(), Some("parse_sumary"));
    }

    #[test]
    fn filtered_run_cannot_back_an_all_tests_claim() {
        let steps: Vec<Step> = bash("a", "cargo test -p x parse_summary", CARGO_OK).to_vec();
        // REPORT says "(17 tests) all pass".
        assert_eq!(check(REPORT, &steps).unwrap().verdict, Verdict::Unverified);
        let narrow = check("The new parse_summary test passes.", &steps).unwrap();
        assert_eq!(narrow.verdict, Verdict::Supported);
        assert_eq!(
            narrow.last_run.unwrap().filter.as_deref(),
            Some("parse_summary")
        );
    }

    #[test]
    fn finds_test_name_filters_per_runner() {
        let filter =
            |command: &str| test_filter(&format!("Input:\n{{\"command\": \"{command}\"}}"));
        assert_eq!(filter("cargo test foo").as_deref(), Some("foo"));
        assert_eq!(filter("cargo test -- --exact foo").as_deref(), Some("foo"));
        assert_eq!(
            filter("cargo nextest run -E test(foo)").as_deref(),
            Some("test(foo)")
        );
        assert_eq!(filter("pytest -k slow tests/").as_deref(), Some("slow"));
        assert_eq!(
            filter("pytest tests/a.py::test_b").as_deref(),
            Some("tests/a.py::test_b")
        );
        assert_eq!(filter("go test ./... -run TestX").as_deref(), Some("TestX"));
        assert_eq!(filter("npm test -- -t login").as_deref(), Some("login"));
        assert_eq!(
            filter("npx jest --testNamePattern=login").as_deref(),
            Some("login")
        );
        // Package, target and path selection, harness flags and
        // redirections are not test-name filters.
        for command in [
            "cargo test",
            "cargo test -p agx-core --features otel-proto",
            "cargo test --workspace -- --nocapture --test-threads 1",
            "cargo test --test cli 2>&1 | tail -5",
            "cargo test > out.log",
            "cargo nextest run --workspace",
            "pytest tests/",
            "go test ./...",
            "npm test",
        ] {
            assert_eq!(filter(command), None, "{command}");
        }
    }

    #[test]
    fn no_check_without_a_pass_claim() {
        assert!(check("Reviewed the parsers; found two bugs.", &[]).is_none());
        assert!(check("Tests do not pass yet.", &[]).is_none());
        assert!(check("The new test fails as expected.", &[]).is_none());
        assert!(check("Lint clean, no tests exist, and the build passes.", &[]).is_none());
        assert!(check("That file passed to `rgx --test tests/urls.toml`.", &[]).is_none());
    }

    #[test]
    fn runner_named_in_prose_is_not_a_test_command() {
        let steps: Vec<Step> = [
            bash("a", "cargo test", CARGO_OK),
            bash(
                "b",
                "gh pr create --body 'ran npm test (168 passed)'",
                "https://x/pull/1\n",
            ),
            bash("c", "CI=1 npm test > out.log", ""),
        ]
        .concat();
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.later_unparsed_runs, 1);
    }

    #[test]
    fn ignores_non_shell_results() {
        // A Read of an old log holding a summary is not a run.
        let input = "{\"file_path\": \"/tmp/test.log\"}";
        let steps = [
            tool_use_step("r", "Read", input),
            tool_result_step("r", CARGO_FAIL, Some("Read"), Some(input)),
        ];
        let c = check(REPORT, &steps).unwrap();
        assert_eq!(c.verdict, Verdict::Unverified);
    }

    #[test]
    fn sums_cargo_summaries_across_test_binaries() {
        let out = "test result: ok. 10 passed; 0 failed\ntest result: ok. 7 passed; 0 failed\n";
        assert_eq!(parse_summary(out), Some((17, 0)));
    }

    #[test]
    fn multiplies_uniq_c_counts_on_cargo_summaries() {
        let out = "   3 test result: ok. 0 passed; 0 failed\n   2 test result: ok. 1 passed; 0 failed\n   1 test result: ok. 108 passed; 0 failed\n";
        assert_eq!(parse_summary(out), Some((110, 0)));
        // A prefix that isn't a count is not a summary line.
        assert_eq!(parse_summary("grep: test result: ok. 5 passed"), None);
    }

    #[test]
    fn parses_pytest_jest_and_vitest_summaries() {
        assert_eq!(
            parse_summary("===== 2 failed, 10 passed in 1.23s ====="),
            Some((10, 2))
        );
        assert_eq!(
            parse_summary("Tests:       1 failed, 10 passed, 11 total"),
            Some((10, 1))
        );
        assert_eq!(
            parse_summary(" Tests  1 failed | 9 passed (10)"),
            Some((9, 1))
        );
        assert_eq!(parse_summary("no summary here"), None);
    }

    #[test]
    fn decodes_content_item_arrays() {
        let detail = "Tool: Bash\nID: x\n\nResult:\n[\n  {\n    \"type\": \"text\",\n    \"text\": \"test result: ok. 3 passed; 0 failed\\n\"\n  }\n]";
        let (_, output) = split_result_detail(detail);
        assert_eq!(parse_summary(&output), Some((3, 0)));
    }
}
