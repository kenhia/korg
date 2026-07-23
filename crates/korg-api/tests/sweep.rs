//! The REST routes and node-preview kinds nothing had ever requested
//! (WI #551).
//!
//! Five routes had no test at all:
//!
//! ```text
//! GET    /api/areas
//! GET    /api/projects/:name/plan      ← sole caller of repo::project_edges
//! GET    /api/reports
//! PUT    /api/daily-plan/:plan_date/order
//! DELETE /api/daily-plan/:node_id
//! ```
//!
//! `/api/projects/:name/plan` is the one that mattered. It is the only caller
//! of `project_edges`, which had zero coverage anywhere, and it feeds both the
//! `/plan` view and the `plan-status` skill — so a regression there does not
//! throw, it just answers "where are we on the plan" wrongly, which is the
//! failure mode nobody notices.
//!
//! `GET /api/nodes/:id` is here too: `api.rs` covered it for `workitem` and
//! `card`, leaving five of the seven kinds unpreviewed.

use axum::http::StatusCode;
use korg_core::repo::{self, NewReport};
use korg_test_support::new;
use serde_json::json;
use time::macros::date;

mod common;
use common::{app, app_with_pool, req};

/// The clock `common::app` pins, so plan dates are not in the past.
const TODAY: &str = "2026-07-11";

async fn work_item(router: &axum::Router, title: &str, project: Option<&str>) -> (i64, i64) {
    let mut body = json!({"title": title, "content": ""});
    if let Some(p) = project {
        body["project"] = json!(p);
    }
    let (st, wi) = req(router, "POST", "/api/work-items", Some(body)).await;
    assert_eq!(st, StatusCode::OK, "create work item: {wi:?}");
    (
        wi["wi_number"].as_i64().expect("wi_number"),
        wi["node_id"].as_i64().expect("node_id"),
    )
}

// --- areas ------------------------------------------------------------------

