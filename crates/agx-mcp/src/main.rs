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
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use agx_core::timeline::Step;

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
        },
        {
            "name": "agx_list_annotations",
            "description": "List every annotation the human has written on steps in the current session. Returns {step_index, text, created_at_ms, updated_at_ms} per note. This is the human → agent messaging channel — users press `a` in the agx TUI to leave guidance ('this assumption is wrong', 'revisit after you finish X'); agents call this to pick that guidance up across turns / sessions.",
            "inputSchema": {"type": "object", "properties": {}, "required": []}
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

/// Error shape split per the MCP spec: a malformed call (unknown tool,
/// bad arguments) is a protocol error (-32602), while a well-formed
/// call that fails while running (session unreadable, …) is a
/// *successful* JSON-RPC result flagged `isError: true` so the model
/// sees the message and can react to it.
fn handle_tools_call(id: Value, params: Value, session: &std::path::Path) -> Response {
    let tool = match parse_tool_call(&params) {
        Ok(t) => t,
        Err(msg) => return err(id, -32602, format!("invalid params: {msg}")),
    };
    match run_tool(&tool, session) {
        Ok(output) => ok(
            id,
            json!({
                "content": [
                    {"type": "text", "text": output}
                ]
            }),
        ),
        Err(e) => ok(
            id,
            json!({
                "content": [
                    {"type": "text", "text": format!("tool `{}` failed: {e:#}", tool.name())}
                ],
                "isError": true
            }),
        ),
    }
}

/// A validated `tools/call` request. Built before the session is
/// touched so bad calls are rejected without paying for a parse.
#[derive(Debug)]
enum Tool {
    SessionSummary,
    RecentErrors { limit: usize },
    Distribution,
    ScanPii,
    Search { query: String, limit: usize },
    ListAnnotations,
}

impl Tool {
    fn name(&self) -> &'static str {
        match self {
            Tool::SessionSummary => "agx_session_summary",
            Tool::RecentErrors { .. } => "agx_recent_errors",
            Tool::Distribution => "agx_tool_distribution",
            Tool::ScanPii => "agx_scan_pii",
            Tool::Search { .. } => "agx_search",
            Tool::ListAnnotations => "agx_list_annotations",
        }
    }
}

fn parse_tool_call(params: &Value) -> std::result::Result<Tool, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing string field `name`")?;
    let empty = serde_json::Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return Err("`arguments` must be an object".into()),
    };
    let limit = |default: usize| -> std::result::Result<usize, String> {
        match args.get("limit") {
            None | Some(Value::Null) => Ok(default),
            Some(v) => v
                .as_u64()
                .map(|n| usize::try_from(n).unwrap_or(usize::MAX))
                .ok_or_else(|| "`limit` must be a non-negative integer".into()),
        }
    };
    Ok(match name {
        "agx_session_summary" => Tool::SessionSummary,
        "agx_recent_errors" => Tool::RecentErrors { limit: limit(5)? },
        "agx_tool_distribution" => Tool::Distribution,
        "agx_scan_pii" => Tool::ScanPii,
        "agx_search" => Tool::Search {
            query: args
                .get("query")
                .and_then(Value::as_str)
                .ok_or("missing string argument `query`")?
                .to_string(),
            limit: limit(20)?,
        },
        "agx_list_annotations" => Tool::ListAnnotations,
        other => return Err(format!("unknown tool `{other}`")),
    })
}

/// Parse-once cache. MCP clients commonly fire 2–6 tool calls in
/// quick succession (summary → errors → distribution → scan_pii →
/// search …); re-parsing a 5 MB JSONL each time would make the
/// agent-side latency scale linearly with call count for no reason.
/// The cache is keyed on `(session_path, mtime)` so re-parsing
/// kicks in automatically when the session grows (the common case
/// during a live Claude Code run).
struct StepCache {
    mtime: SystemTime,
    steps: Vec<Step>,
}
static CACHE: std::sync::Mutex<Option<StepCache>> = std::sync::Mutex::new(None);

fn load_cached(session: &std::path::Path) -> Result<Vec<Step>> {
    let current_mtime = std::fs::metadata(session)
        .with_context(|| format!("stat {}", session.display()))?
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH);
    {
        // A poisoned lock only means a previous holder panicked; the
        // cached value is still a complete `Option<StepCache>`, so
        // recover it instead of crashing the long-lived server.
        let guard = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(cache) = guard.as_ref()
            && cache.mtime == current_mtime
        {
            return Ok(cache.steps.clone());
        }
    }
    // Cache miss — parse and install. Hold the lock only while
    // updating the cache; the parse happens without the lock so a
    // slow parse doesn't stall a concurrent reader.
    let steps = agx_core::loader::load_session(session)
        .with_context(|| format!("loading session {}", session.display()))?;
    *CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(StepCache {
        mtime: current_mtime,
        steps: steps.clone(),
    });
    Ok(steps)
}

