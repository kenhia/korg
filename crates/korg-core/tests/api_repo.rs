//! M5a — repo support for the web API: link dispositions/tags, card move+rank,
//! project creation, and project-scoped / recent work-item queries.

use korg_core::repo::{
    create_card, create_link, create_project, create_work_item, list_cards, list_links,
    list_work_items_by_project, neighbors, recent_project, relate, set_link_disposition,
    set_node_tags, update_card, CardPatch, NewCard, NewLink, NewWorkItem,
};
use korg_test_support::{fresh_korg, new};
use rust_decimal::Decimal;

fn wi(title: &str, project_id: i64) -> NewWorkItem {
    NewWorkItem {
        project_id: Some(project_id),
        content: "x".into(),
        ..new::work_item(title)
    }
}

#[tokio::test]
async fn api_repo_links_cards_projects() {
    let (_c, pool) = fresh_korg().await;

    // Reading-list link: default Unread, then disposition + tags.
    let link = create_link(
        &pool,
        NewLink {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            url: "https://example.com".into(),
            title: Some("Ex".into()),
        },
    )
    .await
    .unwrap()
    .node_id;
    assert_eq!(
        list_links(&pool, Default::default()).await.unwrap().items[0].disposition,
        "Unread"
    );
    set_link_disposition(&pool, link, "Revisit").await.unwrap();
    set_node_tags(&pool, link, &["rust".into(), "mcp".into()])
        .await
        .unwrap();
    let links = list_links(&pool, Default::default()).await.unwrap().items;
    assert_eq!(links[0].disposition, "Revisit");
    assert_eq!(links[0].tags, vec!["rust".to_string(), "mcp".to_string()]);

    // Card move + rank in one update.
    let card = create_card(
        &pool,
        NewCard {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            status: "Backlog".into(),
            title: "Move me".into(),
            description: String::new(),
            rank: Decimal::new(10, 0),
        },
    )
    .await
    .unwrap()
    .node_id;
    update_card(
        &pool,
        card,
        CardPatch {
            status: Some("Active".into()),
            rank: Some(Decimal::new(25, 1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let cards = list_cards(&pool, Default::default()).await.unwrap().items;
    assert_eq!(cards[0].status, "Active");
    assert_eq!(cards[0].rank, Decimal::new(25, 1));

    // Projects + recent/by-project work items.
    let alpha = create_project(&pool, "alpha").await.unwrap();
    let beta = create_project(&pool, "beta").await.unwrap();
    create_work_item(&pool, wi("a1", alpha)).await.unwrap();
    create_work_item(&pool, wi("b1", beta)).await.unwrap();
    // beta touched most recently -> recent project.
    assert_eq!(
        recent_project(&pool).await.unwrap().as_deref(),
        Some("beta")
    );
    let a_items = list_work_items_by_project(&pool, "alpha").await.unwrap();
    assert_eq!(a_items.len(), 1);
    assert_eq!(a_items[0].title, "a1");
    assert_eq!(a_items[0].project.as_deref(), Some("alpha"));
}

/// WI #84 — `relate` must be idempotent and symmetric so the Link Up page can
/// re-link freely without spawning duplicate edges. Dedup is scoped per label.
#[tokio::test]
async fn relate_idempotent() {
    let (_c, pool) = fresh_korg().await;
    let p = create_project(&pool, "proj").await.unwrap();
    let a = create_work_item(&pool, wi("a", p)).await.unwrap().node_id;
    let b = create_work_item(&pool, wi("b", p)).await.unwrap().node_id;

    // Edges are DIRECTED (sprint 008): exact duplicates dedup, the reverse
    // orientation is a distinct edge, and neighbors reports which end you are.
    relate(&pool, a, b, "related-to").await.unwrap();
    relate(&pool, a, b, "related-to").await.unwrap();

    let na = neighbors(&pool, a, Default::default()).await.unwrap().items;
    assert_eq!(na.len(), 1, "exact duplicate relate must not add edges");
    assert_eq!(na[0].node_id, b);
    assert_eq!(na[0].direction, "out", "a is the edge's left");
    let nb = neighbors(&pool, b, Default::default()).await.unwrap().items;
    assert_eq!(nb.len(), 1, "the edge is visible from the other end too");
    assert_eq!(nb[0].direction, "in", "b is the edge's right");

    relate(&pool, b, a, "related-to").await.unwrap();
    assert_eq!(
        neighbors(&pool, a, Default::default())
            .await
            .unwrap()
            .items
            .len(),
        2,
        "reverse orientation is a distinct directed edge"
    );

    // A different label between the same pair is a distinct edge.
    relate(&pool, a, b, "scheduled").await.unwrap();
    let na2 = neighbors(&pool, a, Default::default()).await.unwrap().items;
    assert_eq!(na2.len(), 3, "dedup is scoped per (orientation, label)");
}
