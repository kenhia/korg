//! Sprint 005 — daily reports: upsert semantics (same-day replace keeps
//! node_id), finding edges, list ordering, get with findings.

use korg_core::repo::{
    create_work_item, get_report, list_reports, upsert_report, NewReport, NewWorkItem,
};
use korg_test_support::{fresh_korg, new};
use time::macros::date;

fn wi(title: &str) -> NewWorkItem {
    NewWorkItem {
        wi_tshirt: "S".into(),
        content: "c".into(),
        ..new::work_item(title)
    }
}

fn report(day: time::Date, status: &str, findings: Vec<i64>) -> NewReport {
    NewReport {
        source: "kmon".into(),
        report_date: day,
        status: status.into(),
        summary: format!("summary for {day}"),
        body: "## status\nfull body".into(),
        model: Some("gemma-4-31b-it-awq".into()),
        escalated: false,
        findings,
    }
}

#[tokio::test]
async fn upsert_replaces_same_day_and_keeps_node_id() {
    let (_c, pool) = fresh_korg().await;
    let w = create_work_item(&pool, wi("backup broken")).await.unwrap();

    let first = upsert_report(
        &pool,
        report(date!(2026 - 07 - 04), "problem", vec![w.wi_number]),
    )
    .await
    .unwrap();
    assert!(!first.replaced);
    assert_eq!(first.findings_linked, vec![w.node_id]);

    // same-day re-run: replaced, SAME node_id, edge not duplicated
    let second = upsert_report(
        &pool,
        report(date!(2026 - 07 - 04), "attention", vec![w.wi_number]),
    )
    .await
    .unwrap();
    assert!(second.replaced);
    assert_eq!(second.node_id, first.node_id);

    let full = get_report(&pool, first.node_id).await.unwrap().unwrap();
    assert_eq!(full.row.status, "attention");
    assert_eq!(full.findings.len(), 1);
    assert_eq!(full.findings[0].wi_number, w.wi_number);

    // a different day is a new node; list is newest-first
    let next = upsert_report(&pool, report(date!(2026 - 07 - 05), "ok", vec![]))
        .await
        .unwrap();
    assert_ne!(next.node_id, first.node_id);
    let rows = list_reports(&pool, Some("kmon"), 10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].report_date, date!(2026 - 07 - 05));

    // unknown finding wi_numbers are dropped, not fatal
    let loose = upsert_report(&pool, report(date!(2026 - 07 - 06), "ok", vec![99999]))
        .await
        .unwrap();
    assert!(loose.findings_linked.is_empty());
}

/// D-7 — a same-day re-run REPLACES the finding set. It used to only ever add,
/// so a corrected re-run left the retracted findings attached and `get_report`
/// over-reported them.
#[tokio::test]
async fn rerun_replaces_the_finding_set() {
    let (_c, pool) = fresh_korg().await;
    let a = create_work_item(&pool, wi("disk full")).await.unwrap();
    let b = create_work_item(&pool, wi("cert expiring")).await.unwrap();
    let c = create_work_item(&pool, wi("clock skew")).await.unwrap();

    let day = date!(2026 - 07 - 04);
    let first = upsert_report(
        &pool,
        report(day, "problem", vec![a.wi_number, b.wi_number]),
    )
    .await
    .unwrap();
    let full = get_report(&pool, first.node_id).await.unwrap().unwrap();
    assert_eq!(full.findings.len(), 2);

    // b is retracted, c is new: the edge set becomes exactly {a, c}.
    let second = upsert_report(
        &pool,
        report(day, "attention", vec![a.wi_number, c.wi_number]),
    )
    .await
    .unwrap();
    assert_eq!(second.node_id, first.node_id);
    let full = get_report(&pool, first.node_id).await.unwrap().unwrap();
    let numbers: Vec<i64> = full.findings.iter().map(|f| f.wi_number).collect();
    assert_eq!(
        numbers,
        vec![a.wi_number, c.wi_number],
        "stale edge dropped"
    );

    // An empty re-run clears them all.
    upsert_report(&pool, report(day, "ok", vec![]))
        .await
        .unwrap();
    let full = get_report(&pool, first.node_id).await.unwrap().unwrap();
    assert!(full.findings.is_empty(), "empty run clears the finding set");
}

/// WI #529 — the touch trigger was attached to `node` and `comment` only, so
/// `project.updated` sat frozen at creation time.
#[tokio::test]
async fn project_updated_advances_on_write() {
    let (_c, pool) = fresh_korg().await;
    let id = korg_core::repo::create_project(&pool, "korg")
        .await
        .unwrap();
    let before: time::OffsetDateTime =
        sqlx::query_scalar("SELECT updated FROM project WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

    korg_core::repo::update_project(
        &pool,
        id,
        &korg_core::repo::ProjectPatch {
            status: Some("maintenance".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let after: time::OffsetDateTime =
        sqlx::query_scalar("SELECT updated FROM project WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        after > before,
        "project.updated must advance ({before} -> {after})"
    );
}
