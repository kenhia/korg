//! Live HTTP gate for the MCP server mounted at `/mcp`.
//!
//! Drives the real Streamable-HTTP transport in-process (via Tower `oneshot`)
//! exactly as a remote MCP client would over the network: each JSON-RPC request
//! is an independent POST returning `application/json` (stateless mode). Proves
//! initialize, tools/list, and tools/call (create + list) work end-to-end
//! against a real korg database.

use http_body_util::BodyExt;
use korg_api::{build_router, AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;

async fn app() -> (impl Sized, axum::Router) {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = korg_core::connect(&url).await.expect("connect+migrate");
    let router = build_router(AppState {
        pool: Arc::new(pool),
    });
    (container, router)
}

/// POST a JSON-RPC message to `/mcp` and return (status, parsed-json-body).
async fn rpc(router: &axum::Router, msg: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("host", "localhost")
        .header("mcp-protocol-version", "2025-06-18")
        .body(Body::from(serde_json::to_vec(&msg).unwrap()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.expect("request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// Unwrap a `tools/call` result whose single text content is itself JSON.
fn tool_payload(body: &Value) -> Value {
    let text = body["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("expected text content, got: {body}"));
    serde_json::from_str(text).expect("tool payload is json")
}

#[tokio::test]
async fn mcp_http_end_to_end() {
    let (_c, router) = app().await;

    // 1. initialize handshake returns this server's identity.
    let (st, init) = rpc(
        &router,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "korg-http-gate", "version": "0"}
            }
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "initialize HTTP status; body={init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "korg-mcp");
    assert!(
        init["result"]["capabilities"].get("tools").is_some(),
        "server advertises tools capability: {init}"
    );

    // 2. tools/list advertises the full korg tool surface.
    let (st, tl) = rpc(
        &router,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let tools = tl["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 28, "expected 28 tools, got {}", tools.len());
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"create_work_item"));
    assert!(names.contains(&"list_work_items"));
    assert!(names.contains(&"update_work_item"));
    assert!(names.contains(&"create_project"));
    assert!(names.contains(&"create_area"));
    assert!(names.contains(&"list_areas"));
    assert!(names.contains(&"update_card"));
    assert!(names.contains(&"unrelate"));
    assert!(names.contains(&"add_comment"));

    // 3. tools/call create_work_item — a mutating tool over the wire.
    let (st, created) = rpc(
        &router,
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {
                "name": "create_work_item",
                "arguments": {"title": "via http mcp", "content": "hello", "wi_tshirt": "S"}
            }
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let made = tool_payload(&created);
    assert_eq!(made["wi_number"], 1, "first work item is serial #1: {made}");

    // 4. tools/call list_work_items reflects the new item.
    let (st, listed) = rpc(
        &router,
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": {"name": "list_work_items", "arguments": {}}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let items = tool_payload(&listed);
    let items = items.as_array().expect("work items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "via http mcp");

    // 5. an unknown tool yields a clean tool error (isError), not a transport crash.
    let (st, bad) = rpc(
        &router,
        json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": {"name": "definitely_not_a_tool", "arguments": {}}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let is_err = bad["result"]["isError"].as_bool().unwrap_or(false) || bad.get("error").is_some();
    assert!(is_err, "unknown tool should error cleanly: {bad}");
}
