//! M4-S1 — serializable read APIs over the korg domain.

use korg_core::repo::{
    create_card, create_work_item, get_work_item, list_cards, list_projects, list_work_items,
    NewCard, NewWorkItem,
};
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;

async fn fresh_korg() -> (impl Sized, PgPool) {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("connect");
    korg_core::migrator().run(&pool).await.expect("migrate");
    (container, pool)
}

#[tokio::test]
async fn reads_roundtrip_work_items_cards_projects() {
    let (_c, pool) = fresh_korg().await;

    let wi = create_work_item(
        &pool,
        NewWorkItem {
            project_id: None,
            area_id: None,
            wi_type: "feature".into(),
            wi_status: "open".into(),
            wi_tshirt: "M".into(),
            sprint: Some("s1".into()),
            title: "Build korg-mcp".into(),
            content: "expose tools".into(),
            details: Some("rmcp".into()),
            category: Some("infra".into()),
            tags: vec!["mcp".into(), "rust".into()],
        },
    )
    .await
    .expect("create wi");

    create_card(
        &pool,
        NewCard {
            project_id: None,
            category: None,
            tags: vec![],
            status: "Backlog".into(),
            title: "Sketch board".into(),
            description: "later".into(),
            rank: Decimal::new(5, 1),
        },
    )
    .await
    .expect("create card");

    let items = list_work_items(&pool, Default::default())
        .await
        .expect("list wi")
        .items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].wi_number, wi.wi_number);
    assert_eq!(items[0].title, "Build korg-mcp");
    assert_eq!(items[0].tags, vec!["mcp".to_string(), "rust".to_string()]);

    let got = get_work_item(&pool, wi.wi_number)
        .await
        .expect("get wi")
        .expect("wi present");
    assert_eq!(got.sprint.as_deref(), Some("s1"));
    assert!(get_work_item(&pool, 9999)
        .await
        .expect("get missing")
        .is_none());

    let cards = list_cards(&pool, Default::default())
        .await
        .expect("list cards")
        .items;
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].status, "Backlog");

    // Read views serialize cleanly (the MCP layer depends on this).
    let _ = serde_json::to_value(&items).expect("serialize work items");
    let _ = serde_json::to_value(&cards).expect("serialize cards");
    let _ = list_projects(&pool).await.expect("list projects");
}
