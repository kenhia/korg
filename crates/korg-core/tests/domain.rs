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
use korg_test_support::{fresh_korg, new};
use rust_decimal::Decimal;

#[tokio::test]
async fn domain_cross_kind_relationships_and_reading_list() {
    let (_c, pool) = fresh_korg().await;

    let wi = create_work_item(
        &pool,
        NewWorkItem {
            content: "expose tools".into(),
            tags: vec!["korg".into()],
            ..new::work_item("Wire korg MCP")
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
            project: None,
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
            project: None,
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
    let mut ns = neighbors(&pool, card, Default::default())
        .await
        .expect("neighbors")
        .items;
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
    let links = list_links(&pool, Default::default())
        .await
        .expect("list links")
        .items;
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].url, "https://example.com/typed-nodes");
    assert_eq!(links[0].title.as_deref(), Some("Typed nodes"));
    assert!(!links[0].read, "new link is unread");
}
