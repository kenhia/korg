//! Sprint 004 — sprint_proposal: bundled creation (proposal + `covers` edges
//! in one transaction), pinned-first/rank ordering, status filtering, patch.

use korg_core::repo::{
    create_proposal, create_work_item, list_proposals, node_id_for_wi, update_proposal,
    NewProposal, NewWorkItem, ProposalPatch,
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

fn wi(title: &str) -> NewWorkItem {
    NewWorkItem {
        project_id: None,
        area_id: None,
        wi_type: "task".into(),
        wi_status: "open".into(),
        wi_tshirt: "Unknown".into(),
        sprint: None,
        title: title.into(),
        content: "x".into(),
        details: None,
        category: None,
        tags: vec![],
    }
}

fn proposal(title: &str, rank: i64, covers: Vec<i64>) -> NewProposal {
    NewProposal {
        project_id: None,
        category: None,
        tags: vec![],
        title: title.into(),
        summary: "because reasons".into(),
        rank: Decimal::new(rank, 0),
        pinned: false,
        covers,
    }
}

#[tokio::test]
async fn create_proposal_bundles_covers_edges() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("a")).await.unwrap().wi_number;
    let b = create_work_item(&pool, wi("b")).await.unwrap().wi_number;

    // One call creates the proposal AND both `covers` edges; a wi_number that
    // doesn't resolve (999) is silently dropped rather than erroring.
    let r = create_proposal(&pool, proposal("Sprint: fix things", 0, vec![a, b, 999]))
        .await
        .unwrap();
    assert_eq!(r.covered.len(), 2, "only the two real wi_numbers resolve");

    let a_node = node_id_for_wi(&pool, a).await.unwrap().unwrap();
    let b_node = node_id_for_wi(&pool, b).await.unwrap().unwrap();
    assert!(r.covered.contains(&a_node));
    assert!(r.covered.contains(&b_node));

    let ns = korg_core::repo::neighbors(&pool, r.node_id).await.unwrap();
    assert_eq!(ns.len(), 2);
    assert!(ns.iter().all(|n| n.label == "covers"));
}

#[tokio::test]
async fn list_proposals_pinned_first_then_rank_and_status_filter() {
    let (_c, pool) = fresh_korg().await;
    let low = create_proposal(&pool, proposal("low rank", 1, vec![]))
        .await
        .unwrap();
    let high = create_proposal(&pool, proposal("high rank", 5, vec![]))
        .await
        .unwrap();
    let pinned = create_proposal(&pool, proposal("pinned but high rank", 9, vec![]))
        .await
        .unwrap();
    update_proposal(
        &pool,
        pinned.node_id,
        ProposalPatch {
            pinned: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let all = list_proposals(&pool, None).await.unwrap();
    let order: Vec<i64> = all.iter().map(|p| p.node_id).collect();
    assert_eq!(
        order,
        vec![pinned.node_id, low.node_id, high.node_id],
        "pinned sorts first regardless of rank, then ascending rank"
    );
    assert!(
        all.iter().all(|p| p.status == "proposed"),
        "default status is proposed"
    );

    update_proposal(
        &pool,
        low.node_id,
        ProposalPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let active = list_proposals(&pool, Some("active")).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].node_id, low.node_id);
    let still_proposed = list_proposals(&pool, Some("proposed")).await.unwrap();
    assert_eq!(
        still_proposed.len(),
        2,
        "status filter excludes the now-active one"
    );
}

#[tokio::test]
async fn update_proposal_patches_only_given_fields() {
    let (_c, pool) = fresh_korg().await;
    let p = create_proposal(&pool, proposal("draft", 0, vec![]))
        .await
        .unwrap();

    update_proposal(
        &pool,
        p.node_id,
        ProposalPatch {
            summary: Some("updated summary".into()),
            rank: Some(Decimal::new(3, 0)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let got = list_proposals(&pool, None).await.unwrap();
    let got = &got[0];
    assert_eq!(got.title, "draft", "untouched field preserved");
    assert_eq!(got.summary, "updated summary");
    assert_eq!(got.rank, Decimal::new(3, 0));
    assert!(!got.pinned, "untouched pinned stays false");
}
