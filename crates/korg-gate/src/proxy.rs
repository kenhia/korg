//! The resource-server half: `/mcp`, authenticated, filtered, forwarded.
//!
//! korg-api's `/mcp` is rmcp streamable-HTTP. Today it runs stateless with JSON
//! responses, but the proxy does not lean on that: `Mcp-Session-Id` passes in
//! both directions, `GET` (the SSE stream) and `DELETE` (session end) are
//! forwarded as well as `POST`, and a `tools/list` answer is filtered whether
//! it comes back as JSON or as an SSE stream.
//!
//! What is forwarded is the gate's *re-serialisation* of the message it
//! checked, never the caller's raw bytes. Duplicate JSON keys are the reason: a
//! parser that keeps the first `name` and one that keeps the last would
//! otherwise disagree about which tool is being called.

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::allowlist::{method_allowed, tool_allowed};
use crate::config::{AGENT_HEADER, AGENT_NAME};
use crate::AppState;

const SESSION: &str = "mcp-session-id";
/// Request headers worth carrying upstream. Everything else — notably the
/// caller's `Authorization`, which is a *gate* token korg has no use for, and
/// any cookies — stays here.
const FORWARD_REQUEST: &[&str] = &[
    "content-type",
    "accept",
    SESSION,
    "mcp-protocol-version",
    "last-event-id",
];
const FORWARD_RESPONSE: &[&str] = &["content-type", SESSION, "cache-control"];
/// The one upper bound on a buffered `tools/list` answer; korg's is ~100 KB.
const MAX_FILTERED_BODY: usize = 8 * 1024 * 1024;

/// The 401 that starts Claude's sign-in: exactly the contract's shape.
fn unauthorized(app: &AppState) -> Response {
    let value = format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\"",
        app.config.issuer()
    );
    let mut resp = (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "invalid_token" })),
    )
        .into_response();
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&value).expect("issuer is a valid header value"),
    );
    resp
}

fn authenticate(app: &AppState, headers: &HeaderMap) -> Result<String, Box<Response>> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            let (scheme, token) = v.split_once(' ')?;
            scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
        })
        .and_then(|token| app.store.authenticate(token))
        .ok_or_else(|| Box::new(unauthorized(app)))
}

fn rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A refused *request* (it has an id) is a JSON-RPC error on a 200, which is
/// what an MCP client expects to read. A refused notification or response has
/// no id to answer, so it is an HTTP 400 carrying an id-less error (the
/// streamable-HTTP transport's rule for input the server cannot accept).
fn refuse(id: Option<Value>, code: i64, message: String) -> Response {
    match id {
        Some(id) => Json(rpc_error(id, code, message)).into_response(),
        None => (
            StatusCode::BAD_REQUEST,
            Json(rpc_error(Value::Null, code, message)),
        )
            .into_response(),
    }
}

pub async fn post_mcp(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let client = match authenticate(&app, &headers) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let message: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return refuse(None, -32700, "parse error".into()),
    };
    let Value::Object(obj) = &message else {
        // Batches were removed from MCP in 2025-06-18; deny rather than vet N.
        return refuse(
            None,
            -32600,
            "korg-gate accepts one JSON-RPC message per request".into(),
        );
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return refuse(
            id,
            -32600,
            "korg-gate forwards requests and notifications only".into(),
        );
    };
    if !method_allowed(method) {
        tracing::warn!(client = %&client[..8.min(client.len())], method, "method refused");
        return refuse(
            id,
            -32601,
            format!("method `{method}` is not available through korg-gate"),
        );
    }
    if method == "tools/call" {
        let tool = obj
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !tool_allowed(tool) {
            tracing::warn!(client = %&client[..8.min(client.len())], tool, "tool refused");
            return refuse(
                id,
                -32602,
                format!("tool `{tool}` is not available through korg-gate, which exposes korg read-only"),
            );
        }
        tracing::info!(client = %&client[..8.min(client.len())], tool, "tools/call");
    }
    let filter_tools = method == "tools/list" && id.is_some();
    let forwarded = serde_json::to_vec(&message).expect("a parsed Value re-serialises");

    let upstream = match send(&app, Method::POST, &headers, Some(forwarded)).await {
        Ok(r) => r,
        Err(r) => return with_id(*r, id),
    };
    if filter_tools && upstream.status().is_success() {
        return filtered_tools_list(upstream, id.unwrap_or(Value::Null)).await;
    }
    stream_back(upstream)
}

pub async fn get_mcp(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = authenticate(&app, &headers) {
        return *r;
    }
    match send(&app, Method::GET, &headers, None).await {
        Ok(up) => stream_back(up),
        Err(r) => *r,
    }
}

pub async fn delete_mcp(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = authenticate(&app, &headers) {
        return *r;
    }
    match send(&app, Method::DELETE, &headers, None).await {
        Ok(up) => stream_back(up),
        Err(r) => *r,
    }
}

async fn send(
    app: &AppState,
    method: Method,
    headers: &HeaderMap,
    body: Option<Vec<u8>>,
) -> Result<reqwest::Response, Box<Response>> {
    let mut req = app.http.request(method, app.config.upstream.clone());
    for name in FORWARD_REQUEST {
        if let Some(v) = headers.get(*name) {
            req = req.header(*name, v);
        }
    }
    req = req.header(AGENT_HEADER, AGENT_NAME);
    if let Some(body) = body {
        req = req.body(body);
    }
    req.send().await.map_err(|e| {
        tracing::error!(error = %e, "upstream unreachable");
        Box::new((StatusCode::BAD_GATEWAY, "korg is unreachable from the gate").into_response())
    })
}

