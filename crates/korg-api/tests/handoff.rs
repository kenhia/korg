//! Sprint 025 — handoff endpoints over REST (WI #609, proposal korg:614).
//!
//! The MCP suite (korg-mcp/tests/handoff.rs) and this one exercise the same
//! domain contract through the two transports, so a divergence in either shows
//! up here: the create/get/update round-trip, the two rejection codes/statuses,
//! and the reconciliation win — a `has_handoff` edge inlined on the owner's
//! work-item read (D-5 status matrix + LB-3 read contract).

use axum::http::StatusCode;
use serde_json::{json, Value};

mod common;
use common::{app, req};

fn assert_error(got: (StatusCode, Value), status: StatusCode, code: &str, what: &str) {
    assert_eq!(got.0, status, "{what}: status (body {:?})", got.1);
    assert_eq!(got.1["code"], code, "{what}: code");
}

async fn work_item(router: &axum::Router, title: &str) -> (i64, i64) {
    let (st, wi) = req(
        router,
        "POST",
        "/api/work-items",
        Some(json!({ "title": title, "content": "c" })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    (
        wi["wi_number"].as_i64().unwrap(),
        wi["node_id"].as_i64().unwrap(),
    )
}

#[tokio::test]
async fn create_get_update_round_trip() {
    let (_c, router) = app().await;
    let (_wi_number, wi_node) = work_item(&router, "owner").await;

    let (st, created) = req(
        &router,
        "POST",
        "/api/handoffs",
        Some(json!({
            "title": "Generator output contract",
            "summary": "JSON schema + compatibility",
            "body": "# State\nfull body",
            "related_node_ids": [wi_node],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node_id = created["node_id"].as_i64().unwrap();
    assert_eq!(created["related_node_ids"], json!([wi_node]));

    let (st, full) = req(&router, "GET", &format!("/api/handoffs/{node_id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(full["body"], "# State\nfull body");
    assert_eq!(full["related_truncated"], json!(false));
    assert_eq!(full["related"][0]["label"], "has_handoff");

    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/handoffs/{node_id}"),
        Some(json!({ "body": "# State\nrevised", "archived": true })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, full) = req(&router, "GET", &format!("/api/handoffs/{node_id}"), None).await;
    assert_eq!(full["body"], "# State\nrevised");
    assert_eq!(full["archived"], json!(true));
}

#[tokio::test]
async fn rejections_and_missing_reads() {
    let (_c, router) = app().await;
    let (_wi_number, wi_node) = work_item(&router, "owner").await;

    // A missing owner is 404 not_found.
    assert_error(
        req(
            &router,
            "POST",
            "/api/handoffs",
            Some(json!({
                "title": "half-attached",
                "summary": "s",
                "body": "b",
                "related_node_ids": [wi_node, 999_999],
            })),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "missing owner",
    );

    // No owners and no opt-in is 400 invalid_input.
    assert_error(
        req(
            &router,
            "POST",
            "/api/handoffs",
            Some(json!({ "title": "orphan", "summary": "s", "body": "b" })),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "empty related",
    );

    // A missing handoff read is 404, never 200 null (D-6).
    assert_error(
        req(&router, "GET", "/api/handoffs/999999", None).await,
        StatusCode::NOT_FOUND,
        "not_found",
        "missing handoff",
    );
}

#[tokio::test]
async fn owner_read_inlines_the_handoff() {
    let (_c, router) = app().await;
    let (wi_number, wi_node) = work_item(&router, "has handoff").await;

    let (st, _) = req(
        &router,
        "POST",
        "/api/handoffs",
        Some(json!({
            "title": "Edge context handoff",
            "summary": "s",
            "body": "b",
            "related_node_ids": [wi_node],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, detail) = req(
        &router,
        "GET",
        &format!("/api/work-items/{wi_number}"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let href = detail["related"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["label"] == "has_handoff")
        .expect("has_handoff inlined on the work-item read");
    assert_eq!(href["kind"], "handoff");
    assert_eq!(href["title"], "Edge context handoff");
    assert_eq!(href["direction"], "out");
}
