# MCP integration guide

`agx-mcp` is an MCP (Model Context Protocol) server that exposes
agx's session introspection to AI agents running under
Claude Code / Cline / Gemini CLI / any other MCP-capable host.

This doc covers *what* to hook up, *why* an agent would call each
tool, and *how* agx composes with sift in a three-tool agentic
workflow.

## Why expose these tools to agents

Agents that run for hours without supervision drift. They retry
failing tool calls instead of backing off. They repeat file reads
they already did. They burn cost budgets silently. Users discover
all of this after the fact, in a dashboard.

Giving the agent read-only access to its own trace changes the
loop:

- Agent can self-budget: *"I've used 50k tokens and 8 tool calls
  failed. I should ask the user before continuing."*
- Agent can self-correct: *"I've called `Read` 47 times; I'm in a
  loop. Try a different approach."*
- Agent can self-redact: *"My last Bash output contained an AWS
  key. Strip it before suggesting a commit."*
- Agent can recall: *"Did I already look at `auth.rs`? Let me
  check my own trace before re-reading."*

None of this requires new Claude Code features — it works today
through the MCP tool interface.

## Tool surface (v1 — read-only)

All tools operate on the session file passed to `agx-mcp
--session <path>` at startup. No per-call session arg.

| Tool                      | Use case                                                                  |
|---------------------------|---------------------------------------------------------------------------|
| `agx_session_summary`     | Self-budget: "am I over my cost / error thresholds?"                      |
| `agx_recent_errors`       | Loop detection: "have I been failing on the same thing?"                  |
| `agx_tool_distribution`   | Stuck detection: "am I calling Read in a circle?"                         |
| `agx_scan_pii`            | Pre-commit guardrail: "is there a secret in my recent output?"            |
| `agx_search`              | Memory: "did I already look at this file earlier?"                        |

Every tool returns JSON strings the agent can parse. Schemas match
`docs/eval-integration.md` field names (Step, PII Match, etc.)
where applicable.

## Wiring into Claude Code

`.mcp.json` at the project root (or `~/.claude.json` for global):

```json
{
  "mcpServers": {
    "agx": {
      "command": "agx-mcp",
      "args": ["--session", "${CLAUDE_SESSION_FILE}"]
    }
  }
}
```

Claude Code substitutes `${CLAUDE_SESSION_FILE}` with the path to
the running session's JSONL. The agent then sees `agx_*` tools in
its tool list and can call them like any other MCP tool.

## Wiring into Cline / Gemini CLI / others

Same `agx-mcp --session <path>` command; each host has its own MCP
config file. The stdio protocol is the standard; transport
negotiation just works.

## Composition with sift

Installed alongside [sift](https://github.com/brevity1swos/sift),
agx-mcp fits into a three-layer self-monitoring + review flow:

### Pre-write (agent-side)

```
agent thinks about writing a file
    ↓
agent calls agx_scan_pii (via MCP)  — check own recent outputs
    ↓
agent calls agx_recent_errors       — have I been failing?
    ↓
agent decides: write / stop / ask user
```

The scan happens *before* the write reaches the file tree — sift
never sees redacted-away content.

### At write time (sift's domain)

Sift intercepts the write via its PreToolUse hook, applies policy,
records the pending entry. Independent of agx-mcp — sift doesn't
need to know agx ran.

### At review time

User opens `sift review`. Presses `t` on a pending entry. That
spawns `agx --jump-to <step> <session>` (agx Phase 5.5) so the
reviewer sees the full turn context. `sift review` and agx share
the same session file — no coordination needed beyond the file
path.

### Training-data loop (future)

Sift's ledger of accepted vs reverted writes, combined with agx's
`--export trajectory-dpo` (Phase 6.1 deferred) produces (chosen,
rejected) pairs for DPO training. agx-mcp doesn't participate
directly but the annotations an agent writes via `agx_annotate_step`
(future write tool) would flow into the SFT export.

## What v1 deliberately *doesn't* do

- **No write tools.** `agx_annotate_step` (agent self-reflection)
  and `agx_note_to_user` (out-of-band message to human) are
  valuable but have coordination concerns (multiple agents
  annotating the same session, note-queue semantics) that need a
  separate design pass.
- **No network.** agx-mcp is stdio-only. No remote agents, no
  sandboxed execution. If you need that, wrap this server in your
  own transport.
- **No sift-specific tools.** The composition is workflow-level,
  not API-level. sift ships its own MCP in due course; agx-mcp
  stays agx-focused.

## What to report upstream

If Claude Code's MCP client rejects our server, or a tool returns
wrong data, file an issue at
https://github.com/brevity1swos/agx with:

- Your MCP host version (Claude Code version, etc.).
- The exact JSON-RPC message sequence that broke (stdin/stdout
  captures from the host's logs).
- The agx-mcp version (`agx-mcp --version`).

## Stability

`agx-mcp`'s tool names + input schemas + output shapes follow the
same stability commitments as the main CLI (see
[`docs/stability.md`](stability.md)). Tool additions are MINOR;
renames or return-shape changes are MAJOR bumps.
