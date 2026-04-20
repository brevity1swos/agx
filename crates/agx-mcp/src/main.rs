//! agx-mcp — MCP (Model Context Protocol) server exposing agx's
//! session introspection tools to AI agents.
//!
//! Read-only: agents can query their own session for cost, errors,
//! tool distribution, and PII matches — without disturbing the
//! session file or coordinating with other running agx processes.
//!
//! # Transport
//!
//! JSON-RPC 2.0 over stdio, one message per line. This matches the
//! MCP 2025 spec and works with Claude Code, Cline, Gemini CLI, and
//! any other MCP-capable host.
//!
//! # Wiring it up
//!
//! Claude Code `.mcp.json`:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "agx": {
//!       "command": "agx-mcp",
//!       "args": ["--session", "${CLAUDE_SESSION_FILE}"]
//!     }
//!   }
//! }
//! ```
//!
//! See `docs/mcp-integration.md` for the full guide.

use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "agx-mcp",
    version,
    about = "MCP server for agx session introspection"
)]
struct Cli {
    /// Path to the session file the agent is currently running.
    /// Every tool call operates on this session. Pass the
    /// Claude Code / Codex / Gemini session path here.
    #[arg(long, value_name = "PATH")]
    session: PathBuf,
}

/// Minimal JSON-RPC 2.0 request shape — we only need method + id + params.
#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

/// JSON-RPC 2.0 response shape. Only `result` or `error` is set, never both.
#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

fn ok(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn err(id: Value, code: i32, message: impl Into<String>) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
        }),
    }
}

/// Tool descriptors returned by `tools/list`. Kept as a static shape
/// so the server doesn't re-derive them per call. Input schemas are
/// JSON Schema objects; every tool here takes no arguments because
/// the session path is set at startup via `--session`.
fn tool_list() -> Value {
    json!([
        {
            "name": "agx_session_summary",
            "description": "Summary of the current session: step count, tokens (in/out/cache), cost, unique models, error count. Use this to self-budget and detect anomalies mid-run.",
            "inputSchema": {"type": "object", "properties": {}, "required": []}
        },
        {
            "name": "agx_recent_errors",
            "description": "Last N tool_result steps that matched agx's is_error_result heuristic, with step index and a short snippet of the error output. Use to detect retry loops or escalating failures.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {"type": "integer", "description": "Max matches to return (default 5)"}
                },
                "required": []
            }
        },
        {
            "name": "agx_tool_distribution",
            "description": "Count of tool uses and errors per tool name in the current session, sorted by use count descending. Use to detect tool-call loops (e.g. Read called 47 times).",
            "inputSchema": {"type": "object", "properties": {}, "required": []}
        },
        {
            "name": "agx_scan_pii",
            "description": "Run agx's PII / credential scanner over every step's detail + label in the current session. Returns a list of matches with category, step_index, and snippet. Use to redact before persisting or committing.",
            "inputSchema": {"type": "object", "properties": {}, "required": []}
        },
        {
            "name": "agx_search",
            "description": "Case-insensitive substring search over every step's label + detail. Returns matching step indices with a preview. Use to answer 'did I already try this?' without re-scanning the full trace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Substring to search for"},
                    "limit": {"type": "integer", "description": "Max matches (default 20)"}
                },
                "required": ["query"]
            }
        }
    ])
}

fn handle_initialize(id: Value) -> Response {
    ok(
        id,
        json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": {"listChanged": false}
            },
            "serverInfo": {
                "name": "agx-mcp",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(id: Value) -> Response {
    ok(id, json!({"tools": tool_list()}))
}

fn handle_tools_call(id: Value, params: Value, session: &std::path::Path) -> Response {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    match run_tool(name, &args, session) {
        Ok(output) => ok(
            id,
            json!({
                "content": [
                    {"type": "text", "text": output}
                ]
            }),
        ),
        Err(e) => err(id, -32603, format!("tool `{name}` failed: {e}")),
    }
}

/// Dispatch for a named tool. Each returns a JSON-stringified result
/// that the agent parses on its side. Rendering policy intentionally
/// leaves formatting decisions to the caller — the server only
/// produces structured data.
fn run_tool(name: &str, args: &Value, session: &std::path::Path) -> Result<String> {
    let steps = agx_core::loader::load_session(session)
        .with_context(|| format!("loading session {}", session.display()))?;
    match name {
        "agx_session_summary" => {
            let totals = agx_core::timeline::compute_session_totals(&steps);
            let tool_stats = agx_core::timeline::compute_tool_stats(&steps);
            let error_count: usize = tool_stats.iter().map(|t| t.error_count).sum();
            Ok(serde_json::to_string(&json!({
                "step_count": steps.len(),
                "tokens_in": totals.tokens_in,
                "tokens_out": totals.tokens_out,
                "cache_read": totals.cache_read,
                "cache_create": totals.cache_create,
                "cost_usd": totals.cost_usd,
                "unique_models": totals.unique_models,
                "error_count": error_count,
            }))?)
        }
        "agx_recent_errors" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
            let errors: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, s)| agx_core::timeline::is_error_result(s))
                .rev()
                .take(limit)
                .map(|(i, s)| {
                    json!({
                        "step_index": i,
                        "label": s.label,
                        "snippet": agx_core::timeline::truncate(&s.detail, 200),
                    })
                })
                .collect();
            Ok(serde_json::to_string(&errors)?)
        }
        "agx_tool_distribution" => {
            let tool_stats = agx_core::timeline::compute_tool_stats(&steps);
            let out: Vec<_> = tool_stats
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "use_count": t.use_count,
                        "error_count": t.error_count,
                    })
                })
                .collect();
            Ok(serde_json::to_string(&out)?)
        }
        "agx_scan_pii" => {
            let matches = agx_core::pii::scan_steps(&steps);
            let out: Vec<_> = matches
                .iter()
                .map(|m| {
                    json!({
                        "category": m.category.label(),
                        "step_index": m.step_index,
                        "snippet": m.snippet,
                    })
                })
                .collect();
            Ok(serde_json::to_string(&out)?)
        }
        "agx_search" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let needle = query.to_lowercase();
            let matches: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.label.to_lowercase().contains(&needle)
                        || s.detail.to_lowercase().contains(&needle)
                })
                .take(limit)
                .map(|(i, s)| {
                    json!({
                        "step_index": i,
                        "label": s.label,
                        "preview": agx_core::timeline::truncate(&s.detail, 120),
                    })
                })
                .collect();
            Ok(serde_json::to_string(&matches)?)
        }
        other => anyhow::bail!("unknown tool `{other}`"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                // Protocol-level parse error. MCP says report as a
                // JSON-RPC error with id=null when we can't read the id.
                let resp = err(Value::Null, -32700, format!("parse error: {e}"));
                writeln!(out, "{}", serde_json::to_string(&resp)?)?;
                out.flush()?;
                continue;
            }
        };

        // Notifications (no id) are one-way; MCP's
        // `notifications/initialized` is the common case. Don't reply.
        let is_notification = req.id.is_none();
        if is_notification {
            continue;
        }
        let id = req.id.unwrap_or(Value::Null);

        let resp = match req.method.as_str() {
            "initialize" => handle_initialize(id),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, req.params, &cli.session),
            other => err(id, -32601, format!("unknown method `{other}`")),
        };
        writeln!(out, "{}", serde_json::to_string(&resp)?)?;
        out.flush()?;
    }
    Ok(())
}
