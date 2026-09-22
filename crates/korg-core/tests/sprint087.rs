//! Sprint 087 — archiving a project clears its location metadata.
//!
//! WI 3003, slice 10 of program korg:3062. An archived project's `src_path`,
//! `machines` and `deploy_to` answer "where is the source", and for an
//! archived project the answer is "nowhere" (Ken's ruling, korg:2250,
//! 2026-09-11). Until now nothing enforced that at the moment it becomes
//! true: the 2026-09-11 sweep cleared eleven rows by hand, and kmuster's
//! weekly `check-projects` reported drift up to a week after the fact.
//!
//! The clear happens **inside `update_project`'s existing transaction**, so
//! no reader can observe a row that is archived and still carries a path.
//! That makes kmuster's assertion a backstop rather than the mechanism —
//! which is what WI 2382 asked for, from the side of the boundary that is
//! allowed to write prose.
//!
//! The tests that matter most here are the ones asserting what is *not*
//! touched: nothing lossy, nothing on the way back to `active`, and nothing
//! written twice.

use korg_core::repo::{update_project_by_name, ProjectPatch};
use korg_test_support::fresh_korg;
use sqlx::PgPool;

async fn make_project(pool: &PgPool, name: &str) {
    sqlx::query("INSERT INTO project (name, status) VALUES ($1, 'active')")
        .bind(name)
        .execute(pool)
        .await
        .expect("seed project");
}

/// A project as it looks the moment before somebody archives it: a working
/// copy on disk, a machine it lives on, and somewhere it deploys.
fn located() -> ProjectPatch {
    ProjectPatch {
        src_path: Some(Some("~/src/ai/khound".into())),
        machines: Some(vec!["kubs0".into()]),
        deploy_to: Some(vec!["kubs0".into()]),
        ..Default::default()
    }
}

fn archive() -> ProjectPatch {
    ProjectPatch {
        status: Some("archived".into()),
        ..Default::default()
    }
}

fn today() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
}

/// The whole point, end to end: archive a located project and all three
/// fields are empty, in the same call.
#[tokio::test]
async fn archiving_clears_the_three_location_fields() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(&pool, "subject", &located())
        .await
        .expect("locate");

    let row = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive");

    assert_eq!(row.status, "archived");
    assert_eq!(row.src_path, None, "src_path must be cleared on archive");
    assert!(
        row.machines.is_empty(),
        "machines must be cleared on archive, got {:?}",
        row.machines
    );
    assert!(
        row.deploy_to.is_empty(),
        "deploy_to must be cleared on archive, got {:?}",
        row.deploy_to
    );
}

/// Non-lossy is the half that makes the clear acceptable: a field silently
/// emptied is indistinguishable from one that was never set, and somebody
/// does come back to an archived project.
#[tokio::test]
async fn the_cleared_values_are_recorded_in_notes() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(&pool, "subject", &located())
        .await
        .expect("locate");

    let row = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive");

    let notes = row.notes.expect("archiving must record what it cleared");
    assert!(
        notes.contains("~/src/ai/khound"),
        "the cleared src_path must survive in notes: {notes}"
    );
    assert!(
        notes.contains("`machines` kubs0"),
        "the cleared machines must survive in notes: {notes}"
    );
    assert!(
        notes.contains("`deploy_to` kubs0"),
        "the cleared deploy_to must survive in notes: {notes}"
    );
    assert!(
        notes.contains(&today()),
        "the record must say when it happened: {notes}"
    );
}

/// An archived project's `notes` is where its history already lives — the
/// 2026-09-11 sweep put eleven rows' worth there. Appending must not eat it.
#[tokio::test]
async fn existing_notes_are_kept_and_appended_to() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            notes: Some(Some("STOPPED at Gate A, 2026-08-09 (korg:1175).".into())),
            ..located()
        },
    )
    .await
    .expect("locate");

    let row = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive");

    let notes = row.notes.expect("notes");
    assert!(
        notes.starts_with("STOPPED at Gate A, 2026-08-09 (korg:1175)."),
        "prior notes must stay, and stay first: {notes}"
    );
    assert!(
        notes.contains("~/src/ai/khound"),
        "the appended record must be there too: {notes}"
    );
}

/// A caller archiving *and* rewriting `notes` in one call means the new
/// notes — so the record appends to what the caller just wrote, not to what
/// it replaced. Reading the row inside the transaction is what buys this.
#[tokio::test]
async fn a_notes_write_in_the_same_call_is_what_gets_appended_to() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            notes: Some(Some("the old prose".into())),
            ..located()
        },
    )
    .await
    .expect("locate");

    let row = update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            notes: Some(Some("Retired 2026-09-22 by korg WI 3003.".into())),
            ..archive()
        },
    )
    .await
    .expect("archive");

    let notes = row.notes.expect("notes");
    assert!(
        notes.starts_with("Retired 2026-09-22 by korg WI 3003."),
        "the caller's new notes must lead: {notes}"
    );
    assert!(
        !notes.contains("the old prose"),
        "the replaced notes must not be resurrected: {notes}"
    );
    assert!(
        notes.contains("~/src/ai/khound"),
        "the record must still be appended: {notes}"
    );
}

