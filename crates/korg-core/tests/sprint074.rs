//! Sprint 074 (#1629, korg:1630) — `starred` on projects.
//!
//! Ken works a handful of projects at a time for a week or so, then the set
//! moves on. Both project rails lift the starred ones into a band above the
//! category groups, leaving each project in its normal category position too —
//! the duplication is the feature.
//!
//! What this suite pins is the part that is a *contract* rather than a layout:
//!
//! * **It is a column, not a browser preference** (D-1). Both rails already
//!   keep `groupByCategory` in per-browser sticky storage and starring could
//!   have ridden that for free. A set of hot projects is a fact about the
//!   week's work, so it lives where every browser and every agent can see it.
//! * **A patch that does not mention it does not touch it.** The failure this
//!   guards is specific: `Option<bool>` deserialising to `Some(false)` on an
//!   absent key would make every unrelated project edit silently unstar the
//!   project. Nothing in the UI would look wrong until the band emptied.
//! * **It is absent from the lean read, deliberately.** `ProjectLeanRow`
//!   answers *does this work belong here?*; hotness is not evidence for that,
//!   and a routing agent that can see it is one with a thumb on the scale.
//!
//! Deliberately **not** here: any ordering, gating or filtering built on
//! `starred`. Nothing in korg reads it except the rails — it is not a priority
//! (GP-3), not a tier (GP-10), and not `pinned`, which orders proposal and
//! program queues and is a different idea wearing a similar word.

use korg_core::repo::{
    create_project, get_project, list_projects_full, list_projects_lean, update_project,
    ProjectPatch,
};
use korg_test_support::fresh_korg;
use sqlx::PgPool;

async fn project(pool: &PgPool, name: &str) -> i64 {
    create_project(pool, name).await.expect("create project")
}

async fn starred_of(pool: &PgPool, id: i64) -> bool {
    get_project(pool, id)
        .await
        .expect("get project")
        .expect("project exists")
        .starred
}

#[tokio::test]
async fn a_project_starts_unstarred() {
    let (_c, pool) = fresh_korg().await;
    let id = project(&pool, "korg").await;

    // Migration 0032 defaults to false, which states a fact rather than
    // guessing one: no project has ever been starred.
    assert!(!starred_of(&pool, id).await);
}

#[tokio::test]
async fn starring_round_trips_and_is_reversible() {
    let (_c, pool) = fresh_korg().await;
    let id = project(&pool, "korg").await;

    let updated = update_project(
        &pool,
        id,
        &ProjectPatch {
            starred: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("star");
    assert!(updated.starred, "the write returns the new value");
    assert!(starred_of(&pool, id).await, "and it persisted");

    let updated = update_project(
        &pool,
        id,
        &ProjectPatch {
            starred: Some(false),
            ..Default::default()
        },
    )
    .await
    .expect("unstar");
    assert!(!updated.starred);
    assert!(!starred_of(&pool, id).await);
}

#[tokio::test]
async fn a_patch_that_does_not_mention_starred_leaves_it_alone() {
    let (_c, pool) = fresh_korg().await;
    let id = project(&pool, "korg").await;
    update_project(
        &pool,
        id,
        &ProjectPatch {
            starred: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("star");

    // The failure mode this exists for: an absent key deserialising to
    // `Some(false)` would make every unrelated edit quietly unstar the project.
    let updated = update_project(
        &pool,
        id,
        &ProjectPatch {
            description: Some(Some("work tracking".into())),
            ..Default::default()
        },
    )
    .await
    .expect("edit something else");

    assert!(
        updated.starred,
        "editing the description must not clear the star"
    );
}

#[tokio::test]
async fn an_absent_json_key_is_not_a_false() {
    let (_c, pool) = fresh_korg().await;
    let id = project(&pool, "korg").await;
    update_project(
        &pool,
        id,
        &ProjectPatch {
            starred: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("star");

    // The same guarantee one layer out, where callers actually live: REST and
    // MCP both hand `update_project` a `ProjectPatch` deserialised from JSON,
    // so the `#[serde(default)]` is the thing under test, not the struct
    // literal the test above builds by hand.
    let patch: ProjectPatch =
        serde_json::from_str(r#"{"description": "work tracking"}"#).expect("deserialise patch");
    assert!(patch.starred.is_none(), "an absent key is None, not Some");

    let updated = update_project(&pool, id, &patch).await.expect("apply");
    assert!(updated.starred);
}

#[tokio::test]
async fn starred_rides_the_full_read_and_not_the_lean_one() {
    let (_c, pool) = fresh_korg().await;
    let id = project(&pool, "korg").await;
    update_project(
        &pool,
        id,
        &ProjectPatch {
            starred: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("star");

    let full = list_projects_full(&pool, None).await.expect("full read");
    let row = full
        .items
        .iter()
        .find(|p| p.name == "korg")
        .expect("korg is in the full read");
    assert!(row.starred, "the full projection carries it");

    // And the lean one does not — asserted on the wire rather than on the
    // struct, because the wire is what an agent reads. This is a decision, not
    // an omission: the lean row is the routing projection, and a project being
    // hot this week is not evidence that a work item belongs to it.
    let lean = list_projects_lean(&pool, None).await.expect("lean read");
    let json = serde_json::to_string(&lean).expect("serialise lean");
    assert!(
        !json.contains("starred"),
        "the lean routing projection must not carry hotness: {json}"
    );
}