/// Dispatch for a named tool. Each returns a JSON-stringified result
/// that the agent parses on its side. Rendering policy intentionally
/// leaves formatting decisions to the caller — the server only
/// produces structured data.
fn run_tool(tool: &Tool, session: &std::path::Path) -> Result<String> {
    let steps = load_cached(session)?;
    match tool {
        Tool::SessionSummary => {
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
        Tool::RecentErrors { limit } => {
            let errors: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, s)| agx_core::timeline::is_error_result(s))
                .rev()
                .take(*limit)
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
        Tool::Distribution => {
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
        Tool::ScanPii => {
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
        Tool::Search { query, limit } => {
            let needle = query.to_lowercase();
            let matches: Vec<_> = steps
                .iter()
                .enumerate()
                .filter(|(_, s)| {
                    s.label.to_lowercase().contains(&needle)
                        || s.detail.to_lowercase().contains(&needle)
                })
                .take(*limit)
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
        Tool::ListAnnotations => {
            // Load notes via the same sidecar the TUI uses. Fault-
            // tolerant: a missing file returns an empty set
            // (Annotations::load_for handles the missing case by
            // constructing a fresh empty annotations struct).
            let anns = agx_core::annotations::Annotations::load_for(session);
            let notes: Vec<_> = anns
                .iter()
                .map(|(idx, note)| {
                    json!({
                        "step_index": idx,
                        "text": note.text,
                        "created_at_ms": note.created_at_ms,
                        "updated_at_ms": note.updated_at_ms,
                    })
                })
                .collect();
            Ok(serde_json::to_string(&notes)?)
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if let Some(resp) = handle_line(&line, &cli.session) {
            writeln!(out, "{}", serde_json::to_string(&resp)?)?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Process one line of stdin. Returns `None` when no reply is due:
/// blank lines and notifications.
///
/// The request is inspected as a raw `Value` rather than deserialized
/// into a struct with `id: Option<Value>`, because serde collapses an
/// explicit `"id": null` into `None` — and JSON-RPC treats an absent
/// id (notification, no reply) differently from a present-null id
/// (request, reply with id null).
fn handle_line(line: &str, session: &std::path::Path) -> Option<Response> {
    if line.trim().is_empty() {
        return None;
    }
    let value: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        // We can't read the id of unparseable input, so per JSON-RPC
        // the error goes back with id=null.
        Err(e) => return Some(err(Value::Null, -32700, format!("parse error: {e}"))),
    };
    let Some(obj) = value.as_object() else {
        return Some(err(
            Value::Null,
            -32600,
            "invalid request: expected a JSON object",
        ));
    };

    let id = obj.get("id");
    let id_ok = matches!(
        id,
        None | Some(Value::Null | Value::String(_) | Value::Number(_))
    );
    let method = obj.get("method").and_then(Value::as_str);
    let version_ok = obj.get("jsonrpc").and_then(Value::as_str) == Some("2.0");
    let (Some(method), true, true) = (method, id_ok, version_ok) else {
        // Echo the id back only when it is itself well-formed.
        let reply_id = id.filter(|_| id_ok).cloned().unwrap_or(Value::Null);
        return Some(err(
            reply_id,
            -32600,
            "invalid request: need jsonrpc \"2.0\", a string method, and a string/number/null id",
        ));
    };

    // Notifications (no id key at all) are one-way; MCP's
    // `notifications/initialized` is the common case. Don't reply.
    let id = id?.clone();
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    Some(match method {
        "initialize" => handle_initialize(id),
        // MCP liveness check: the spec mandates an empty result.
        "ping" => ok(id, json!({})),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, params, session),
        other => err(id, -32601, format!("unknown method `{other}`")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/sample_session.jsonl")
    }

    #[test]
    fn load_cached_survives_poisoned_mutex() {
        // A panic while holding the lock poisons it; the server must
        // keep serving rather than crash on every later tool call.
        let _ = std::thread::spawn(|| {
            let _guard = CACHE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            panic!("poison the cache");
        })
        .join();
        assert!(CACHE.is_poisoned());
        let first = load_cached(&fixture()).expect("first load");
        let second = load_cached(&fixture()).expect("cached load");
        assert_eq!(first.len(), second.len());
        assert!(!first.is_empty());
    }

    /// Run one line through the server and return the reply as JSON.
    fn reply(line: &str) -> Option<Value> {
        handle_line(line, &fixture()).map(|r| serde_json::to_value(r).unwrap())
    }

    fn error_code(v: &Value) -> i64 {
        v["error"]["code"].as_i64().expect("error code")
    }

    #[test]
    fn request_with_null_id_gets_a_reply() {
        let r = reply(r#"{"jsonrpc":"2.0","id":null,"method":"tools/list"}"#)
            .expect("explicit null id is a request, not a notification");
        assert_eq!(r["id"], Value::Null);
        assert!(r["result"]["tools"].is_array());
    }

    #[test]
    fn notification_without_id_gets_no_reply() {
        assert!(reply(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
    }

    #[test]
    fn blank_line_gets_no_reply() {
        assert!(reply("   ").is_none());
    }

    #[test]
    fn invalid_json_is_parse_error_with_null_id() {
        let r = reply("{not json").expect("reply");
        assert_eq!(error_code(&r), -32700);
        assert_eq!(r["id"], Value::Null);
    }

    #[test]
    fn non_object_json_is_invalid_request() {
        let r = reply("42").expect("reply");
        assert_eq!(error_code(&r), -32600);
        assert_eq!(r["id"], Value::Null);
    }

    #[test]
    fn missing_method_is_invalid_request_echoing_id() {
        let r = reply(r#"{"jsonrpc":"2.0","id":7}"#).expect("reply");
        assert_eq!(error_code(&r), -32600);
        assert_eq!(r["id"], json!(7));
    }

    #[test]
    fn wrong_jsonrpc_version_is_invalid_request() {
        let r = reply(r#"{"jsonrpc":"1.0","id":"a","method":"tools/list"}"#).expect("reply");
        assert_eq!(error_code(&r), -32600);
        assert_eq!(r["id"], json!("a"));
    }

    #[test]
    fn ping_returns_empty_result() {
        let r = reply(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#).expect("reply");
        assert_eq!(r["result"], json!({}));
        assert!(r.get("error").is_none());
    }

    fn call(params: Value, session: &std::path::Path) -> Value {
        serde_json::to_value(handle_tools_call(json!(1), params, session)).unwrap()
    }

    fn missing_session() -> PathBuf {
        PathBuf::from("/nonexistent/agx-mcp-test/session.jsonl")
    }

    #[test]
    fn unknown_tool_is_invalid_params_without_loading_session() {
        // The session path doesn't exist: if the server tried to load
        // it first we'd see a tool execution error instead.
        let r = call(json!({"name": "agx_nope"}), &missing_session());
        assert_eq!(error_code(&r), -32602);
    }

    #[test]
    fn missing_tool_name_is_invalid_params() {
        let r = call(json!({"arguments": {}}), &fixture());
        assert_eq!(error_code(&r), -32602);
    }

    #[test]
    fn non_object_arguments_is_invalid_params() {
        let r = call(
            json!({"name": "agx_session_summary", "arguments": [1]}),
            &fixture(),
        );
        assert_eq!(error_code(&r), -32602);
    }

    #[test]
    fn search_without_query_is_invalid_params() {
        let r = call(json!({"name": "agx_search", "arguments": {}}), &fixture());
        assert_eq!(error_code(&r), -32602);
    }

    #[test]
    fn non_integer_limit_is_invalid_params() {
        let r = call(
            json!({"name": "agx_recent_errors", "arguments": {"limit": "five"}}),
            &fixture(),
        );
        assert_eq!(error_code(&r), -32602);
    }

    #[test]
    fn session_load_failure_is_tool_error_result() {
        let r = call(json!({"name": "agx_session_summary"}), &missing_session());
        assert!(
            r.get("error").is_none(),
            "execution failure is not an RPC error"
        );
        assert_eq!(r["result"]["isError"], json!(true));
        assert_eq!(r["result"]["content"][0]["type"], json!("text"));
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|t| !t.is_empty())
        );
    }

    #[test]
    fn successful_tool_call_is_not_flagged_as_error() {
        let r = call(
            json!({"name": "agx_search", "arguments": {"query": "a", "limit": 2}}),
            &fixture(),
        );
        assert!(r.get("error").is_none());
        assert_ne!(r["result"]["isError"], json!(true));
        let text = r["result"]["content"][0]["text"].as_str().expect("text");
        let matches: Value = serde_json::from_str(text).expect("json payload");
        assert!(matches.as_array().is_some_and(|m| m.len() <= 2));
    }

    #[test]
    fn unknown_method_is_method_not_found() {
        let r = reply(r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#).expect("reply");
        assert_eq!(error_code(&r), -32601);
    }
}