/// `GET /api/areas` lists a project's areas, and is scoped to the project it
/// was asked about rather than returning every area korg knows.
#[tokio::test]
async fn areas_are_listed_per_project() {
    let (_pg, router) = app().await;

    for name in ["korg", "other"] {
        let (st, _) = req(
            &router,
            "POST",
            "/api/projects",
            Some(json!({"name": name})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
    }
    for (project, area) in [("korg", "core"), ("korg", "web"), ("other", "elsewhere")] {
        let (st, body) = req(
            &router,
            "POST",
            "/api/areas",
            Some(json!({"project": project, "name": area})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "create area: {body:?}");
    }

    let (st, areas) = req(&router, "GET", "/api/areas?project=korg", None).await;
    assert_eq!(st, StatusCode::OK);
    let names: Vec<&str> = areas
        .as_array()
        .expect("array")
        .iter()
        .map(|a| a["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["core", "web"], "korg's areas only");

    let (st, none) = req(&router, "GET", "/api/areas?project=no-such", None).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "an unknown project has no areas; that is not an error: {none:?}"
    );
    assert!(none.as_array().expect("array").is_empty());
}

// --- the plan view ----------------------------------------------------------

/// `GET /api/projects/:name/plan` returns the project's work items **and** its
/// `depends_on` edges — the only exercise `repo::project_edges` gets anywhere.
///
/// The assertion that matters is the scoping one: an edge is included only if
/// *both* endpoints belong to the project. An edge leaking in from another
/// project would draw a dependency arrow between nodes the view cannot render.
#[tokio::test]
async fn the_plan_view_returns_items_and_their_intra_project_edges() {
    let (_pg, router) = app().await;
    for name in ["korg", "other"] {
        req(
            &router,
            "POST",
            "/api/projects",
            Some(json!({"name": name})),
        )
        .await;
    }

    let (_, a) = work_item(&router, "foundation", Some("korg")).await;
    let (_, b) = work_item(&router, "depends on foundation", Some("korg")).await;
    let (_, elsewhere) = work_item(&router, "another project's item", Some("other")).await;
    let (_, unscoped) = work_item(&router, "no project at all", None).await;

    let relate = |left: i64, right: i64| {
        let router = router.clone();
        async move {
            let (st, body) = req(
                &router,
                "POST",
                "/api/relationships",
                Some(json!({"left": left, "right": right, "label": "depends_on"})),
            )
            .await;
            assert_eq!(st, StatusCode::OK, "relate: {body:?}");
        }
    };
    relate(b, a).await; // both in korg — belongs in the plan
    relate(a, elsewhere).await; // crosses into another project
    relate(a, unscoped).await; // one endpoint has no project

    let (st, plan) = req(&router, "GET", "/api/projects/korg/plan", None).await;
    assert_eq!(st, StatusCode::OK);

    let titles: Vec<&str> = plan["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["title"].as_str().expect("title"))
        .collect();
    assert_eq!(
        titles.len(),
        2,
        "the plan holds korg's items and nobody else's: {titles:?}"
    );

    let edges = plan["edges"].as_array().expect("edges");
    assert_eq!(
        edges.len(),
        1,
        "only the edge with both endpoints in korg belongs: {edges:?}"
    );
    assert_eq!(
        edges[0],
        json!([b, a]),
        "and it reads left-depends-on-right"
    );
}

/// A project with nothing in it answers honestly rather than 404ing — the
/// `/plan` view renders an empty graph, and `plan-status` reports no work.
#[tokio::test]
async fn an_empty_project_has_an_empty_plan() {
    let (_pg, router) = app().await;
    req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name": "empty"})),
    )
    .await;

    let (st, plan) = req(&router, "GET", "/api/projects/empty/plan", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(plan["items"].as_array().expect("items").is_empty());
    assert!(plan["edges"].as_array().expect("edges").is_empty());
}

// --- reports ----------------------------------------------------------------

/// `GET /api/reports` and `GET /api/reports/:node_id`. Reports are written over
/// MCP only, so these two reads are the whole REST surface for them — and the
/// `/reports` UI's only source.
#[tokio::test]
async fn reports_can_be_read_over_rest() {
    let (_pg, pool, router) = app_with_pool().await;

    let (st, empty) = req(&router, "GET", "/api/reports", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        empty.as_array().expect("array").is_empty(),
        "no reports yet: {empty:?}"
    );

    // Reports have no REST write route — seed through the repo `create_report`
    // dispatches to.
    let finding = repo::create_work_item(&pool, new::work_item("a finding"))
        .await
        .expect("wi");
    for (source, day) in [
        ("kmon", date!(2026 - 07 - 10)),
        ("kmon", date!(2026 - 07 - 11)),
        ("other", date!(2026 - 07 - 11)),
    ] {
        repo::upsert_report(
            &pool,
            NewReport {
                findings: vec![finding.wi_number],
                body: "the body".into(),
                ..new::report(source, day)
            },
        )
        .await
        .expect("report");
    }

    let (st, all) = req(&router, "GET", "/api/reports", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(all.as_array().expect("array").len(), 3);

    let (st, mine) = req(&router, "GET", "/api/reports?source=kmon", None).await;
    assert_eq!(st, StatusCode::OK);
    let items = mine.as_array().expect("array");
    assert_eq!(items.len(), 2, "the source filter reaches the query string");
    assert_eq!(
        items[0]["report_date"], "2026-07-11",
        "newest first survives the REST hop"
    );

    let node_id = items[0]["node_id"].as_i64().expect("node_id");
    let (st, one) = req(&router, "GET", &format!("/api/reports/{node_id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        one["body"], "the body",
        "the full read carries the markdown"
    );
    assert_eq!(
        one["findings"][0]["wi_number"], finding.wi_number,
        "and the linked findings"
    );

    let (st, missing) = req(&router, "GET", "/api/reports/999999", None).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "a missing report must 404, not 200 with null"
    );
    assert_eq!(missing["code"], "not_found");
}

// --- daily plan: the two mutating routes ------------------------------------

/// `PUT /api/daily-plan/:plan_date/order` renumbers a day, and
/// `DELETE /api/daily-plan/:node_id` removes one item and closes the gap.
#[tokio::test]
async fn a_day_can_be_reordered_and_items_deleted() {
    let (_pg, router) = app().await;

    let mut ids = Vec::new();
    for title in ["first", "second", "third"] {
        let (_, node_id) = work_item(&router, title, None).await;
        let (st, item) = req(
            &router,
            "POST",
            "/api/daily-plan",
            Some(json!({"source_node_id": node_id, "plan_date": TODAY})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "plan item: {item:?}");
        ids.push(item["node_id"].as_i64().expect("node_id"));
    }

    let (st, reordered) = req(
        &router,
        "PUT",
        &format!("/api/daily-plan/{TODAY}/order"),
        Some(json!({"node_ids": [ids[2], ids[1], ids[0]]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "reorder: {reordered:?}");
    let order: Vec<i64> = reordered
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["node_id"].as_i64().expect("node_id"))
        .collect();
    assert_eq!(order, vec![ids[2], ids[1], ids[0]]);

    // A reorder that omits an item is a conflict, not a silent partial write.
    let (st, err) = req(
        &router,
        "PUT",
        &format!("/api/daily-plan/{TODAY}/order"),
        Some(json!({"node_ids": [ids[0]]})),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "partial reorder: {err:?}");
    assert_eq!(err["code"], "conflict");

    let (st, deleted) = req(
        &router,
        "DELETE",
        &format!("/api/daily-plan/{}", ids[1]),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "delete: {deleted:?}");
    assert_eq!(deleted["deleted"], true);

    let (st, day) = req(
        &router,
        "GET",
        &format!("/api/daily-plan?from={TODAY}&to={TODAY}"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let positions: Vec<i64> = day
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["position"].as_i64().expect("position"))
        .collect();
    assert_eq!(positions, vec![0, 1], "the gap is closed, not left sparse");

    let (st, gone) = req(
        &router,
        "DELETE",
        &format!("/api/daily-plan/{}", ids[1]),
        None,
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "deleting twice must 404, not report success again: {gone:?}"
    );
}

// --- node previews ----------------------------------------------------------

/// `GET /api/nodes/:id` resolves any node id to a uniform preview. `api.rs`
/// covers `workitem` and `card`; this covers the other five kinds.
///
/// The preview is what the UI renders when you follow a relationship to a node
/// whose kind you were not already looking at, so an uncovered kind degrades
/// silently to the fallback title `"{kind} #{id}"` — a preview that renders,
/// looks deliberate, and says nothing. Each assertion below therefore checks a
/// field that only the kind-specific branch can produce.
#[tokio::test]
async fn every_node_kind_previews_with_its_own_shape() {
    let (_pg, pool, router) = app_with_pool().await;

    let preview = |id: i64| {
        let router = router.clone();
        async move {
            let (st, body) = req(&router, "GET", &format!("/api/nodes/{id}"), None).await;
            assert_eq!(st, StatusCode::OK, "preview {id}: {body:?}");
            body
        }
    };
    let field = |p: &serde_json::Value, label: &str| -> Option<String> {
        p["fields"]
            .as_array()?
            .iter()
            .find(|f| f["label"] == label)?["value"]
            .as_str()
            .map(str::to_string)
    };

    // --- link ---
    let link = repo::create_link(&pool, new::link("https://example.invalid/read-me"))
        .await
        .expect("link");
    let p = preview(link.node_id).await;
    assert_eq!(p["kind"], "link");
    assert_eq!(
        p["badges"],
        json!(["Unread", "unread"]),
        "a link previews its disposition and read state"
    );
    assert_eq!(
        field(&p, "URL").as_deref(),
        Some("https://example.invalid/read-me")
    );

    // --- report ---
    let report = repo::upsert_report(
        &pool,
        NewReport {
            body: "the full report".into(),
            escalated: true,
            model: Some("claude".into()),
            ..new::report("kmon", date!(2026 - 07 - 11))
        },
    )
    .await
    .expect("report");
    let p = preview(report.node_id).await;
    assert_eq!(p["kind"], "report");
    assert_eq!(
        p["title"], "kmon — 2026-07-11",
        "a report titles itself source — date"
    );
    assert_eq!(p["badges"], json!(["ok", "escalated"]));
    assert_eq!(field(&p, "Model").as_deref(), Some("claude"));
    assert_eq!(p["body"], "the full report");

    // --- sprint_proposal ---
    let proposal = repo::create_proposal(
        &pool,
        korg_core::repo::NewProposal {
            summary: "what this sprint is".into(),
            pinned: true,
            ..new::proposal("a proposal")
        },
    )
    .await
    .expect("proposal");
    let p = preview(proposal.row.node_id).await;
    assert_eq!(p["kind"], "sprint_proposal");
    assert_eq!(p["title"], "a proposal");
    assert_eq!(p["badges"], json!(["proposed", "pinned"]));
    assert_eq!(p["body"], "what this sprint is");
    assert_eq!(p["body_label"], "Summary");

    // --- topic ---
    let (st, topic) = req(
        &router,
        "POST",
        "/api/topics",
        Some(json!({"name": "a topic", "description": "what it is about"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "create topic: {topic:?}");
    let p = preview(topic["node_id"].as_i64().expect("node_id")).await;
    assert_eq!(p["kind"], "topic");
    assert_eq!(p["title"], "a topic");
    assert_eq!(p["body"], "what it is about");
    assert_eq!(p["body_label"], "Description");

    // --- daily_plan_item ---
    let (_, source) = work_item(&router, "planned work", None).await;
    let (st, item) = req(
        &router,
        "POST",
        "/api/daily-plan",
        Some(json!({"source_node_id": source, "plan_date": TODAY})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "plan item: {item:?}");
    let item_id = item["node_id"].as_i64().expect("node_id");
    let p = preview(item_id).await;
    assert_eq!(p["kind"], "daily_plan_item");
    assert_eq!(p["title"], "planned work", "it previews its display text");
    assert_eq!(field(&p, "Date").as_deref(), Some(TODAY));
    assert_eq!(
        field(&p, "Source").as_deref(),
        Some(format!("#{source}").as_str())
    );
    assert_eq!(
        p["badges"],
        json!([]),
        "an incomplete item carries no completion badge"
    );

    // Completing it adds the badge — the one preview field that changes.
    let (st, done) = req(
        &router,
        "PATCH",
        &format!("/api/daily-plan/{item_id}/completion"),
        Some(json!({"completed": true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "complete: {done:?}");
    let p = preview(item_id).await;
    assert_eq!(p["badges"], json!(["complete"]));
}

/// No node has this id, and the preview says so rather than inventing one.
#[tokio::test]
async fn previewing_a_missing_node_is_not_found() {
    let (_pg, router) = app().await;
    let (st, body) = req(&router, "GET", "/api/nodes/999999", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}
