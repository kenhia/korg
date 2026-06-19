//! S4 — snapshot source readers.
//!
//! Restores the frozen kwi/kcard snapshots into an ephemeral Postgres and
//! verifies the typed readers return one struct per source row (drop nothing).
//! Reader counts are checked against a direct `COUNT(*)` on the restored
//! tables, so this proves reader fidelity independent of absolute totals.

mod common;

use korg_migrate::source::{read_kcard, read_kwi};
use sqlx::{PgPool, Row};

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("count query")
        .get::<i64, _>(0)
}

#[tokio::test]
async fn read_sources_matches_row_counts() {
    let (_pg, kwi_pool, kcard_pool) = common::staged_sources().await;

    let kwi = read_kwi(&kwi_pool).await.expect("read kwi");
    assert_eq!(kwi.projects.len() as i64, count(&kwi_pool, "project").await);
    assert_eq!(kwi.areas.len() as i64, count(&kwi_pool, "area").await);
    assert_eq!(kwi.workitems.len() as i64, count(&kwi_pool, "workitem").await);
    assert_eq!(kwi.related.len() as i64, count(&kwi_pool, "related").await);

    let kcard = read_kcard(&kcard_pool).await.expect("read kcard");
    assert_eq!(kcard.cards.len() as i64, count(&kcard_pool, "cards").await);
    assert_eq!(
        kcard.comments.len() as i64,
        count(&kcard_pool, "comments").await
    );

    // Sanity: every source actually has data (guards against an empty restore).
    assert!(kwi.workitems.len() > 0, "kwi work items should be non-empty");
    assert!(kcard.cards.len() > 0, "kcard cards should be non-empty");
}