/// Turn a bare 502 into a JSON-RPC error when there is a request id to answer.
fn with_id(resp: Response, id: Option<Value>) -> Response {
    match id {
        Some(id) => (
            resp.status(),
            Json(rpc_error(
                id,
                -32603,
                "korg is unreachable from the gate".into(),
            )),
        )
            .into_response(),
        None => resp,
    }
}

fn copy_headers(from: &reqwest::header::HeaderMap, to: &mut HeaderMap) {
    for name in FORWARD_RESPONSE {
        if let Some(v) = from.get(*name) {
            if let Ok(v) = HeaderValue::from_bytes(v.as_bytes()) {
                to.insert(HeaderName::from_static(name), v);
            }
        }
    }
}

fn stream_back(up: reqwest::Response) -> Response {
    let status = StatusCode::from_u16(up.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = HeaderMap::new();
    copy_headers(up.headers(), &mut headers);
    let mut resp = Response::new(Body::from_stream(up.bytes_stream()));
    *resp.status_mut() = status;
    *resp.headers_mut() = headers;
    resp
}

/// Buffer a `tools/list` answer and drop every tool the allowlist does not
/// name. Anything the gate cannot parse fails *closed*: the caller gets an
/// error, never the unfiltered list.
async fn filtered_tools_list(up: reqwest::Response, id: Value) -> Response {
    let status = StatusCode::from_u16(up.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut headers = HeaderMap::new();
    copy_headers(up.headers(), &mut headers);
    let is_sse = up
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let fail = |why: &str| {
        tracing::error!(why, "tools/list response could not be filtered");
        Json(rpc_error(
            id.clone(),
            -32603,
            "korg-gate could not filter the tool list".into(),
        ))
        .into_response()
    };
    let bytes = match up.bytes().await {
        Ok(b) if b.len() <= MAX_FILTERED_BODY => b,
        Ok(_) => return fail("too large"),
        Err(_) => return fail("upstream body failed"),
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return fail("not utf-8");
    };
    let filtered = if is_sse {
        filter_sse(text)
    } else {
        filter_json(text).map(|v| v.to_string())
    };
    let Some(filtered) = filtered else {
        return fail("unparseable");
    };
    headers.remove(header::CONTENT_LENGTH);
    let mut resp = Response::new(Body::from(filtered));
    *resp.status_mut() = status;
    *resp.headers_mut() = headers;
    resp
}

/// Filter one JSON-RPC message. A message with `result.tools` keeps only the
/// allowlisted tools; one without (an error) passes as-is, since it lists none.
pub fn filter_json(text: &str) -> Option<Value> {
    let mut msg: Value = serde_json::from_str(text).ok()?;
    if let Some(result) = msg.get_mut("result") {
        let tools = result.get_mut("tools")?.as_array_mut()?;
        tools.retain(|t| {
            t.get("name")
                .and_then(Value::as_str)
                .is_some_and(tool_allowed)
        });
    } else if msg.get("error").is_none() {
        return None;
    }
    Some(msg)
}

/// Filter every `data:` payload in an SSE body that carries a tool list.
/// Multi-line `data:` fields are joined per the SSE spec before parsing.
pub fn filter_sse(text: &str) -> Option<String> {
    let mut out = String::new();
    for event in text.split("\n\n") {
        if event.trim().is_empty() {
            continue;
        }
        let mut other = Vec::new();
        let mut data = Vec::new();
        for line in event.lines() {
            match line.strip_prefix("data:") {
                Some(d) => data.push(d.strip_prefix(' ').unwrap_or(d)),
                None => other.push(line),
            }
        }
        for line in other {
            out.push_str(line);
            out.push('\n');
        }
        if !data.is_empty() {
            let joined = data.join("\n");
            let rewritten = match serde_json::from_str::<Value>(&joined) {
                Ok(v) if v.get("result").is_some() || v.get("error").is_some() => {
                    filter_json(&joined)?.to_string()
                }
                // A notification or anything else that is not the answer.
                Ok(v) => v.to_string(),
                Err(_) => return None,
            };
            out.push_str("data: ");
            out.push_str(&rewritten);
            out.push('\n');
        }
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_tool_list_keeps_only_allowlisted_tools() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"get_item"},{"name":"create_work_item"}]}}"#;
        let v = filter_json(body).unwrap();
        let names: Vec<_> = v["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["get_item"]);
    }

    #[test]
    fn a_result_without_a_tool_array_fails_closed() {
        assert!(filter_json(r#"{"jsonrpc":"2.0","id":1,"result":{"tools":"nope"}}"#).is_none());
        assert!(filter_json(r#"{"jsonrpc":"2.0","id":1}"#).is_none());
        assert!(filter_json("not json").is_none());
    }

    #[test]
    fn sse_tool_list_is_filtered_and_other_fields_survive() {
        let body = "event: message\nid: 7\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[{\"name\":\"delete_area\"},{\"name\":\"search\"}]}}\n\n";
        let out = filter_sse(body).unwrap();
        assert!(out.contains("event: message\n"));
        assert!(out.contains("id: 7\n"));
        assert!(out.contains("search"));
        assert!(!out.contains("delete_area"));
    }

    #[test]
    fn sse_with_an_unparseable_payload_fails_closed() {
        assert!(filter_sse("data: {not json\n\n").is_none());
    }
}
