# agx-mcp

Model Context Protocol (MCP) server that exposes agx's session
introspection tools to AI agents. Read-only — lets an agent
self-monitor its own trace mid-run without writing anything back.

See also: [docs/mcp-integration.md](../../docs/mcp-integration.md)
for the full wiring guide and the agx ↔ sift composition story.

## What it's for

You're running Claude Code (or Cline, or Gemini CLI, or any
MCP-capable agent). The agent has been going for a while and you
want it to know things like:

- *"You've spent $0.40 and 5 tool calls failed. Consider stopping."*
- *"You've called `Read` 47 times. Are you in a loop?"*
- *"Your last Bash output leaked an AWS key. Redact before persisting."*
- *"You already looked at auth.rs in turn 3 — here's what you found."*

agx-mcp gives the agent tools to answer those questions itself.

## Tools

Every tool operates on the session file the server was launched
with (passed via `--session`). No arguments except where noted.

| Tool                      | Returns                                                                   |
|---------------------------|---------------------------------------------------------------------------|
| `agx_session_summary`     | Step count, tokens (in/out/cache), cost, unique models, error count       |
| `agx_recent_errors`       | Last N failed tool results (step_index, label, snippet). `limit` arg.     |
| `agx_tool_distribution`   | Per-tool use_count + error_count, sorted desc                             |
| `agx_scan_pii`            | All PII/credential matches across the session (category, step, snippet)   |
| `agx_search`              | Substring search over labels + details (step_index, label, preview)       |
| `agx_list_annotations`    | Human → agent messaging: notes the user left via `a` in the TUI           |

## Install

```sh
cargo install --path crates/agx-mcp
```

## Wire into Claude Code

Add to your project's `.mcp.json`:

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

Then the agent calls tools like:

```
<tool>agx_session_summary</tool>
→ {"step_count": 47, "tokens_in": 12000, "cost_usd": 0.04, ...}
```

## Wire into Cline / Gemini CLI

Same binary, different config file — each MCP host has its own
discovery path. See the host's MCP docs. The command + args above
work unchanged.

## Why read-only

v1 is read-only by design. Write tools (`agx_annotate_step`,
`agx_note_to_user`) have real coordination concerns across
multiple agents sharing the annotations file, and the user-facing
note channel needs a separate design pass. Read-only is the fast
path to useful — agents can self-budget and self-correct today.

Write tools are tracked in
[the roadmap](../../ROADMAP.md) (Phase 9 or Phase 8.5 depending on
where they land).

## Transport

JSON-RPC 2.0 over stdio, newline-delimited messages. MCP protocol
version `2025-03-26`. Compatible with every MCP host that speaks
stdio.

## License

Dual-licensed under MIT OR Apache-2.0.
