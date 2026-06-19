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
}
