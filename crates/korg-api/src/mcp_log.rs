//! One INFO line per `/mcp` request, naming its caller (WI 4242).
//!
//! korg-gate stamps every request it forwards with `X-Homelab-Agent`, and
//! klams, kaed and karc all log their caller by that header. korg-api was the
//! one server that did not, so "is Duet reading korg right now, and which
//! tools" had no answer in kubsdb's own log — rmcp's lines prove a request
//! arrived and say nothing about who sent it.
//!
//! The line carries the header value (`-` when absent), the JSON-RPC method and,
//! for `tools/call`, the tool name. Reading the method means reading the body,
//! so the middleware buffers it and hands the same bytes on. That costs nothing
//! the transport was not already paying: rmcp parses the whole message before
//! dispatching it, so the body is buffered either way.
//!
//! Deliberately a log line and not a row: the 2936 activity log records what
//! changed, and who asked is a question this header cannot answer anyway —
//! every Claude session declares the same `claude`.

use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use serde_json::Value;

/// The header korg-gate (and every homelab agent client) stamps.
pub const AGENT_HEADER: &str = "x-homelab-agent";

/// One JSON-RPC message's loggable shape: its method, and the tool name when
/// the method is `tools/call`.
#[derive(Debug, PartialEq, Eq)]
pub struct Call {
    pub method: String,
    pub tool: Option<String>,
}

/// Every message in a request body, in order. A batch yields one entry per
/// element; a body that is not JSON-RPC yields none, which the caller logs as
/// `-` rather than guessing.
pub fn calls(body: &[u8]) -> Vec<Call> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items.iter().filter_map(call).collect(),
        one => call(&one).into_iter().collect(),
    }
}

fn call(msg: &Value) -> Option<Call> {
    let method = msg.get("method")?.as_str()?.to_owned();
    let tool = (method == "tools/call")
        .then(|| msg.pointer("/params/name").and_then(Value::as_str))
        .flatten()
        .map(str::to_owned);
    Some(Call { method, tool })
}

/// Render the calls for one log field: `tools/call:get_board`, comma-joined for
/// a batch, `-` for nothing parseable (a GET stream, a malformed body).
pub fn render(calls: &[Call]) -> String {
    if calls.is_empty() {
        return "-".to_owned();
    }
    calls
        .iter()
        .map(|c| match &c.tool {
            Some(tool) => format!("{}:{tool}", c.method),
            None => c.method.clone(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The middleware: log, then pass the request on with its body intact.
pub async fn log_caller(req: Request, next: Next) -> Response {
    let agent = req
        .headers()
        .get(AGENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-")
        .to_owned();

    if req.method() != Method::POST {
        tracing::info!(target: "korg_api::mcp", agent = %agent, http = %req.method(), rpc = "-", "mcp request");
        return next.run(req).await;
    }

    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            // The body could not be read at all; rmcp would have failed the same
            // way. Log the caller anyway and let the transport answer.
            tracing::info!(target: "korg_api::mcp", agent = %agent, http = "POST", rpc = "-", error = %e, "mcp request");
            return next.run(Request::from_parts(parts, Body::empty())).await;
        }
    };
    let rpc = render(&calls(&bytes));
    tracing::info!(target: "korg_api::mcp", agent = %agent, http = "POST", rpc = %rpc, "mcp request");
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_call_names_its_tool() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_board","arguments":{}}}"#;
        assert_eq!(render(&calls(body)), "tools/call:get_board");
    }

    #[test]
    fn other_methods_carry_no_tool() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"name":"ignored"}}"#;
        assert_eq!(render(&calls(body)), "tools/list");
        let note = br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert_eq!(render(&calls(note)), "notifications/initialized");
    }

    #[test]
    fn a_batch_is_listed_in_order() {
        let body = br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},
                        {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search"}}]"#;
        assert_eq!(render(&calls(body)), "initialize,tools/call:search");
    }

    #[test]
    fn nothing_parseable_renders_a_dash() {
        assert_eq!(render(&calls(b"not json")), "-");
        assert_eq!(render(&calls(br#"{"result":{}}"#)), "-");
        assert_eq!(render(&calls(b"")), "-");
    }
}
