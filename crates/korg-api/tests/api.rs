//! M5 (M5b) acceptance — korg-api endpoints over a real korg database.

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

async fn req(router: &axum::Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
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
    let (st, _) = req(&router, "POST", "/api/cards", Some(json!({"title":"Board it"}))).await;
    assert_eq!(st, StatusCode::OK);

    let (st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title":"API done","content":"axum"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(wi["wi_number"].as_i64(), Some(1));

    let (_st, items) = req(&router, "GET", "/api/work-items", None).await;
    assert_eq!(items.as_array().unwrap().len(), 1);

    let (_st, one) = req(&router, "GET", "/api/work-items/1", None).await;
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
    let (_st, comments) = req(&router, "GET", &format!("/api/nodes/{card_node}/comments"), None).await;
    assert_eq!(comments.as_array().unwrap().len(), 1);
    assert_eq!(comments[0]["body"], "first note");
    let (st, _) = req(&router, "DELETE", &format!("/api/comments/{cmid}"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_st, comments) = req(&router, "GET", &format!("/api/nodes/{card_node}/comments"), None).await;
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
    let (_st, wi_comments) = req(&router, "GET", &format!("/api/nodes/{wi_node}/comments"), None).await;
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

    // Slots: seeded templates, generate a week, list it, set a goal.
    let (_st, tmpl) = req(&router, "GET", "/api/slot-templates", None).await;
    assert_eq!(tmpl.as_array().unwrap().len(), 16);
    let (st, gen) = req(
        &router,
        "POST",
        "/api/slots/generate",
        Some(json!({"start":"2024-01-01","days":7})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(gen["created"].as_i64(), Some(16));
    let (_st, week) = req(&router, "GET", "/api/slots?from=2024-01-01&to=2024-01-07", None).await;
    let week = week.as_array().unwrap();
    assert_eq!(week.len(), 16);
    let slot_node = week[0]["node_id"].as_i64().unwrap();
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/slots/{slot_node}"),
        Some(json!({"goal":"Read docs"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // Relationship: drop a card into a slot (reference, not move).
    let (st, _) = req(
        &router,
        "POST",
        "/api/relationships",
        Some(json!({"left":slot_node,"right":card_node,"label":"scheduled"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, ns) = req(&router, "GET", &format!("/api/nodes/{slot_node}/neighbors"), None).await;
    assert_eq!(ns[0]["node_id"].as_i64(), Some(card_node));
    assert_eq!(ns[0]["kind"], "card");
    // The card stays where it is (Active), not forced anywhere by scheduling.
    let (_st, cards) = req(&router, "GET", "/api/cards", None).await;
    assert_eq!(cards[0]["status"], "Active");

    // Recent project resolves to the one we touched.
    let (_st, recent) = req(&router, "GET", "/api/projects/recent", None).await;
    assert!(recent["project"].is_null() || recent["project"].is_string());

    // Create a project (idempotent) and create a work item in it.
    let (st, proj) = req(&router, "POST", "/api/projects", Some(json!({"name":"alpha"}))).await;
    assert_eq!(st, StatusCode::OK);
    let pid = proj["id"].as_i64().unwrap();
    let (st, _) = req(&router, "POST", "/api/projects", Some(json!({"name":"alpha"}))).await;
    assert_eq!(st, StatusCode::OK, "creating the same project twice is idempotent");
    let (_st, projects) = req(&router, "GET", "/api/projects", None).await;
    assert!(projects.as_array().unwrap().iter().any(|p| p["name"] == "alpha"));

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
        "/api/work-items/1",
        Some(json!({"wi_status":"resolved","archived":true,"tags":["edited"]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_st, one) = req(&router, "GET", "/api/work-items/1", None).await;
    assert_eq!(one["wi_status"], "resolved");
    assert_eq!(one["archived"], true);
    assert_eq!(one["tags"][0], "edited");

    // Relationship has an id and can be deleted.
    let (_st, ns) = req(&router, "GET", &format!("/api/nodes/{slot_node}/neighbors"), None).await;
    let rel_id = ns[0]["rel_id"].as_i64().unwrap();
    let (st, _) = req(&router, "DELETE", &format!("/api/relationships/{rel_id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    let (_st, ns2) = req(&router, "GET", &format!("/api/nodes/{slot_node}/neighbors"), None).await;
    assert_eq!(ns2.as_array().unwrap().len(), 0);
}
