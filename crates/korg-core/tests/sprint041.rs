//! Sprint 041 — project timestamps join "every column" (#905).
//!
//! `get_project` and `list_projects detail:"full"` both promise "every
//! column", and both returned ten of twelve: `project.created` and
//! `project.updated` have existed since 0001, and migration 0013 has been
//! advancing `updated` on every write since #529 — unobserved, because
//! `ProjectRow` never selected them. Projects were the last kind whose
//! recency an agent could not read.
//!
//! The lean `list_projects` row stays lean. Timestamps are not routing
//! signal, and the tiering decision (#828) is that the default view carries
//! only what answers *does this belong here?*.

use korg_core::repo::{
    create_project, get_project, get_project_detail, list_projects_full, list_projects_lean,
    update_project, ProjectPatch,
};
use korg_test_support::fresh_korg;

/// The probe from the 894 measurement re-run, as a test: ask for every column
/// and get the timestamps, then prove `updated` is a live recency signal
/// rather than a frozen copy of `created`.
#[tokio::test]
async fn project_rows_carry_timestamps_and_a_write_advances_updated() {
    let (_c, pool) = fresh_korg().await;
    let id = create_project(&pool, "timestamped").await.expect("create");
    let before = get_project(&pool, id)
        .await
        .expect("get")
        .expect("just created");

    assert_eq!(
        before.created, before.updated,
        "a project nobody has edited yet has not been touched"
    );

    let after = update_project(
        &pool,
        id,
        &ProjectPatch {
            description: Some(Some("routing metadata and its drift checks".into())),
            ..Default::default()
        },
    )
    .await
    .expect("update");

    assert_eq!(after.created, before.created, "created is immutable");
    assert!(
        after.updated > before.updated,
        "0013's trigger has to advance `updated` — that is the whole recency \
         signal (created {}, updated {})",
        after.created,
        after.updated
    );
}

/// Both reads that promise "every column" have to keep the promise, and the
/// lean one has to keep its own opposite promise. Asserted on serialized JSON
/// because the wire shape is what the two tool descriptions describe.
#[tokio::test]
async fn the_full_tier_carries_timestamps_and_the_lean_tier_does_not() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "tiered").await.expect("create");

    let detail = get_project_detail(&pool, "tiered")
        .await
        .expect("get_project_detail")
        .expect("just created");
    let detail = serde_json::to_value(&detail).expect("serialize detail");
    assert!(
        detail.get("created").is_some() && detail.get("updated").is_some(),
        "get_project promises every column: {detail}"
    );

    let full = list_projects_full(&pool, None).await.expect("full list");
    let full = serde_json::to_value(&full.items[0]).expect("serialize full row");
    assert!(
        full.get("created").is_some() && full.get("updated").is_some(),
        "list_projects detail:\"full\" promises every column: {full}"
    );

    let lean = list_projects_lean(&pool, None).await.expect("lean list");
    let lean = serde_json::to_value(&lean.items[0]).expect("serialize lean row");
    assert!(
        lean.get("created").is_none() && lean.get("updated").is_none(),
        "the lean routing row stays lean — timestamps answer *when*, not \
         *does this belong here?*: {lean}"
    );
}
