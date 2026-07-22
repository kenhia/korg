//! The §4.2 error-status matrix from the 2026-07 deep review, as tests.
//!
//! Every row here was live-verified as *wrong* before this sprint: bad input
//! and missing entities came back as 200s or 500s, and one of them
//! (`PATCH /api/cards/<work-item node>`) silently mutated a different entity
//! and reported success. These assertions are the regression fence.

use http_body_util::BodyExt;
use korg_api::{build_router, AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use time::macros::datetime;
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
        config: Arc::new(
            korg_core::config::KorgConfig::fixed("UTC", datetime!(2026-07-11 12:00 UTC)).unwrap(),
        ),
    });
    (container, router)
}

async fn req(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };
    let resp = router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .expect("request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}

/// Assert status + the stable `code` field (D-5) together: agents branch on
/// the code, so a right status with a missing code is still a broken contract.
fn assert_error(got: (StatusCode, Value), status: StatusCode, code: &str, what: &str) {
    assert_eq!(got.0, status, "{what}: status (body {:?})", got.1);
    assert_eq!(got.1["code"], code, "{what}: code");
    assert!(
        got.1["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{what}: message"
    );
}

async fn work_item(router: &axum::Router, title: &str) -> i64 {
    let (st, wi) = req(
        router,
        "POST",
        "/api/work-items",
        Some(json!({"title": title, "content": "c"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    wi["wi_number"].as_i64().unwrap()
}

#[tokio::test]
async fn missing_entities_are_404_not_success_or_500() {
    let (_c, router) = app().await;
    let wi = work_item(&router, "extant").await;

    // Updates against a missing row used to return `200 {"ok":true}` (F-03).
    assert_error(
        req(
            &router,
            "PATCH",
            "/api/work-items/9999",
            Some(json!({"title":"ghost"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "PATCH missing work item",
    );

    // Single-item reads: 404, not `200 null` (D-6).
    assert_error(
        req(&router, "GET", "/api/work-items/9999", None).await,
        StatusCode::NOT_FOUND,
        "not_found",
        "GET missing work item",
    );
    assert_error(
        req(&router, "GET", "/api/nodes/9999", None).await,
        StatusCode::NOT_FOUND,
        "not_found",
        "GET missing node",
    );
    assert_error(
        req(&router, "GET", "/api/topics/9999", None).await,
        StatusCode::NOT_FOUND,
        "not_found",
        "GET missing topic",
    );
    // …and reports, which used to be a 500 (F-02).
    assert_error(
        req(&router, "GET", "/api/reports/9999", None).await,
        StatusCode::NOT_FOUND,
        "not_found",
        "GET missing report",
    );

    // Comments and relationships to nonexistent nodes were raw FK 500s.
    assert_error(
        req(
            &router,
            "POST",
            "/api/nodes/9999/comments",
            Some(json!({"body":"orphan"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "comment on missing node",
    );
    assert_error(
        req(
            &router,
            "POST",
            "/api/relationships",
            Some(json!({"left": wi, "right": 9999, "label": "related-to"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "relate to missing node",
    );
    assert_error(
        req(
            &router,
            "PATCH",
            "/api/comments/9999",
            Some(json!({"body":"edit"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "PATCH missing comment",
    );
    assert_error(
        req(
            &router,
            "PATCH",
            "/api/topics/9999",
            Some(json!({"name":"ghost"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "PATCH missing topic",
    );
    assert_error(
        req(
            &router,
            "PATCH",
            "/api/links/9999",
            Some(json!({"read":true})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "PATCH missing link",
    );

    // Deletes report what they did instead of always claiming success.
    let (st, body) = req(&router, "DELETE", "/api/comments/9999", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["deleted"], false, "no such comment");
    let (st, body) = req(&router, "DELETE", "/api/relationships/9999", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["deleted"], false, "no such edge");
}

#[tokio::test]
async fn bad_input_is_400_not_500() {
    let (_c, router) = app().await;
    let wi = work_item(&router, "extant").await;

    assert_error(
        req(
            &router,
            "POST",
            "/api/work-items",
            Some(json!({"title":"t","content":"c","wi_tshirt":"GIGANTIC"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unknown t-shirt size",
    );
    // wi_type was entirely free text before D-2.
    assert_error(
        req(
            &router,
            "POST",
            "/api/work-items",
            Some(json!({"title":"t","content":"c","wi_type":"taks"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unknown wi_type",
    );
    assert_error(
        req(
            &router,
            "POST",
            "/api/cards",
            Some(json!({"title":"c","status":"Bogus"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unknown card status",
    );
    assert_error(
        req(
            &router,
            "PATCH",
            &format!("/api/links/{wi}"),
            Some(json!({"disposition":"Someday"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unknown link disposition",
    );
    assert_error(
        req(
            &router,
            "GET",
            "/api/daily-plan?from=notadate&to=2026-07-11",
            None,
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unparseable date",
    );
    assert_error(
        req(
            &router,
            "GET",
            "/api/daily-plan/history?preset=fortnight",
            None,
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unknown history preset",
    );

    // A parent that doesn't resolve used to silently CLEAR the parent (F-06).
    assert_error(
        req(
            &router,
            "PATCH",
            &format!("/api/work-items/{wi}"),
            Some(json!({"parent": 9999})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unresolvable parent",
    );

    // An area belongs to one project; create never checked (F-05).
    let (_st, alpha) = req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name":"alpha"})),
    )
    .await;
    let (_st, beta) = req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name":"beta"})),
    )
    .await;
    let (_st, area) = req(
        &router,
        "POST",
        "/api/areas",
        Some(json!({"project":"beta","name":"ui"})),
    )
    .await;
    assert!(beta["id"].as_i64().is_some());
    assert_error(
        req(
            &router,
            "POST",
            "/api/work-items",
            Some(json!({
                "title":"cross","content":"c",
                "project_id": alpha["id"], "area_id": area["id"]
            })),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "area from another project",
    );

    // Unknown project on area create was a 500 from fetch_one.
    assert_error(
        req(
            &router,
            "POST",
            "/api/areas",
            Some(json!({"project":"nope","name":"ui"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "area under unknown project",
    );
}

/// F-04, the sharpest edge in the review: `PATCH /api/cards/<node>` bound only
/// the node id, so pointing it at a work item archived the work item and
/// answered `{"ok":true}`. Since 0009 made `wi_number == node_id`, that is a
/// slip an agent will eventually make.
#[tokio::test]
async fn cross_kind_patches_404_and_mutate_nothing() {
    let (_c, router) = app().await;
    let wi = work_item(&router, "not a card").await;

    assert_error(
        req(
            &router,
            "PATCH",
            &format!("/api/cards/{wi}"),
            Some(json!({"archived": true, "title": "hijack"})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "card patch against a work item",
    );
    assert_error(
        req(
            &router,
            "PATCH",
            &format!("/api/proposals/{wi}"),
            Some(json!({"archived": true})),
        )
        .await,
        StatusCode::NOT_FOUND,
        "not_found",
        "proposal patch against a work item",
    );

    let (st, item) = req(&router, "GET", &format!("/api/work-items/{wi}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(item["archived"], false, "work item must be untouched");
    assert_eq!(item["title"], "not a card", "work item must be untouched");
}

/// Mutations acknowledge with the entity a read would return (WI #525), so a
/// caller never has to issue a follow-up GET to learn what it just wrote.
#[tokio::test]
async fn mutations_return_the_updated_entity() {
    let (_c, router) = app().await;

    let (_st, created) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"row me","content":"c"})),
    )
    .await;
    assert_eq!(created["title"], "row me");
    assert_eq!(created["wi_status"], "open");
    assert_eq!(created["comment_count"], 0);
    let wi = created["wi_number"].as_i64().unwrap();

    let (st, updated) = req(
        &router,
        "PATCH",
        &format!("/api/work-items/{wi}"),
        Some(json!({"wi_status":"resolved"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(updated["wi_status"], "resolved");
    assert_eq!(updated["wi_number"], wi);

    let (_st, card) = req(
        &router,
        "POST",
        "/api/cards",
        Some(json!({"title":"card","status":"Backlog"})),
    )
    .await;
    let card_node = card["node_id"].as_i64().unwrap();
    let (st, moved) = req(
        &router,
        "PATCH",
        &format!("/api/cards/{card_node}"),
        Some(json!({"status":"Active"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(moved["status"], "Active");
    assert_eq!(moved["node_id"], card_node);

    let (_st, proposal) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"p","summary":"s","work_item_numbers":[wi]})),
    )
    .await;
    assert_eq!(proposal["status"], "proposed");
    assert_eq!(proposal["covered"].as_array().unwrap().len(), 1);
    let pnode = proposal["node_id"].as_i64().unwrap();
    let (st, activated) = req(
        &router,
        "PATCH",
        &format!("/api/proposals/{pnode}"),
        Some(json!({"status":"active"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(activated["status"], "active");

    let (_st, topic) = req(&router, "POST", "/api/topics", Some(json!({"name":"T"}))).await;
    let tnode = topic["node_id"].as_i64().unwrap();
    let (st, archived) = req(
        &router,
        "POST",
        &format!("/api/topics/{tnode}/archive"),
        Some(json!({"archived": true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(archived["archived"], true);

    let (_st, link) = req(
        &router,
        "POST",
        "/api/links",
        Some(json!({"url":"https://example.com"})),
    )
    .await;
    assert_eq!(link["disposition"], "Unread");
    let lnode = link["node_id"].as_i64().unwrap();
    let (st, read) = req(
        &router,
        "PATCH",
        &format!("/api/links/{lnode}"),
        Some(json!({"read": true, "disposition": "Done"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(read["read"], true);
    assert_eq!(read["disposition"], "Done");

    let (st, project) = req(
        &router,
        "PATCH",
        "/api/projects/alpha",
        Some(json!({"status":"maintenance"})),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "unknown project (body {project:?})"
    );
    req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name":"alpha"})),
    )
    .await;
    let (st, project) = req(
        &router,
        "PATCH",
        "/api/projects/alpha",
        Some(json!({"status":"maintenance"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(project["status"], "maintenance");
    assert_eq!(project["name"], "alpha");
}

/// Sprint 014 — `relate` rejects self-edges (WI #532) and `neighbors` filters
/// server-side with an exact truncation flag (WI #533).
#[tokio::test]
async fn relationships_reject_self_edges_and_neighbors_filters() {
    let (_c, router) = app().await;
    let hub = work_item(&router, "hub").await;
    let dep = work_item(&router, "dependency").await;

    assert_error(
        req(
            &router,
            "POST",
            "/api/relationships",
            Some(json!({"left": hub, "right": hub, "label": "depends_on"})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "self-edge",
    );

    for label in ["depends_on", "related-to"] {
        let (st, _) = req(
            &router,
            "POST",
            "/api/relationships",
            Some(json!({"left": hub, "right": dep, "label": label})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{label}");
    }

    let (st, all) = req(&router, "GET", &format!("/api/nodes/{hub}/neighbors"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(all["total"], 2);
    assert_eq!(all["truncated"], false);

    // `directed` tells a reader when to ignore `direction` (D-1).
    let items = all["items"].as_array().unwrap();
    let related = items.iter().find(|n| n["label"] == "related-to").unwrap();
    assert_eq!(related["directed"], false);
    let depends = items.iter().find(|n| n["label"] == "depends_on").unwrap();
    assert_eq!(depends["directed"], true);

    let (_st, filtered) = req(
        &router,
        "GET",
        &format!("/api/nodes/{hub}/neighbors?label=depends_on"),
        None,
    )
    .await;
    assert_eq!(filtered["total"], 1);
    assert_eq!(filtered["items"][0]["label"], "depends_on");

    let (_st, clipped) = req(
        &router,
        "GET",
        &format!("/api/nodes/{hub}/neighbors?limit=1"),
        None,
    )
    .await;
    assert_eq!(clipped["items"].as_array().unwrap().len(), 1);
    assert_eq!(clipped["total"], 2, "total counts every match");
    assert_eq!(clipped["truncated"], true);
}

/// The `covers` edge a proposal writes must read proposal -> work item over
/// REST too — this is the read `start-sprint` walks (WI #531).
#[tokio::test]
async fn proposal_covers_edges_read_outward_over_rest() {
    let (_c, router) = app().await;
    let wi = work_item(&router, "covered").await;
    let (_st, proposal) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"bundle","summary":"s","work_item_numbers":[wi]})),
    )
    .await;
    let pnode = proposal["node_id"].as_i64().unwrap();

    let (_st, from_proposal) = req(
        &router,
        "GET",
        &format!("/api/nodes/{pnode}/neighbors?label=covers&kind=workitem"),
        None,
    )
    .await;
    assert_eq!(from_proposal["items"][0]["direction"], "out");
    assert_eq!(from_proposal["items"][0]["node_id"], wi);

    let (_st, from_item) = req(&router, "GET", &format!("/api/nodes/{wi}/neighbors"), None).await;
    assert_eq!(from_item["items"][0]["direction"], "in");
}

/// Sprint 015 — the §4.3 collection contract: every list is enveloped, bounded,
/// filtered, and excludes archived rows by default (WI #534, D-3).
#[tokio::test]
async fn collection_reads_are_enveloped_bounded_and_filtered() {
    let (_c, router) = app().await;
    for i in 0..3 {
        work_item(&router, &format!("item {i}")).await;
    }
    let archived = work_item(&router, "archived one").await;
    req(
        &router,
        "PATCH",
        &format!("/api/work-items/{archived}"),
        Some(json!({"archived": true})),
    )
    .await;

    // Default: envelope, documented page size, archived excluded.
    let (st, page) = req(&router, "GET", "/api/work-items", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(page["total"], 3, "archived excluded by default");
    assert_eq!(page["limit"], 200);
    assert_eq!(page["offset"], 0);
    assert_eq!(page["items"].as_array().unwrap().len(), 3);

    // …but still reachable.
    let (_st, all) = req(&router, "GET", "/api/work-items?archived=all", None).await;
    assert_eq!(all["total"], 4);
    let (_st, only) = req(&router, "GET", "/api/work-items?archived=true", None).await;
    assert_eq!(only["total"], 1);
    assert_eq!(only["items"][0]["wi_number"], archived);

    // A bad tri-state value is a 400, not a silent reinterpretation.
    assert_error(
        req(&router, "GET", "/api/work-items?archived=maybe", None).await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "archived=maybe",
    );

    // total counts every match; limit/offset page through it.
    let (_st, first) = req(&router, "GET", "/api/work-items?limit=2", None).await;
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    assert_eq!(first["total"], 3, "total is the full filtered count");
    let (_st, second) = req(&router, "GET", "/api/work-items?limit=2&offset=2", None).await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_ne!(
        first["items"][0]["wi_number"],
        second["items"][0]["wi_number"]
    );

    // Cards and links filter server-side too.
    req(
        &router,
        "POST",
        "/api/cards",
        Some(json!({"title":"backlog card","status":"Backlog"})),
    )
    .await;
    req(
        &router,
        "POST",
        "/api/cards",
        Some(json!({"title":"active card","status":"Active"})),
    )
    .await;
    let (_st, active) = req(&router, "GET", "/api/cards?status=Active", None).await;
    assert_eq!(active["total"], 1);
    assert_eq!(active["items"][0]["title"], "active card");
    assert_error(
        req(&router, "GET", "/api/cards?status=Bogus", None).await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "unknown card status filter",
    );

    let (_st, link) = req(
        &router,
        "POST",
        "/api/links",
        Some(json!({"url":"https://example.com"})),
    )
    .await;
    let lnode = link["node_id"].as_i64().unwrap();
    req(
        &router,
        "PATCH",
        &format!("/api/links/{lnode}"),
        Some(json!({"disposition":"Revisit"})),
    )
    .await;
    let (_st, revisit) = req(&router, "GET", "/api/links?disposition=Revisit", None).await;
    assert_eq!(revisit["total"], 1);
    let (_st, unread) = req(&router, "GET", "/api/links?disposition=Unread", None).await;
    assert_eq!(unread["total"], 0);
}

/// WI #535 — the two-level read contract past work items: every commentable
/// row carries an exact `comment_count`, and the REST detail read matches the
/// MCP tool's shape instead of being a bare row.
#[tokio::test]
async fn commentable_rows_signal_discussion_and_detail_inlines_it() {
    let (_c, router) = app().await;
    let wi = work_item(&router, "discussed").await;
    let (_st, card) = req(&router, "POST", "/api/cards", Some(json!({"title":"c"}))).await;
    let (_st, topic) = req(&router, "POST", "/api/topics", Some(json!({"name":"t"}))).await;
    let (_st, proposal) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"p","summary":"s","work_item_numbers":[wi]})),
    )
    .await;

    for node in [
        wi,
        card["node_id"].as_i64().unwrap(),
        topic["node_id"].as_i64().unwrap(),
        proposal["node_id"].as_i64().unwrap(),
    ] {
        req(
            &router,
            "POST",
            &format!("/api/nodes/{node}/comments"),
            Some(json!({"body":"a note"})),
        )
        .await;
    }

    let (_st, cards) = req(&router, "GET", "/api/cards", None).await;
    assert_eq!(cards["items"][0]["comment_count"], 1);
    let (_st, topics) = req(&router, "GET", "/api/topics", None).await;
    assert_eq!(topics["items"][0]["comment_count"], 1);
    let (_st, proposals) = req(&router, "GET", "/api/proposals", None).await;
    assert_eq!(proposals[0]["comment_count"], 1);
    assert_eq!(
        proposals[0]["covered_count"], 1,
        "covers signal without a join"
    );

    // REST get_work_item now returns the detail shape (same as MCP).
    let (st, detail) = req(&router, "GET", &format!("/api/work-items/{wi}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["comment_count"], 1);
    assert_eq!(detail["comments"].as_array().unwrap().len(), 1);
    assert_eq!(detail["comments_truncated"], false);

    // REST can patch category now (it was hardcoded to None).
    let (st, patched) = req(
        &router,
        "PATCH",
        &format!("/api/work-items/{wi}"),
        Some(json!({"category":"ops"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(patched["category"], "ops");
}

/// WI #536 — `get_proposal` answers "what is this sprint" in one call, which is
/// what the Planning page and start-sprint used three for.
#[tokio::test]
async fn get_proposal_returns_covered_refs_and_comments() {
    let (_c, router) = app().await;
    let a = work_item(&router, "first").await;
    let b = work_item(&router, "second").await;
    let (_st, proposal) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"bundle","summary":"s","work_item_numbers":[b, a]})),
    )
    .await;
    let pnode = proposal["node_id"].as_i64().unwrap();
    req(
        &router,
        "POST",
        &format!("/api/nodes/{pnode}/comments"),
        Some(json!({"body":"why this bundle"})),
    )
    .await;

    let (st, detail) = req(&router, "GET", &format!("/api/proposals/{pnode}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["title"], "bundle");
    let covered = detail["covered"].as_array().unwrap();
    assert_eq!(covered.len(), 2);
    assert_eq!(covered[0]["wi_number"], a, "ordered by wi_number");
    assert_eq!(covered[1]["wi_number"], b);
    assert_eq!(covered[0]["title"], "first");
    assert_eq!(covered[0]["wi_status"], "open");
    assert_eq!(covered[0]["comment_count"], 0);
    assert_eq!(detail["comments"].as_array().unwrap().len(), 1);
    assert_eq!(detail["covered_count"], 2);

    // A proposal covering nothing is empty, not an error.
    let (_st, bare) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"empty","summary":"s"})),
    )
    .await;
    let (st, bare_detail) = req(
        &router,
        "GET",
        &format!("/api/proposals/{}", bare["node_id"].as_i64().unwrap()),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(bare_detail["covered"].as_array().unwrap().is_empty());

    assert_error(
        req(&router, "GET", "/api/proposals/999999", None).await,
        StatusCode::NOT_FOUND,
        "not_found",
        "missing proposal",
    );
}

/// WI #538 — one transaction, so an invalid disposition cannot leave tags
/// applied behind it; and WI #565 — proposals filter by project.
#[tokio::test]
async fn link_update_is_atomic_and_proposals_filter_by_project() {
    let (_c, router) = app().await;
    let (_st, link) = req(
        &router,
        "POST",
        "/api/links",
        Some(json!({"url":"https://example.com","title":"Ex"})),
    )
    .await;
    let lnode = link["node_id"].as_i64().unwrap();

    assert_error(
        req(
            &router,
            "PATCH",
            &format!("/api/links/{lnode}"),
            Some(json!({"disposition":"Someday","tags":["applied"]})),
        )
        .await,
        StatusCode::BAD_REQUEST,
        "invalid_input",
        "invalid disposition",
    );
    let (_st, after) = req(&router, "GET", "/api/links", None).await;
    assert_eq!(
        after["items"][0]["disposition"], "Unread",
        "nothing changed"
    );
    assert!(
        after["items"][0]["tags"].as_array().unwrap().is_empty(),
        "the valid half of a rejected patch must not land"
    );

    // The whole patch applies together.
    let (st, updated) = req(
        &router,
        "PATCH",
        &format!("/api/links/{lnode}"),
        Some(json!({"disposition":"Summarized","read":true,"tags":["ai"]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(updated["disposition"], "Summarized");
    assert_eq!(updated["read"], true);
    assert_eq!(updated["tags"][0], "ai");

    // Proposals by project (WI #565).
    let (_st, alpha) = req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name":"alpha"})),
    )
    .await;
    req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"in alpha","summary":"s","project_id": alpha["id"]})),
    )
    .await;
    req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title":"unassigned","summary":"s"})),
    )
    .await;
    let (_st, all) = req(&router, "GET", "/api/proposals", None).await;
    assert_eq!(all.as_array().unwrap().len(), 2);
    let (_st, scoped) = req(&router, "GET", "/api/proposals?project=alpha", None).await;
    assert_eq!(scoped.as_array().unwrap().len(), 1);
    assert_eq!(scoped[0]["title"], "in alpha");
}
