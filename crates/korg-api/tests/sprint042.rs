//! Sprint 042 over REST — the membership markers on the row reads the web app
//! walks, and the Planning rail's rollup (#824, #813, #823).
//!
//! The core suite (korg-core/tests/sprint042.rs) pins the SQL. This one pins
//! the two things only the transport can get wrong: that the markers survive
//! serialization onto both row tiers, and that `/api/proposals/rollup` is
//! reachable at all — it shares a prefix with `/api/proposals/:node_id`, and a
//! router that resolved `rollup` as a node id would 404 (or worse, 400) on a
//! route that looks registered.

use axum::http::StatusCode;
use serde_json::{json, Value};

mod common;
use common::{app, req};

async fn work_item(router: &axum::Router, title: &str, details: Option<&str>) -> i64 {
    let mut body = json!({ "title": title, "content": "c", "project": "korg" });
    if let Some(d) = details {
        body["details"] = json!(d);
    }
    let (st, wi) = req(router, "POST", "/api/work-items", Some(body)).await;
    assert_eq!(st, StatusCode::OK, "create work item: {wi:?}");
    wi["wi_number"].as_i64().unwrap()
}

async fn project(router: &axum::Router, name: &str) {
    let (st, p) = req(
        router,
        "POST",
        "/api/projects",
        Some(json!({ "name": name })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "create project: {p:?}");
}

async fn proposal(router: &axum::Router, title: &str, covers: Vec<i64>) -> i64 {
    let (st, p) = req(
        router,
        "POST",
        "/api/proposals",
        Some(json!({
            "title": title,
            "summary": "s",
            "project": "korg",
            "work_item_numbers": covers,
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "create proposal: {p:?}");
    p["node_id"].as_i64().unwrap()
}

fn find(items: &Value, wi_number: i64) -> Value {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["wi_number"] == wi_number)
        .unwrap_or_else(|| panic!("#{wi_number} missing"))
        .clone()
}

/// Both row tiers, one assertion each — the full REST row the Work Items page
/// walks, and the lean survey the Review page reads.
#[tokio::test]
async fn both_row_tiers_carry_the_membership_markers() {
    let (_c, router) = app().await;
    project(&router, "korg").await;
    let covered = work_item(&router, "covered", Some("the long form")).await;
    let loose = work_item(&router, "loose", None).await;
    let p = proposal(&router, "a sprint", vec![covered]).await;

    let (st, full) = req(&router, "GET", "/api/work-items", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(find(&full["items"], covered)["proposal_node_id"], p);
    assert_eq!(find(&full["items"], loose)["proposal_node_id"], Value::Null);
    assert_eq!(find(&full["items"], covered)["has_handoff"], false);

    let (st, survey) = req(&router, "GET", "/api/work-items/survey", None).await;
    assert_eq!(st, StatusCode::OK);
    let c = find(&survey["items"], covered);
    assert_eq!(c["proposal_node_id"], p);
    assert_eq!(
        c["has_details"], true,
        "the marker the survey projection could not carry before"
    );
    assert_eq!(find(&survey["items"], loose)["has_details"], false);
}

/// A handoff attached to a work item lights the row marker on both tiers —
/// the edge is written by `POST /api/handoffs`, read back off the row.
#[tokio::test]
async fn a_handoff_lights_the_row_marker() {
    let (_c, router) = app().await;
    project(&router, "korg").await;
    let wi_number = work_item(&router, "owner", None).await;
    let (st, item) = req(
        &router,
        "GET",
        &format!("/api/work-items/{wi_number}"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(item["has_handoff"], false, "nothing attached yet");
    let node_id = item["node_id"].as_i64().unwrap();

    let (st, _) = req(
        &router,
        "POST",
        "/api/handoffs",
        Some(json!({
            "title": "picking this up",
            "summary": "state",
            "body": "# context",
            "related_node_ids": [node_id],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (_, item) = req(
        &router,
        "GET",
        &format!("/api/work-items/{wi_number}"),
        None,
    )
    .await;
    assert_eq!(item["has_handoff"], true);
    let (_, survey) = req(&router, "GET", "/api/work-items/survey", None).await;
    assert_eq!(find(&survey["items"], wi_number)["has_handoff"], true);
}

/// #823's rail count, and the route-shape check: `rollup` is a static segment
/// sharing a prefix with `/api/proposals/:node_id`.
#[tokio::test]
async fn the_rollup_route_resolves_and_counts_per_project() {
    let (_c, router) = app().await;
    project(&router, "korg").await;
    project(&router, "empty").await;
    let a = work_item(&router, "a", None).await;
    let _b = work_item(&router, "b", None).await;
    proposal(&router, "a sprint", vec![a]).await;

    let (st, rollup) = req(&router, "GET", "/api/proposals/rollup", None).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "static segment must win over :node_id — got {rollup:?}"
    );

    let rows = rollup.as_array().unwrap();
    let korg = rows.iter().find(|r| r["project"] == "korg").unwrap();
    assert_eq!(korg["proposals"], 1);
    assert_eq!(korg["wi_in_proposal"], 1);
    assert_eq!(korg["wi_total"], 2);

    let empty = rows.iter().find(|r| r["project"] == "empty").unwrap();
    assert_eq!(
        (
            empty["proposals"].as_i64(),
            empty["wi_in_proposal"].as_i64(),
            empty["wi_total"].as_i64()
        ),
        (Some(0), Some(0), Some(0)),
        "a project with nothing in it still gets a rail entry"
    );
}