/// Setting a path and archiving in one call is a caller contradicting
/// itself, and archiving is the later intent — the row must not come out of
/// the call still located.
#[tokio::test]
async fn archiving_beats_a_location_set_in_the_same_call() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;

    let row = update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            status: Some("archived".into()),
            ..located()
        },
    )
    .await
    .expect("archive");

    assert_eq!(row.src_path, None);
    assert!(row.machines.is_empty());
    assert!(row.deploy_to.is_empty());
    let notes = row
        .notes
        .expect("the values still existed, so they are recorded");
    assert!(notes.contains("~/src/ai/khound"), "{notes}");
}

/// Archiving a project that never had location metadata must not invent a
/// paragraph saying it cleared nothing. All 16 live archived rows are
/// already clean, so this is the common case on any re-archive.
#[tokio::test]
async fn archiving_an_unlocated_project_writes_no_record() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            notes: Some(Some("just prose".into())),
            ..Default::default()
        },
    )
    .await
    .expect("seed notes");

    let row = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive");

    assert_eq!(row.status, "archived");
    assert_eq!(
        row.notes.as_deref(),
        Some("just prose"),
        "nothing was cleared, so nothing is recorded"
    );
}

/// Archiving twice records once. The guard is "is there anything to clear",
/// not "is this a transition", so the second call finds an empty row and
/// does nothing — which is also what makes this a safe backstop against a
/// row that went stale by some other route.
#[tokio::test]
async fn re_archiving_does_not_write_a_second_record() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(&pool, "subject", &located())
        .await
        .expect("locate");

    let once = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive")
        .notes
        .expect("notes");
    let twice = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("re-archive")
        .notes
        .expect("notes");

    assert_eq!(once, twice, "the second archive must be a no-op");
    assert_eq!(
        twice.matches("Archived-project metadata cleared").count(),
        1,
        "exactly one record: {twice}"
    );
}

/// Un-archiving is how a project comes back (`selectors.rs` keeps
/// `update_project` out of #884's refusal precisely so it can). Nothing
/// about going active clears anything, and re-locating the row afterwards
/// must stick.
#[tokio::test]
async fn coming_back_to_active_clears_nothing() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive");

    let row = update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            status: Some("active".into()),
            ..located()
        },
    )
    .await
    .expect("revive");

    assert_eq!(row.status, "active");
    assert_eq!(row.src_path.as_deref(), Some("~/src/ai/khound"));
    assert_eq!(row.machines, vec!["kubs0".to_string()]);
    assert_eq!(row.deploy_to, vec!["kubs0".to_string()]);
    assert_eq!(
        row.notes, None,
        "nothing was cleared, so nothing is recorded"
    );
}

/// A patch that does not mention `status` leaves an archived row alone —
/// the trigger is archiving, not touching an archived project. This is the
/// case kmuster's `archived_metadata_retained` check remains the backstop
/// for: metadata that arrives on an archived row by some other route.
#[tokio::test]
async fn editing_an_archived_project_without_restating_status_does_not_clear() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive");

    let row = update_project_by_name(&pool, "subject", &located())
        .await
        .expect("edit");

    assert_eq!(row.status, "archived");
    assert_eq!(
        row.src_path.as_deref(),
        Some("~/src/ai/khound"),
        "korg clears on archive; it is not a sweeper"
    );
}

/// Multiple machines render unambiguously. The live corpus only ever had
/// one, which is why the 2026-09-11 wording could get away with commas
/// between fields; a semicolon is what keeps `machines kai, kubs0` from
/// reading as two fields.
#[tokio::test]
async fn multiple_machines_are_separated_from_the_next_field() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;
    update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            machines: Some(vec!["kai".into(), "kubs0".into()]),
            deploy_to: Some(vec!["kubsdb".into()]),
            ..Default::default()
        },
    )
    .await
    .expect("locate");

    let notes = update_project_by_name(&pool, "subject", &archive())
        .await
        .expect("archive")
        .notes
        .expect("notes");

    assert!(
        notes.contains("`machines` kai, kubs0; `deploy_to` kubsdb"),
        "list commas must not be confusable with field separators: {notes}"
    );
    // The standing sentence names all three fields; only the list at the end
    // says what was actually cleared.
    let listed = notes
        .split_once("Cleared values: ")
        .expect("a record names what it cleared")
        .1;
    assert!(
        !listed.contains("`src_path`"),
        "a field that was not set is not named as cleared: {listed}"
    );
}
