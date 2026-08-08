//! Live HTTP gate for the MCP server mounted at `/mcp`.
//!
//! Drives the real Streamable-HTTP transport in-process (via Tower `oneshot`)
//! exactly as a remote MCP client would over the network: each JSON-RPC request
//! is an independent POST returning `application/json` (stateless mode). Proves
//! initialize, tools/list, and tools/call (create + list) work end-to-end
//! against a real korg database.

use serde_json::{json, Value};
use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod common;
use common::app;

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
    // Asserted against the library's own list rather than a literal count.
    //
    // This line carried a hand-maintained number and a six-clause changelog of
    // every sprint that had moved it — the exact "inventory maintained by hand"
    // that `docs_drift` exists to abolish, sitting in a suite `docs_drift` does
    // not read. Sprint 056 added two tools and this was the only thing in the
    // workspace that failed, well after the three *real* inventories (the
    // catalogue, the README count, the schema snapshot) had already agreed.
    //
    // Comparing the sets is also the stronger assertion, and closer to what
    // this suite is actually for: the subject here is the HTTP transport, and
    // what it owes is that `tools/list` over JSON-RPC delivers exactly what the
    // library advertises in-process — not that some number is some other number.
    let served: BTreeSet<String> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .map(str::to_string)
        .collect();
    let advertised: BTreeSet<String> = korg_mcp::tools::tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    assert_eq!(
        served, advertised,
        "the MCP HTTP transport must serve exactly the tools the library advertises"
    );
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
    assert!(names.contains(&"create_handoff"));
    assert!(names.contains(&"get_handoff"));
    assert!(names.contains(&"update_handoff"));
    assert!(!names.contains(&"generate_slots"));
    // The whole daily-slots surface is gone (#965), not merely renamed.
    for retired in [
        "create_topic",
        "search_topics",
        "create_daily_plan_item",
        "daily_plan_history",
    ] {
        assert!(
            !names.contains(&retired),
            "`{retired}` was removed with the slots feature and must not come back"
        );
    }

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
    // Since 0009_identity, wi_number IS the node id — one number everywhere.
    assert_eq!(
        made["wi_number"], made["node_id"],
        "wi_number == node_id: {made}"
    );
    let _wi_number = made["wi_number"].as_i64().unwrap();

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
    let page = tool_payload(&listed);
    let items = page["items"].as_array().expect("work items array");
    assert_eq!(items.len(), 1);
    assert_eq!(page["total"], 1);
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
