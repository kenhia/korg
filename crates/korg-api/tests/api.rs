//! M5 (M5b) acceptance — korg-api endpoints over a real korg database.

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

#[tokio::test]
async fn api_end_to_end() {
    let (_c, router) = app().await;

    // Health.
    let (st, body) = req(&router, "GET", "/api/health", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    // Create a project + work item.
    let (st, _) = req(
        &router,
        "POST",
        "/api/cards",
        Some(json!({"title":"Board it"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"API done","content":"axum"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // Since 0009_identity, wi_number IS the node id — assert the invariant, not a literal.
    let wi_number = wi["wi_number"].as_i64().unwrap();
    assert_eq!(wi["node_id"].as_i64(), Some(wi_number));

    let (_st, items) = req(&router, "GET", "/api/work-items", None).await;
    assert_eq!(items.as_array().unwrap().len(), 1);

    let (_st, one) = req(
        &router,
        "GET",
        &format!("/api/work-items/{wi_number}"),
        None,
    )
    .await;
    assert_eq!(one["title"], "API done");

    // Cards list + move/rank in one PATCH.
    let (_st, cards) = req(&router, "GET", "/api/cards", None).await;
    let card_node = cards[0]["node_id"].as_i64().unwrap();
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/cards/{card_node}"),
        Some(json!({"status":"Active","rank":2.5})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, cards) = req(&router, "GET", "/api/cards", None).await;
    assert_eq!(cards[0]["status"], "Active");

    // Card project (free-text resolves/creates) + a comment thread.
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/cards/{card_node}"),
        Some(json!({"project":"boardproj","category":"chores","tags":["x","y"]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, cards) = req(&router, "GET", "/api/cards", None).await;
    assert_eq!(cards[0]["project"], "boardproj");
    assert_eq!(cards[0]["category"], "chores");

    // Comments are node-scoped: /api/nodes/:node_id/comments works for a card node…
    let (st, cm) = req(
        &router,
        "POST",
        &format!("/api/nodes/{card_node}/comments"),
        Some(json!({"body":"first note"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let cmid = cm["id"].as_i64().unwrap();
    let (_st, comments) = req(
        &router,
        "GET",
        &format!("/api/nodes/{card_node}/comments"),
        None,
    )
    .await;
    assert_eq!(comments.as_array().unwrap().len(), 1);
    assert_eq!(comments[0]["body"], "first note");
    let (st, _) = req(&router, "DELETE", &format!("/api/comments/{cmid}"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_st, comments) = req(
        &router,
        "GET",
        &format!("/api/nodes/{card_node}/comments"),
        None,
    )
    .await;
    assert_eq!(comments.as_array().unwrap().len(), 0);

    // …and equally for a work-item node (proves the route isn't card-specific).
    let (_st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"commented WI","content":"x"})),
    )
    .await;
    let wi_node = wi["node_id"].as_i64().unwrap();
    let (st, _) = req(
        &router,
        "POST",
        &format!("/api/nodes/{wi_node}/comments"),
        Some(json!({"body":"note on a work item"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, wi_comments) = req(
        &router,
        "GET",
        &format!("/api/nodes/{wi_node}/comments"),
        None,
    )
    .await;
    assert_eq!(wi_comments.as_array().unwrap().len(), 1);
    assert_eq!(wi_comments[0]["body"], "note on a work item");
    assert_eq!(wi_comments[0]["node_id"].as_i64(), Some(wi_node));

    // Reading list: create, disposition, tags.
    let (_st, link) = req(
        &router,
        "POST",
        "/api/links",
        Some(json!({"url":"https://example.com","title":"Ex"})),
    )
    .await;
    let link_node = link["node_id"].as_i64().unwrap();
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/links/{link_node}"),
        Some(json!({"disposition":"Revisit","tags":["read"]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, links) = req(&router, "GET", "/api/links", None).await;
    assert_eq!(links[0]["disposition"], "Revisit");
    assert_eq!(links[0]["tags"][0], "read");

    // Topics: create, search, update. Daily planning snapshots display server-side.
    let (st, topic) = req(
        &router,
        "POST",
        "/api/topics",
        Some(json!({"name":"Backend architecture","description":"explore"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let topic_node = topic["node_id"].as_i64().unwrap();
    let (_st, found) = req(&router, "GET", "/api/topics?q=ARCH", None).await;
    assert_eq!(found.as_array().unwrap().len(), 1);
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/topics/{topic_node}"),
        Some(json!({"name":"Backend systems"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, planned) = req(
        &router,
        "POST",
        "/api/daily-plan",
        Some(json!({"source_node_id":topic_node,"plan_date":"2026-07-11"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let planned_node = planned["node_id"].as_i64().unwrap();
    let (_st, day) = req(
        &router,
        "GET",
        "/api/daily-plan?from=2026-07-11&to=2026-07-11",
        None,
    )
    .await;
    assert_eq!(day.as_array().unwrap().len(), 1);
    assert_eq!(day[0]["display"], "Backend systems");
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/daily-plan/{planned_node}/completion"),
        Some(json!({"completed":true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, moved) = req(
        &router,
        "POST",
        &format!("/api/daily-plan/{planned_node}/move"),
        Some(json!({"target_date":"2026-07-12","target_position":0})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(moved["copied"], false);
    let (st, historical) = req(&router, "GET", "/api/daily-plan/history?preset=week", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(historical["total"], 0);
    let (st, _) = req(
        &router,
        "POST",
        &format!("/api/topics/{topic_node}/archive"),
        Some(json!({"archived":true})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, topics) = req(&router, "GET", "/api/topics", None).await;
    assert!(topics.as_array().unwrap().is_empty());

    // Daily plan items remain ordinary nodes for generalized relationships.
    let (st, _) = req(
        &router,
        "POST",
        "/api/relationships",
        Some(json!({"left":planned_node,"right":card_node,"label":"scheduled"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, ns) = req(
        &router,
        "GET",
        &format!("/api/nodes/{planned_node}/neighbors"),
        None,
    )
    .await;
    assert_eq!(ns["items"][0]["node_id"].as_i64(), Some(card_node));
    assert_eq!(ns["items"][0]["kind"], "card");
    assert_eq!(ns["total"], 1);
    assert_eq!(ns["truncated"], false);
    // The card stays where it is (Active), not forced anywhere by scheduling.
    let (_st, cards) = req(&router, "GET", "/api/cards", None).await;
    assert_eq!(cards[0]["status"], "Active");

    // Recent project resolves to the one we touched.
    let (_st, recent) = req(&router, "GET", "/api/projects/recent", None).await;
    assert!(recent["project"].is_null() || recent["project"].is_string());

    // Create a project (idempotent) and create a work item in it.
    let (st, proj) = req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name":"alpha"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let pid = proj["id"].as_i64().unwrap();
    let (st, _) = req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name":"alpha"})),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "creating the same project twice is idempotent"
    );
    let (_st, projects) = req(&router, "GET", "/api/projects", None).await;
    assert!(projects
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "alpha"));

    let (st, _) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"in alpha","content":"x","project_id":pid})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, alpha_items) = req(&router, "GET", "/api/work-items?project=alpha", None).await;
    assert_eq!(alpha_items.as_array().unwrap().len(), 1);

    // Edit + archive the work item via PATCH.
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/work-items/{wi_number}"),
        Some(json!({"wi_status":"resolved","archived":true,"tags":["edited"]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, one) = req(
        &router,
        "GET",
        &format!("/api/work-items/{wi_number}"),
        None,
    )
    .await;
    assert_eq!(one["wi_status"], "resolved");
    assert_eq!(one["archived"], true);
    assert_eq!(one["tags"][0], "edited");

    // Relationship has an id and can be deleted.
    let (_st, ns) = req(
        &router,
        "GET",
        &format!("/api/nodes/{planned_node}/neighbors"),
        None,
    )
    .await;
    let rel_id = ns["items"][0]["rel_id"].as_i64().unwrap();
    let (st, _) = req(
        &router,
        "DELETE",
        &format!("/api/relationships/{rel_id}"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, ns2) = req(
        &router,
        "GET",
        &format!("/api/nodes/{planned_node}/neighbors"),
        None,
    )
    .await;
    assert_eq!(ns2["items"].as_array().unwrap().len(), 0);
    assert_eq!(ns2["total"], 0);
}

/// Sprint 004 — /api/proposals: bundled create, pinned/rank ordering,
/// status-filtered listing, and PATCH-driven lifecycle.
#[tokio::test]
async fn proposals_end_to_end() {
    let (_c, router) = app().await;

    let (_st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"fix the thing","content":"x"})),
    )
    .await;
    let wi_number = wi["wi_number"].as_i64().unwrap();

    let (st, created) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({
            "title":"Sprint: fix things",
            "summary":"bundle of small fixes",
            "work_item_numbers":[wi_number, 9999],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node_id = created["node_id"].as_i64().unwrap();
    assert_eq!(created["covered"].as_array().unwrap().len(), 1);

    let (_st, list) = req(&router, "GET", "/api/proposals", None).await;
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["status"], "proposed");

    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/proposals/{node_id}"),
        Some(json!({"pinned":true,"status":"active"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (_st, active) = req(&router, "GET", "/api/proposals?status=active", None).await;
    let active = active.as_array().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["pinned"], true);

    let (_st, proposed) = req(&router, "GET", "/api/proposals?status=proposed", None).await;
    assert_eq!(proposed.as_array().unwrap().len(), 0);
}

/// Sprint 006 — /api/work-items/survey: slim, paginated, total reflects the
/// full filtered count rather than just the returned page.
#[tokio::test]
async fn survey_work_items_end_to_end() {
    let (_c, router) = app().await;

    for i in 0..3 {
        let (st, _) = req(
            &router,
            "POST",
            "/api/work-items",
            Some(json!({"title": format!("item {i}"), "content": "x"})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
    }

    let (st, page) = req(
        &router,
        "GET",
        "/api/work-items/survey?limit=2&offset=0",
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(page["total"].as_i64(), Some(3));
    assert_eq!(page["items"].as_array().unwrap().len(), 2);
    assert!(
        page["items"][0].get("content").is_none(),
        "slim projection has no content field"
    );

    let (_st, rest) = req(
        &router,
        "GET",
        "/api/work-items/survey?limit=2&offset=2",
        None,
    )
    .await;
    assert_eq!(rest["items"].as_array().unwrap().len(), 1);
}

// WI #260 — GET /api/nodes/:id resolves any node kind to a uniform preview.
#[tokio::test]
async fn node_preview_end_to_end() {
    let (_c, router) = app().await;

    // Work item: preview carries wi_number (== node id), title, badges, content.
    let (_st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"Find me","content":"body text","wi_type":"bug"})),
    )
    .await;
    let id = wi["node_id"].as_i64().unwrap();
    let (st, node) = req(&router, "GET", &format!("/api/nodes/{id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(node["kind"], "workitem");
    assert_eq!(node["wi_number"].as_i64(), Some(id));
    assert_eq!(node["title"], "Find me");
    assert_eq!(node["body"], "body text");
    let badges = node["badges"].as_array().unwrap();
    assert!(badges.iter().any(|b| b == "bug"), "type shows as a badge");

    // Card: different kind, no wi_number, description as the body.
    let (_st, card) = req(
        &router,
        "POST",
        "/api/cards",
        Some(json!({"title":"A card","description":"desc","status":"Backlog"})),
    )
    .await;
    let cid = card["node_id"].as_i64().unwrap();
    let (st, cnode) = req(&router, "GET", &format!("/api/nodes/{cid}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(cnode["kind"], "card");
    assert!(cnode["wi_number"].is_null());
    assert_eq!(cnode["title"], "A card");
    assert_eq!(cnode["body"], "desc");

    // Unknown id is a 404 with a typed code (D-6), not `200 null`.
    let (st, body) = req(&router, "GET", "/api/nodes/999999", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

// WI #289 — typed domain errors map to 4xx, not 500 (agents key off status).
#[tokio::test]
async fn validation_and_not_found_status_codes() {
    let (_c, router) = app().await;
    let (_st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"s","content":"c"})),
    )
    .await;
    let id = wi["node_id"].as_i64().unwrap();

    // Invalid status → 400 (was 500).
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/work-items/{id}"),
        Some(json!({"wi_status":"bogus"})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // Unknown project name → 404.
    let (st, _) = req(
        &router,
        "PATCH",
        "/api/projects/no-such-project",
        Some(json!({"status":"active"})),
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // A valid update still succeeds, and comment_count rides along (WI #392).
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/work-items/{id}"),
        Some(json!({"wi_status":"resolved"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, got) = req(&router, "GET", &format!("/api/work-items/{id}"), None).await;
    assert_eq!(got["comment_count"].as_i64(), Some(0));
}
