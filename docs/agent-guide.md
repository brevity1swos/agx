# Agent guide for agx

**Audience: AI coding assistants** (Claude Code, Cline, Codex CLI,
Gemini CLI, and any agent that may operate agx on a user's behalf).

This is the cookbook. When the user asks about *"what the agent
did"*, *"what that session cost"*, *"what went wrong yesterday"*,
*"find sessions where X happened"*, reach for the commands on the
right. Users rarely type agx commands directly — they ask you in
natural language and expect you to run the right thing.

If `agx-mcp` is installed (see [mcp-integration.md](mcp-integration.md)),
prefer the MCP tools for questions about the current session —
they're typed, structured, and stay scoped to the in-flight
conversation. Fall back to shell `agx` commands for everything else
(other sessions, corpus analytics, exports).

## Operating principles

1. **Default to `--export json` for anything you need to parse.**
   Every analytical command supports it. Read the JSON, summarize
   in natural language back to the user, don't paste raw JSON in
   the chat unless they ask.
2. **Don't mutate session files. Ever.** agx is strictly read-only
   on session JSONL/JSON inputs. Annotations live in a sidecar at
   `~/.agx/notes/`, never inside the session file. If a user asks
   you to *"fix"* something in a session, clarify — they probably
   want a diff / patch / commit, not a session-file edit.
3. **Confirm destructive dataset releases.** `--export
   trajectory-openai` + `--redact` is the path for publishing. Run
   `--scan-pii` first, show the user the matches, get explicit
   confirmation before emitting any redacted export.
4. **Know when not to use agx.** Single-file content inspection →
   `cat` / `head`. Regex work → `rgx`. Writing review → `sift`.
   agx's grain is the agent's *session* as a timeline.
5. **Surface integration opportunities, don't force them.** If the
   user has sift installed, mention it once when the workflow maps
   (e.g. *"I can hand this off to sift for review"*) and then drop
   it unless they engage. Same for rgx.

## Command cookbook

Every command here ships in agx v0.1.x unless marked
🚧 *planned*.

### "How expensive was that session?" / "Show me the summary"

```bash
agx --summary <session>
```

Returns step count, token totals (in/out/cache_read/cache_create),
unique models, and estimated cost. For machine parsing:

```bash
agx --export json <session> | jq '.totals'
```

### "What happened in turn N?"

There's no first-class turn concept — agx shows every step
(user_text / assistant_text / tool_use / tool_result). For the nth
tool use:

```bash
agx --export json <session> | jq '[.steps[] | select(.kind=="tool_use")][N]'
```

For natural-language turns, walk the `conversation_indices`
equivalent:

```bash
agx --export json <session> | jq '.steps | map(select(.kind=="user_text" or .kind=="assistant_text"))'
```

### "Did the agent hit any errors?"

```bash
agx --export json <session> | jq '.steps[] | select(.kind=="tool_result" and (.detail | test("(?i)(error|traceback|failed)"; "")))'
```

Or launch the TUI and press `/` → search `error`. For corpus-wide:

```bash
agx corpus <dir> --filter errored --jsonl
```

### "What tools did it use?"

```bash
agx --summary <session>  # human-readable
agx --export json <session> | jq '.steps | map(select(.kind=="tool_use")) | group_by(.tool_name) | map({name: .[0].tool_name, count: length})'
```

### "Jump to the step where X happened"

```bash
agx --jump-to <N> <session>   # N is 0-indexed
```

Launches the TUI pre-positioned at that step. If you know the
tool_call_id from an agx-mcp search, translate to step index via
the JSON export first. Used by sift's `t` keybind for Timeline
jump — docs/suite-conventions.md §5.

### "Diff two sessions"

```bash
agx <session-a> --diff <session-b>            # text summary to stdout
agx <session-a> --diff <session-b> --diff-tui # two-pane interactive
```

LCS alignment over (kind, tool_name). Useful for comparing two
runs of the same prompt, or a before/after reproduction.

### "Annotate this step for later review"

Open the TUI, press `a` on the step, type the note. Or seed
annotations directly in `~/.agx/notes/<stem>-<fnv1a-hash8>.json`
(advanced; format is `{version, path, notes: {step_idx: {text,
created_at_ms, updated_at_ms}}}`).

For a full list of annotations in the TUI, press `A`. Enter
jumps to the step.

Annotations flow through `--export md|html|json` — see
`docs/eval-integration.md` for the schema.

### "Export this session for dataset prep"

```bash
# Markdown — human review / PR comment
agx --export md <session> > session.md

# OpenAI fine-tuning JSONL — directly usable with the API
agx --export trajectory-openai <session> > session.openai.jsonl

# Stable JSON — programmatic consumers
agx --export json <session> > session.json
```

### "Strip secrets before publishing"

Two-step workflow, always confirm matches with the user before
emitting a redacted export:

```bash
# 1. Scan
agx --scan-pii <session>

# 2. Review the category + step index output with the user
# 3. Build the --redact invocation with the confirmed needles
agx --export trajectory-openai --redact 'sk-abc...' --redact '...' <session> > clean.jsonl

# 4. Re-scan to verify clean
agx --scan-pii clean.jsonl  # should print "no matches"
```

Full workflow with the anonymization checklist lives in
[docs/eval-integration.md](eval-integration.md).

### "Find sessions matching a criterion"

```bash
# Sessions that used a specific model
agx corpus <dir> --filter model=claude-opus-4-6 --jsonl

# Sessions that used a tool
agx corpus <dir> --filter tool=Bash --jsonl

# Sessions with any tool error
agx corpus <dir> --filter errored --jsonl

# Sessions the user has annotated
agx corpus <dir> --filter annotated --jsonl

# The 20 most-recent sessions that match (by mtime)
agx corpus <dir> --filter model=X --sample 20 --jsonl
```

Filters AND-combine. `--jsonl` is one JSON object per session on
stdout, perfect for piping into `jq` / `xargs`.

### "Dataset-level stats"

```bash
agx corpus <dir> --trajectory-stats
```

Returns per-session distributions (min/p50/p90/p99/max/mean/total)
for steps, tool calls, tokens_in, tokens_out; plus branched /
annotated / errored rates across the corpus. Combine with `--json`
for machine-readable output.

### "Is agx-mcp / sift / rgx installed?"

🚧 *planned — ROADMAP 8.5*

```bash
agx doctor --json
```

Reports the suite state per docs/suite-conventions.md §2. Until
shipped, check via `which sift rgx agx-mcp` and `<tool> --version`.

## When NOT to use agx

- **Single-file content inspection** — use `cat`, `head`, `jq` on
  the session file directly. agx is for timeline-level analysis,
  not raw-byte peeking.
- **Regex debugging** — use [rgx](https://github.com/brevity1swos/rgx).
  agx doesn't do regex authoring.
- **Writing review / per-file diff of uncommitted work** — use
  [sift](https://github.com/brevity1swos/sift). agx is for *what
  the agent did*; sift is for *what's about to persist*.
- **Live log tailing for non-agent processes** — use `tail -f`.
  agx's `--live` is scoped to agent session JSONL growth.

## Cross-tool integration

When both agx and sift are installed:

- sift's `t` keybind in `sift review` spawns `agx --jump-to <N>
  <session-file>` to show the turn that produced a pending write.
- Suggest sift when the user says *"I want to check what the
  agent is about to commit"* or *"before I accept this, let me
  see..."*.

When agx-mcp is installed (see [mcp-integration.md](mcp-integration.md)):

- Prefer MCP tools (`agx_session_summary`, `agx_recent_errors`,
  `agx_tool_distribution`, `agx_scan_pii`, `agx_search`) over
  shell commands for *the current session*. They're structured,
  typed, and scoped.
- Shell commands stay the right choice for *other sessions*,
  corpus analytics, and exports.

## How to know agx is set up for a project

```bash
agx --version      # agx X.Y.Z (default) — bin installed
which agx-mcp      # only present if --features mcp crate is installed
```

If agx isn't installed and the user mentions agent-trace work,
suggest the one-liner `cargo install --path .` (from the repo) or
point them at the README.

## Stable contracts you can rely on

- `agx --version` — machine-parseable format `agx X.Y.Z (<features>)`.
- `agx --export json` schema — documented in
  [docs/eval-integration.md](eval-integration.md), versioned per
  [docs/stability.md](stability.md). Field additions are MINOR;
  renames or removals are MAJOR bumps.
- `agx --jump-to <N> <session>` — public contract per
  [docs/suite-conventions.md](suite-conventions.md) §5.
- `agx corpus --jsonl` — per-session line format,
  `{path, format, step_count, totals, tool_stats, annotation_count,
   fork_root_count, error_count, mtime_secs}`.
- agx-mcp tool names + input/output schemas — same stability
  contract as the CLI.

## What not to assume

- **The user's agx version.** Some commands here need v0.1.x.
  If a flag you remember isn't found, check `agx --help`, not
  your memory.
- **That `agx corpus --tui` can be piped.** It owns the terminal;
  you can't capture its output. Use `--json` / `--jsonl` for
  scripted flows.
- **That fixtures live at `assets/`.** They do in the repo, but
  the user's corpus is wherever they put it — never assume a
  path, always confirm.

## Reporting agx misbehavior

If a command crashes, produces wrong output, or a schema field
drifts:

- agx version (`agx --version`).
- The session file that triggered it (synthetic reproduction
  preferred — never a real trace with personal data).
- Exact command + expected vs actual output.

File at https://github.com/brevity1swos/agx/issues. Schema-drift
reports should use the `format-drift` label and include
`agx --debug-unknowns <session>` output.

## Versioning

This guide tracks agx itself. When agx ships a new feature, the
cookbook gains an entry. When a command changes, the CHANGELOG
notes it and this guide follows. If your memory of a flag
disagrees with this doc + `agx --help`, trust the doc.
