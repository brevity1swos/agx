---
name: agx
description: Inspect AI coding-agent session transcripts (Claude Code, Codex CLI, Gemini CLI, OpenAI-compatible, LangChain, Vercel AI SDK, OpenTelemetry GenAI) with the agx CLI. Use when the user asks what an agent did in a session, what a session cost in tokens or dollars, which tools it called, where it hit errors, what its subagents did and whether their "tests pass" reports hold up, how two runs differ, which past sessions match a criterion, or how to export or redact a session for sharing or a dataset.
---

# agx

agx reads agent session files and turns them into a timeline of steps:
`user_text`, `assistant_text`, `tool_use`, `tool_result`. It is read-only on
session files and needs no instrumentation. Users rarely type agx commands
themselves; they ask in plain language and expect you to run the right one.

## Rules

1. **Parse JSON, answer in prose.** Use `agx --export json` (or `--jsonl` /
   `--json` on `corpus`) for anything you need to compute, then summarize.
   Don't paste raw JSON unless the user asks for it.
2. **Never edit session files.** If the user wants to "fix" something in a
   session, they almost always want a code change, not a transcript edit.
   Ask.
3. **Confirm before any redacted export.** Run `--scan-pii`, show the user
   the matches, and get explicit agreement on what to redact before writing
   an export meant for sharing.
4. **Check before assuming a flag exists.** If a command fails, run
   `agx --help` or `agx <subcommand> --help` rather than retrying from memory.
5. **Ask before installing.** If `agx` isn't on PATH, tell the user it
   installs with `cargo install agx-tui` (the binary is `agx`); don't install
   it yourself unless they say so.

## Finding the session

| Agent | Where sessions live |
|---|---|
| Claude Code | `~/.claude/projects/<project-dir>/<session-id>.jsonl` |
| Codex CLI | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` |
| Gemini CLI | `~/.gemini/tmp/<project>/chats/session-*.json` |

The project directory is the working directory with separators replaced by
`-`. Recent Claude Code sessions: `ls -t ~/.claude/projects/*/*.jsonl | head`.
The format is detected from content, so any of these files (or an exported
trace) can be passed directly. Never guess a path; confirm with the user when
more than one candidate fits.

## Recipes

**Cost and overview**
```bash
agx --summary <session>                       # steps, tokens, models, est. cost
agx --export json <session> | jq '.totals'    # same totals, machine-readable
```

**Which tools it used**
```bash
agx --export json <session> \
  | jq '.steps | map(select(.kind=="tool_use")) | group_by(.tool_name)
        | map({tool: .[0].tool_name, count: length})'
```

**Errors.** Tool results that look like failures:
```bash
agx --export json <session> \
  | jq '[.steps | to_entries[]
         | select(.value.kind=="tool_result"
                  and (.value.detail | test("(?i)error|traceback|failed")))
         | {step: .key, label: .value.label}]'
```
This is a text heuristic; read the matching steps before reporting them as
real failures.

**The nth tool call, or the conversation only**
```bash
agx --export json <session> | jq '[.steps[] | select(.kind=="tool_use")][N]'
agx --export json <session> \
  | jq '.steps | map(select(.kind=="user_text" or .kind=="assistant_text"))'
```
Step indices are 0-based and count every step, not just turns.

**Open the TUI at a step** (for the user to look at, not for you to parse):
```bash
agx --jump-to <N> <session>
```

**Compare two runs** (alignment over step kind and tool name):
```bash
agx <session-a> --diff <session-b>
```

**Subagents of a Claude Code session**, and whether their "tests pass"
reports hold up:
```bash
agx subagents <session>                       # tree for the user
agx subagents --json <session> \
  | jq '.subagents[] | {agent_id, description, spawn_step,
                        verdict: .claim_check.verdict}'
```
`claim_check` compares a report's test claim with the subagent's own
last test run. Verdicts: `supported`, `contradicted` (last run
failed), `stale` (code edited after the run), `unverified` (no run
found, a run that executed no tests, or a name-filtered run behind an
"all tests pass" claim). Raise anything but `supported`, and present `supported` as
"the transcript backs the claim", not as proof the tests are good.

**Slice a long session** before exporting or summarizing:
`--after 10m`, `--before 1h`, `--after-step 100`, `--before-step 200`, or
`--range 100..200`.

**Search across many sessions**
```bash
agx corpus <dir> --filter model=<name> --jsonl   # one JSON object per session
agx corpus <dir> --filter tool=Bash --jsonl
agx corpus <dir> --filter errored --jsonl
agx corpus <dir> --filter annotated --jsonl      # sessions with user notes
agx corpus <dir> --filter errored --sample 20 --jsonl   # 20 newest matches
agx corpus <dir> --trajectory-stats --json       # p50/p90/p99 of steps, tokens
```
Filters combine with AND. Each `--jsonl` line has `path`, `format`,
`step_count`, `tokens_in`, `tokens_out`, `cache_read`, `cache_create`,
`cost_usd`, `models`, `tool_counts`, `error_count`, `annotation_count`,
`fork_root_count`, `mtime_secs`. `corpus --tui` owns the terminal; never try
to capture its output.

**Export**
```bash
agx --export md <session> > session.md                    # human review
agx --export json <session> > session.json                # stable schema
agx --export trajectory-openai <session> > session.jsonl  # fine-tuning format
```

**Redact before sharing.** Always in this order, with the user:
```bash
agx --scan-pii <session>          # 1. report candidate secrets by category
# 2. show the matches; agree on the exact strings to mask
agx --export trajectory-openai --redact '<string>' --redact '<string>' \
  <session> > clean.jsonl         # 3. export with every agreed string masked
agx --scan-pii clean.jsonl        # 4. expect "no matches"
```
`--scan-pii` is heuristic. A clean re-scan is not proof that nothing
sensitive remains; say so.

## Current session over MCP

If `agx-mcp` is installed (`cargo install agx-mcp`) and registered with the
agent, prefer its tools for questions about the session in progress:
`agx_session_summary`, `agx_recent_errors`, `agx_tool_distribution`,
`agx_search`, `agx_scan_pii`, `agx_list_annotations`. Use the CLI for other
sessions, corpus questions, and exports.

## Check the setup

```bash
agx --version       # prints "agx X.Y.Z"
agx doctor --json   # agx version and features; agx-mcp, sift, rgx presence
```
`doctor` always exits 0; a missing sibling is information, not an error.

## When not to use agx

- Reading raw bytes of a session file: use `jq` / `head` directly.
- Authoring or debugging a regex: use `rgx`.
- Reviewing or reverting the files an agent wrote, per turn: use `sift`.
  Mention it once if it fits, then drop it unless the user engages.
- Tailing logs of non-agent processes: use `tail -f`.

## Reference

Stable contracts: the `--export json` schema (`{totals, steps,
annotations?}`), `--jump-to <N>`, the `corpus --jsonl` line, and the agx-mcp
tool schemas. Additions are minor versions; renames and removals are breaking
(see `docs/stability.md`). Bugs and format drift go to
https://github.com/brevity1swos/agx/issues with `agx --version`, the exact
command, and a synthetic reproduction. Never attach a real session file.
