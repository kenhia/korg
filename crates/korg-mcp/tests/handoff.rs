//! Sprint 025 — handoff tools over MCP (WI #609, proposal korg:614).
//!
//! The dispatch fence proves the three arms run; this proves they behave: the
//! create/get/update round-trip, the two rejection paths carrying the right
//! `code`, and the load-bearing reconciliation contract — a `has_handoff` edge
//! shows up in `get_work_item` / `get_proposal`'s LB-3 `related` block, titled,
//! with truncation exact past the cap. No handoff-specific read field exists.

use korg_test_support::{fresh_korg, new};
use serde_json::{json, Value};

mod common;
use common::{args, body, error_text, server};

/// The error `code` an isError tool result carries (`{message, code}`).
fn error_code(result: &rmcp::model::CallToolResult) -> String {
    let text = error_text(result);
    let v: Value = serde_json::from_str(&text).expect("error body is json");
    v["code"].as_str().expect("code").to_string()
}

#[tokio::test]
async fn create_get_update_round_trip() {
    let (_pg, pool) = fresh_korg().await;
    let wi = korg_core::repo::create_work_item(&pool, new::work_item("owner"))
        .await
        .unwrap();
    let server = server(pool);

    let created = body(
        &server
            .call(
                "create_handoff",
                args(json!({
                    "title": "Generator output contract",
                    "summary": "JSON schema + compatibility",
                    "body": "# State\nfull body",
                    "related_node_ids": [wi.node_id],
                })),
            )
            .await
            .unwrap(),
    );
    let node_id = created["node_id"].as_i64().unwrap();
    assert_eq!(created["related_node_ids"], json!([wi.node_id]));

    let full = body(
        &server
            .call("get_handoff", args(json!({ "node_id": node_id })))
            .await
            .unwrap(),
    );
    assert_eq!(full["body"], "# State\nfull body");
    assert_eq!(full["related_truncated"], json!(false));
    // The handoff sees its owner on the `in` side (owner -> handoff).
    assert_eq!(full["related"].as_array().unwrap().len(), 1);
    assert_eq!(full["related"][0]["label"], "has_handoff");
    assert_eq!(full["related"][0]["node_id"], json!(wi.node_id));

    body(
        &server
            .call(
                "update_handoff",
                args(json!({ "node_id": node_id, "body": "# State\nrevised" })),
            )
            .await
            .unwrap(),
    );
    let full = body(
        &server
            .call("get_handoff", args(json!({ "node_id": node_id })))
            .await
            .unwrap(),
    );
    assert_eq!(full["body"], "# State\nrevised");
}

#[tokio::test]
async fn create_rejections_carry_the_right_code() {
    let (_pg, pool) = fresh_korg().await;
    let wi = korg_core::repo::create_work_item(&pool, new::work_item("owner"))
        .await
        .unwrap();
    let server = server(pool);

    // A missing owner is not_found, and the whole create rolls back.
    let missing = server
        .call(
            "create_handoff",
            args(json!({
                "title": "half-attached",
                "summary": "s",
                "body": "b",
                "related_node_ids": [wi.node_id, 999_999],
            })),
        )
        .await
        .unwrap();
    assert_eq!(error_code(&missing), "not_found");

    // No owners and no opt-in is invalid_input.
    let empty = server
        .call(
            "create_handoff",
            args(json!({ "title": "orphan", "summary": "s", "body": "b" })),
        )
        .await
        .unwrap();
    assert_eq!(error_code(&empty), "invalid_input");
    // The DB-level "no partial insert remains" assertion lives in the core suite
    // (korg-core/tests/handoff.rs); here we fence the transport-visible `code`.
}

#[tokio::test]
async fn owner_reads_surface_the_handoff() {
    let (_pg, pool) = fresh_korg().await;
    let wi = korg_core::repo::create_work_item(&pool, new::work_item("has handoff"))
        .await
        .unwrap();
    let prop = korg_core::repo::create_proposal(&pool, {
        let mut p = new::proposal("covering sprint");
        p.covers = vec![wi.wi_number];
        p
    })
    .await
    .unwrap();
    let server = server(pool);

    body(
        &server
            .call(
                "create_handoff",
                args(json!({
                    "title": "Edge context handoff",
                    "summary": "s",
                    "body": "b",
                    "related_node_ids": [wi.node_id, prop.row.node_id],
                })),
            )
            .await
            .unwrap(),
    );

    // get_work_item: the handoff is inlined, titled, as an `out` edge (WI is the
    // subject). No handoff-specific field — it rides the generic `related`.
    let detail = body(
        &server
            .call("get_work_item", args(json!({ "wi_number": wi.wi_number })))
            .await
            .unwrap(),
    );
    let href = detail["related"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["label"] == "has_handoff")
        .expect("has_handoff is inlined on the work-item read");
    assert_eq!(href["kind"], "handoff");
    assert_eq!(href["title"], "Edge context handoff");
    assert_eq!(href["direction"], "out");

    // get_proposal: has_handoff shows, `covers` does not (it is in `covered`).
    let pdetail = body(
        &server
            .call("get_proposal", args(json!({ "node_id": prop.row.node_id })))
            .await
            .unwrap(),
    );
    let labels: Vec<&str> = pdetail["related"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap())
        .collect();
    assert!(
        labels.contains(&"has_handoff"),
        "proposal related: {labels:?}"
    );
    assert!(!labels.contains(&"covers"), "covers must stay in `covered`");
}

/// Response-size / truncation: a node with more handoffs than the cap inlines
/// exactly the cap and flags the rest, so a busy node never blows the payload.
#[tokio::test]
async fn many_handoffs_truncate_exactly() {
    let (_pg, pool) = fresh_korg().await;
    let wi = korg_core::repo::create_work_item(&pool, new::work_item("busy"))
        .await
        .unwrap();
    let server = server(pool);

    for i in 0..(korg_core::repo::RELATED_CONTEXT_CAP + 1) {
        body(
            &server
                .call(
                    "create_handoff",
                    args(json!({
                        "title": format!("handoff {i}"),
                        "summary": "s",
                        "body": "b",
                        "related_node_ids": [wi.node_id],
                    })),
                )
                .await
                .unwrap(),
        );
    }

    let detail = body(
        &server
            .call("get_work_item", args(json!({ "wi_number": wi.wi_number })))
            .await
            .unwrap(),
    );
    assert_eq!(
        detail["related"].as_array().unwrap().len(),
        korg_core::repo::RELATED_CONTEXT_CAP as usize,
        "inlines exactly the cap"
    );
    assert_eq!(detail["related_truncated"], json!(true));
}
