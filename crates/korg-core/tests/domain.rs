//! M2 acceptance — generalized cross-kind relationships + reading-list links.
//!
//! In a fresh korg DB: create a work item, a card, and a reading-list link;
//! link work item <-> card and card <-> link; then prove the card's neighbors
//! span both kinds with the right labels, the link round-trips, and the first
//! work item's wi_number equals its node id (0009_identity).

use korg_core::repo::{
    create_card, create_link, create_work_item, list_links, neighbors, NewCard, NewLink,
    NewWorkItem,
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
async fn domain_cross_kind_relationships_and_reading_list() {
    let (_c, pool) = fresh_korg().await;

    let wi = create_work_item(
        &pool,
        NewWorkItem {
            project_id: None,
            area_id: None,
            wi_type: "task".into(),
            wi_status: "open".into(),
            wi_tshirt: "Unknown".into(),
            sprint: None,
            title: "Wire korg MCP".into(),
            content: "expose tools".into(),
            details: None,
            category: None,
            tags: vec!["korg".into()],
        },
    )
    .await
    .expect("create work item");
    // Since 0009_identity, wi_number IS the node id — one number everywhere.
    assert_eq!(wi.wi_number, wi.node_id, "wi_number == node_id");

    let card = create_card(
        &pool,
        NewCard {
            project_id: None,
            category: Some("research".into()),
            tags: vec![],
            status: "Active".into(),
            title: "Design unified model".into(),
            description: String::new(),
            rank: Decimal::new(1, 0),
        },
    )
    .await
    .expect("create card")
    .node_id;

    let link = create_link(
        &pool,
        NewLink {
            project_id: None,
            category: None,
            tags: vec!["read-later".into()],
            url: "https://example.com/typed-nodes".into(),
            title: Some("Typed nodes".into()),
        },
    )
    .await
    .expect("create link")
    .node_id;

    // Cross-kind edges.
    korg_core::repo::relate(&pool, wi.node_id, card, "implements")
        .await
        .expect("relate wi-card");
    korg_core::repo::relate(&pool, card, link, "references")
        .await
        .expect("relate card-link");

    // The card sees both a work item and a link as neighbors.
    let mut ns = neighbors(&pool, card).await.expect("neighbors");
    ns.sort_by(|a, b| a.kind.cmp(&b.kind));
    assert_eq!(ns.len(), 2, "card should have two neighbors");

    let kinds: Vec<&str> = ns.iter().map(|n| n.kind.as_str()).collect();
    assert!(kinds.contains(&"workitem"), "neighbor of kind workitem");
    assert!(kinds.contains(&"link"), "neighbor of kind link");

    let wi_n = ns.iter().find(|n| n.kind == "workitem").unwrap();
    assert_eq!(wi_n.node_id, wi.node_id);
    assert_eq!(wi_n.label, "implements");
    let link_n = ns.iter().find(|n| n.kind == "link").unwrap();
    assert_eq!(link_n.node_id, link);
    assert_eq!(link_n.label, "references");

    // Reading list round-trips.
    let links = list_links(&pool).await.expect("list links");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://example.com/typed-nodes");
    assert_eq!(links[0].title.as_deref(), Some("Typed nodes"));
    assert!(!links[0].read, "new link is unread");
}
