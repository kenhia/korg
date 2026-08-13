//! The MCP protocol revision korg actually implements (sprint 058, korg:1215).
//!
//! Separate from `mcp_http.rs` — that suite proves the transport carries korg's
//! tools; this one proves korg tells the truth about *which revision* it speaks
//! and emits the shape that revision requires.
//!
//! Every request here is raw JSON-RPC, and that is not a stylistic choice.
//! rmcp's own client cannot drive a conformant `2026-07-28` session: it omits
//! the per-request `_meta` keys rmcp's own server requires, so a test written
//! against it silently exercises the `2025-11-25` path and passes while proving
//! nothing. kaed's sprint 016 lost a canary battery to exactly that (korg:1220
//! comment) — a green rmcp-client test says nothing about `2026-07-28`.
//!
//! A conformant non-`initialize` request at `2026-07-28` needs all of:
//!   * `_meta` carrying `io.modelcontextprotocol/protocolVersion` **and**
//!     `io.modelcontextprotocol/clientCapabilities` (SEP-2575),
//!   * an `MCP-Protocol-Version` header **agreeing** with that `_meta` value,
//!   * the SEP-2243 `Mcp-Method` header naming the body's method (plus
//!     `Mcp-Name` for `tools/call`).
//!
//! It carries no `mcp-session-id`: `2026-07-28` uses the inline lifecycle and
//! `initialize` issues no session (SEP-2567).

use serde_json::{json, Value};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

mod common;
use common::app;

/// The revision this sprint added. Spelled literally rather than taken from
/// `rmcp::model::ProtocolVersion` on purpose: the assertions below are about
/// the bytes on the wire, and a constant that moves with a dependency bump
/// would make them agree with rmcp instead of with the spec.
const V_2026: &str = "2026-07-28";
const V_2025_11: &str = "2025-11-25";

/// POST raw JSON-RPC to `/mcp` with an explicit protocol version and whatever
/// SEP-2243 headers the caller needs.
async fn post(
    router: &axum::Router,
    version: &str,
    extra: &[(&str, &str)],
    msg: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("host", "localhost")
        .header("mcp-protocol-version", version);
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let resp = router
        .clone()
        .oneshot(
            req.body(Body::from(serde_json::to_vec(&msg).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

/// The `_meta` block SEP-2575 requires on every non-`initialize` request once
/// `2026-07-28` is negotiated. Omitting either key is a `-32602`, not a warning.
fn meta_2026() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": V_2026,
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

async fn initialize(router: &axum::Router, version: &str) -> Value {
    let (st, body) = post(
        router,
        version,
        &[],
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": {"name": "korg-protocol-gate", "version": "0"}
            }
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "initialize at {version}: {body}");
    body
}

/// The bug this sprint exists to prevent (korg:1212, korg:1215).
///
/// A client that negotiates `2026-07-28` validates `tools/list` against that
/// revision's schema, which requires the SEP-2549 cache metadata. Advertising
/// the version without emitting the fields makes Claude Code ≥2.1.227 drop
/// **every** tool while leaving the session up — connected, instructions
/// delivered, nothing callable, no error anywhere the user can see it.
#[tokio::test]
async fn tools_list_carries_cache_metadata_for_a_2026_07_28_peer() {
    let (_c, router) = app().await;

    let (st, body) = post(
        &router,
        V_2026,
        &[("mcp-method", "tools/list")],
        json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list",
            "params": {"_meta": meta_2026()}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "tools/list at {V_2026}: {body}");

    let result = &body["result"];
    assert!(
        result["tools"].as_array().is_some_and(|t| !t.is_empty()),
        "the whole point is that the tools still arrive: {body}"
    );
    // Well-typed, not merely present: the client's validator rejected
    // `undefined` and would equally reject a stringified number.
    assert!(
        result["ttlMs"].is_number(),
        "ttlMs must be a number, got {:?} in {body}",
        result["ttlMs"]
    );
    assert_eq!(
        result["cacheScope"], "public",
        "korg's catalogue is static per build, so it is honestly public: {body}"
    );
}

/// The other half of the same decision: a `2025-11-25` peer is entitled to
/// `2025-11-25`'s shape. korg serves a newer revision than some of its clients
/// ask for, which means it owns the translation — rmcp strips `resultType` for
/// legacy peers for this exact reason, but it does **not** strip cache metadata.
#[tokio::test]
async fn tools_list_omits_cache_metadata_for_a_legacy_peer() {
    let (_c, router) = app().await;
    initialize(&router, V_2025_11).await;

    let (st, body) = post(
        &router,
        V_2025_11,
        &[],
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "tools/list at {V_2025_11}: {body}");

    let result = &body["result"];
    assert!(
        result["tools"].as_array().is_some_and(|t| !t.is_empty()),
        "legacy peers still get the catalogue: {body}"
    );
    assert!(
        result.get("ttlMs").is_none() && result.get("cacheScope").is_none(),
        "a 2025-11-25 result must not carry 2026-07-28 fields: {body}"
    );
}

/// korg answers the revision it *implements*, never the SDK's idea of latest.
///
/// The fallback is pinned to a korg constant rather than `ProtocolVersion::
/// LATEST` (kaed 015 D-3): `LATEST` moving underneath a server is precisely how
/// this bug class arrives — through a dependency bump with no code to review.
#[tokio::test]
async fn negotiation_answers_the_revision_korg_implements() {
    let (_c, router) = app().await;

    for v in [V_2026, V_2025_11, "2025-06-18", "2025-03-26", "2024-11-05"] {
        let body = initialize(&router, v).await;
        assert_eq!(
            body["result"]["protocolVersion"], v,
            "{v} is supported, so it must negotiate as itself: {body}"
        );
    }

    // An unknown version comes back as korg's ceiling, which is how you read a
    // server's real revision from outside without trusting its docs.
    let body = initialize(&router, "9999-12-31").await;
    assert_eq!(
        body["result"]["protocolVersion"], V_2026,
        "an unsupported request falls back to what korg serves: {body}"
    );
}

/// The wire suite cannot prove client *acceptance* — only a live Claude Code
/// session can (see the sprint record). What it can prove is that a real tool
/// call survives the `2026-07-28` request shape end to end, which is the half
/// that would break silently if `_meta` handling or MRTR framing were wrong.
#[tokio::test]
async fn a_real_tool_call_completes_at_2026_07_28() {
    let (_c, router) = app().await;

    let (st, body) = post(
        &router,
        V_2026,
        &[("mcp-method", "tools/call"), ("mcp-name", "get_board")],
        json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "get_board", "arguments": {}, "_meta": meta_2026()}
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "tools/call at {V_2026}: {body}");
    assert!(
        body["error"].is_null(),
        "a conformant 2026-07-28 call must not be rejected: {body}"
    );
    assert_ne!(
        body["result"]["isError"], true,
        "get_board should succeed against a fresh korg: {body}"
    );
}
