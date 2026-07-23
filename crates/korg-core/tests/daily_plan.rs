use korg_core::daily_plan::{
    create_item, delete_item, history, list_items, move_item, reorder_day, set_completion,
    LifecycleContext,
};
use korg_core::repo::{
    create_card, create_link, create_work_item, NewCard, NewLink, NewWorkItem, WorkItemPatch,
};
use korg_core::topics::{
    archive_topic, create_topic, get_topic, list_topics, update_topic, NewTopic, TopicPatch,
};
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;
use time::macros::{date, datetime};

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

fn ctx() -> LifecycleContext {
    LifecycleContext {
        today: date!(2026 - 07 - 11),
        now: datetime!(2026-07-11 16:30 UTC),
    }
}

fn topic(name: &str) -> NewTopic {
    NewTopic {
        project_id: None,
        project: None,
        category: None,
        tags: vec![],
        name: name.into(),
        description: None,
    }
}

async fn work_item(pool: &PgPool, title: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            project_id: None,
            project: None,
            area_id: None,
            area: None,
            wi_type: "task".into(),
            wi_status: "open".into(),
            wi_tshirt: "S".into(),
            sprint: None,
            title: title.into(),
            content: String::new(),
            details: None,
            category: None,
            tags: vec![],
        },
    )
    .await
    .unwrap()
    .node_id
}

#[tokio::test]
async fn topic_crud_search_and_archive() {
    let (_c, pool) = fresh_korg().await;
    assert!(create_topic(&pool, topic("   ")).await.is_err());
    let id = create_topic(&pool, topic("Rust async"))
        .await
        .unwrap()
        .node_id;
    let got = get_topic(&pool, id).await.unwrap().unwrap();
    assert_eq!(got.name, "Rust async");
    assert!(!got.archived);
    assert_eq!(
        list_topics(&pool, Default::default())
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        list_topics(
            &pool,
            korg_core::topics::TopicQuery {
                q: Some("ASY".into()),
                ..Default::default()
            }
        )
        .await
        .unwrap()
        .items
        .len(),
        1
    );

    update_topic(
        &pool,
        id,
        TopicPatch {
            name: Some("Rust async updated".into()),
            description: Some(Some("notes".into())),
            category: Some(Some("learning".into())),
            tags: Some(vec!["rust".into()]),
        },
    )
    .await
    .unwrap();
    let got = get_topic(&pool, id).await.unwrap().unwrap();
    assert_eq!(got.description.as_deref(), Some("notes"));
    assert_eq!(got.category.as_deref(), Some("learning"));

    archive_topic(&pool, id, true).await.unwrap();
    assert!(list_topics(&pool, Default::default())
        .await
        .unwrap()
        .items
        .is_empty());
    assert!(list_topics(
        &pool,
        korg_core::topics::TopicQuery {
            q: Some("rust".into()),
            ..Default::default()
        }
    )
    .await
    .unwrap()
    .items
    .is_empty());
    assert!(get_topic(&pool, id).await.unwrap().unwrap().archived);
}

