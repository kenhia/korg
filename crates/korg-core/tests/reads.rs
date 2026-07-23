//! M4-S1 — serializable read APIs over the korg domain.

use korg_core::repo::{
    create_card, create_work_item, get_work_item, list_cards, list_projects, list_work_items,
    NewCard, NewWorkItem,
};
use korg_test_support::{fresh_korg, new};
use rust_decimal::Decimal;

#[tokio::test]
async fn reads_roundtrip_work_items_cards_projects() {
    let (_c, pool) = fresh_korg().await;

    let wi = create_work_item(
        &pool,
        NewWorkItem {
            wi_type: "feature".into(),
            wi_tshirt: "M".into(),
            sprint: Some("s1".into()),
            content: "expose tools".into(),
            details: Some("rmcp".into()),
            category: Some("infra".into()),
            tags: vec!["mcp".into(), "rust".into()],
            ..new::work_item("Build korg-mcp")
        },
    )
    .await
    .expect("create wi");

    create_card(
        &pool,
        NewCard {
            project_id: None,
            project: None,
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
