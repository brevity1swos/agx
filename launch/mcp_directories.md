# MCP Registries / Directories — agx-mcp

The agx-only channel. `agx-mcp` is a genuine MCP (Model Context Protocol) server,
so it belongs in the MCP-server registries — a warm, on-topic audience where no
other terminal trace debugger is listed. **Highest signal-to-effort channel; do
this first.**

## What to list

- **Crate:** `agx-mcp` (`cargo install agx-mcp`)
- **Repo:** https://github.com/brevity1swos/agx (server lives in `crates/agx-mcp/`)
- **One-liner:** A read-only MCP server that lets an AI agent introspect its own
  session mid-run — session summary, recent errors, tool-call distribution, PII
  scan, search, and human annotations — by reading the trace file the agent CLI
  already writes.
- **Transport:** stdio (JSON-RPC 2.0), MCP 2025 spec.
- **Tools exposed:** `agx_session_summary`, `agx_recent_errors`,
  `agx_tool_distribution`, `agx_scan_pii`, `agx_search`, `agx_list_annotations`.

## Targets (open a PR / submit per each list's CONTRIBUTING)

| Registry | URL | Notes |
|----------|-----|-------|
| punkpeye/awesome-mcp-servers | github.com/punkpeye/awesome-mcp-servers | Largest list; has a "Developer Tools" / "Observability" section — agx-mcp fits both |
| wong2/awesome-mcp-servers | github.com/wong2/awesome-mcp-servers | Curated, smaller; PR-based |
| modelcontextprotocol/servers | github.com/modelcontextprotocol/servers | Official; community section accepts third-party servers |
| mcp.so / mcpservers.org | (web submit forms) | Aggregators; submit via their form |

## Suggested entry text (markdown list item)

```markdown
- [agx-mcp](https://github.com/brevity1swos/agx) — Read-only server that lets an
  agent introspect its **own** session mid-run (summary, recent errors,
  tool-call loops, PII scan) by parsing the trace file the CLI already writes.
  Works with Claude Code, Codex, Gemini, and 5 other formats. (Rust)
```

## Positioning note

Lead with the **self-introspection** framing — most MCP servers give the agent
new *external* capabilities (search, DB, files); agx-mcp is unusual in giving the
agent visibility into *itself* (am I looping? am I burning budget? did I already
try this?). That novelty is the hook in a crowded list. Pair it with the human →
agent annotation channel (`agx_list_annotations`: a human leaves a note in the
TUI, the agent reads it next turn) as the "human-in-the-loop" angle.