#[tokio::test]
async fn planning_snapshots_orders_duplicates_and_validates_sources() {
    let (_c, pool) = fresh_korg().await;
    let wi = work_item(&pool, "Original title").await;
    let t = create_topic(&pool, topic("Explore")).await.unwrap().node_id;
    let card = create_card(
        &pool,
        NewCard {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            status: "Backlog".into(),
            title: "Card source".into(),
            description: String::new(),
            rank: Decimal::ZERO,
        },
    )
    .await
    .unwrap()
    .node_id;
    let link = create_link(
        &pool,
        NewLink {
            project_id: None,
            project: None,
            category: None,
            tags: vec![],
            url: "https://example.com".into(),
            title: None,
        },
    )
    .await
    .unwrap()
    .node_id;

    let a = create_item(&pool, wi, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap()
        .node_id;
    let b = create_item(&pool, t, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap()
        .node_id;
    let c = create_item(&pool, wi, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap()
        .node_id;
    create_item(&pool, card, date!(2026 - 07 - 12), &ctx())
        .await
        .unwrap();
    assert!(create_item(&pool, link, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap_err()
        .to_string()
        .contains("source kind"));
    assert!(create_item(&pool, 999_999, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap_err()
        .to_string()
        .contains("source"));

    korg_core::repo::update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            title: Some("Renamed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let items = list_items(&pool, date!(2026 - 07 - 11), date!(2026 - 07 - 11))
        .await
        .unwrap();
    assert_eq!(
        items.iter().map(|i| i.node_id).collect::<Vec<_>>(),
        vec![a, b, c]
    );
    assert_eq!(
        items[0].display, "Original title",
        "snapshot survives rename"
    );
    assert_eq!(
        items[0].source_title, "Renamed",
        "current title is resolved"
    );
    assert_eq!(items[0].source_kind, "workitem");
    assert_eq!(
        items.iter().filter(|i| i.source_node_id == wi).count(),
        2,
        "duplicates allowed"
    );
}

#[tokio::test]
async fn completion_reorder_delete_move_copy_and_frozen_history() {
    let (_c, pool) = fresh_korg().await;
    let source = work_item(&pool, "Plan me").await;
    let past = create_item(
        &pool,
        source,
        date!(2026 - 07 - 10),
        &LifecycleContext {
            today: date!(2026 - 07 - 10),
            now: datetime!(2026-07-10 09:00 UTC),
        },
    )
    .await
    .unwrap()
    .node_id;
    let a = create_item(&pool, source, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap()
        .node_id;
    let b = create_item(&pool, source, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap()
        .node_id;

    set_completion(&pool, past, true, &ctx()).await.unwrap();
    assert_eq!(
        list_items(&pool, date!(2026 - 07 - 10), date!(2026 - 07 - 10))
            .await
            .unwrap()[0]
            .completed_at,
        Some(ctx().now)
    );
    set_completion(&pool, past, false, &ctx()).await.unwrap();

    reorder_day(&pool, date!(2026 - 07 - 11), &[b, a], &ctx())
        .await
        .unwrap();
    assert_eq!(
        list_items(&pool, date!(2026 - 07 - 11), date!(2026 - 07 - 11))
            .await
            .unwrap()
            .iter()
            .map(|i| i.node_id)
            .collect::<Vec<_>>(),
        vec![b, a]
    );
    assert!(reorder_day(&pool, date!(2026 - 07 - 10), &[past], &ctx())
        .await
        .unwrap_err()
        .to_string()
        .contains("past"));
    assert!(delete_item(&pool, past, &ctx())
        .await
        .unwrap_err()
        .to_string()
        .contains("past"));

    let moved = move_item(&pool, a, date!(2026 - 07 - 12), 0, &ctx())
        .await
        .unwrap();
    assert!(!moved.copied);
    assert_eq!(moved.node_id, a);
    let copied = move_item(&pool, past, date!(2026 - 07 - 12), 0, &ctx())
        .await
        .unwrap();
    assert!(copied.copied);
    assert_ne!(copied.node_id, past);
    assert!(move_item(&pool, b, date!(2026 - 07 - 10), 0, &ctx())
        .await
        .unwrap_err()
        .to_string()
        .contains("target"));

    delete_item(&pool, b, &ctx()).await.unwrap();
    assert!(
        !list_items(&pool, date!(2026 - 07 - 11), date!(2026 - 07 - 11))
            .await
            .unwrap()
            .iter()
            .any(|i| i.node_id == b)
    );
}

#[tokio::test]
async fn history_includes_all_filters_and_rejects_today() {
    let (_c, pool) = fresh_korg().await;
    let one = work_item(&pool, "One").await;
    let two = work_item(&pool, "Two").await;
    let old = LifecycleContext {
        today: date!(2026 - 07 - 09),
        now: datetime!(2026-07-09 10:00 UTC),
    };
    let completed = create_item(&pool, one, date!(2026 - 07 - 09), &old)
        .await
        .unwrap()
        .node_id;
    create_item(
        &pool,
        two,
        date!(2026 - 07 - 10),
        &LifecycleContext {
            today: date!(2026 - 07 - 10),
            now: datetime!(2026-07-10 10:00 UTC),
        },
    )
    .await
    .unwrap();
    create_item(&pool, one, date!(2026 - 07 - 11), &ctx())
        .await
        .unwrap();
    set_completion(&pool, completed, true, &ctx())
        .await
        .unwrap();

    let all = history(
        &pool,
        date!(2026 - 07 - 09),
        date!(2026 - 07 - 10),
        None,
        &ctx(),
    )
    .await
    .unwrap();
    assert_eq!(all.total, 2);
    assert_eq!(all.completed, 1);
    assert_eq!(all.completion_rate, 0.5);
    assert_eq!(all.items.len(), 2);
    let filtered = history(
        &pool,
        date!(2026 - 07 - 09),
        date!(2026 - 07 - 10),
        Some(one),
        &ctx(),
    )
    .await
    .unwrap();
    assert_eq!(filtered.total, 1);
    assert!(history(
        &pool,
        date!(2026 - 07 - 09),
        date!(2026 - 07 - 11),
        None,
        &ctx()
    )
    .await
    .unwrap_err()
    .to_string()
    .contains("before today"));
    assert!(history(
        &pool,
        date!(2026 - 07 - 10),
        date!(2026 - 07 - 09),
        None,
        &ctx()
    )
    .await
    .is_err());
}
