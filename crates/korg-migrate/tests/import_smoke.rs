//! S5 — importer smoke test.
//!
//! Restores both snapshots, runs the importer into a fresh migrated korg DB,
//! and asserts korg's row counts equal the source counts (F1 in miniature)
//! plus the wi_number sequence is advanced to max+1.

mod common;

use korg_migrate::import::import;
use korg_migrate::source::{read_kcard, read_kwi};
use sqlx::{PgPool, Row};

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("count")
        .get::<i64, _>(0)
}

#[tokio::test]
async fn import_smoke_counts_match_sources() {
    if common::skip_snapshot_suite("import_smoke") {
        return;
    }
    let (pg, kwi_pool, kcard_pool) = common::staged_sources().await;
    let korg = common::migrate_korg(&pg).await;

    let kwi = read_kwi(&kwi_pool).await.expect("read kwi");
    let kcard = read_kcard(&kcard_pool).await.expect("read kcard");

    let report = import(&kwi, &kcard, &korg).await.expect("import");

    // korg nodes split into work items + cards.
    assert_eq!(
        count(&korg, "workitem").await,
        kwi.workitems.len() as i64,
        "work item count"
    );
    assert_eq!(
        count(&korg, "card").await,
        kcard.cards.len() as i64,
        "card count"
    );
    assert_eq!(
        count(&korg, "comment").await,
        kcard.comments.len() as i64,
        "comment count"
    );
    assert_eq!(
        count(&korg, "relationship").await,
        kwi.related.len() as i64,
        "relationship count"
    );
    assert_eq!(
        count(&korg, "node").await,
        (kwi.workitems.len() + kcard.cards.len()) as i64,
        "node count = work items + cards"
    );
    assert_eq!(
        count(&korg, "area").await,
        kwi.areas.len() as i64,
        "area count"
    );

    // Projects merged by name: union of kwi projects and kcard card projects.
    let mut names: std::collections::HashSet<String> =
        kwi.projects.iter().map(|p| p.project.clone()).collect();
    for c in &kcard.cards {
        if let Some(p) = &c.project {
            names.insert(p.clone());
        }
    }
    assert_eq!(
        count(&korg, "project").await,
        names.len() as i64,
        "merged project count"
    );

    // 0009_identity: the single node sequence continues past the imported max
    // (node ids ARE wi_numbers now).
    let next: i64 = sqlx::query("SELECT nextval(pg_get_serial_sequence('node','id'))")
        .fetch_one(&korg)
        .await
        .expect("nextval")
        .get(0);
    assert!(
        next > report.max_wi_number,
        "node sequence past imported max"
    );
}
