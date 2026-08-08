//! Sprint 054 (#810 + #870, korg:1080) — the UI catch-up bundle's core halves.
//!
//! The sprint was ranked last on purpose: an audit of "is every node type's
//! shape visible somewhere?" is only worth running against the *final* shape,
//! and sprints 050-053 kept changing it. Run last, it was supposed to be the
//! sweep that caught whatever the preceding sprints had added — and it was,
//! immediately:
//!
//! **`schedule` had been invisible since sprint 051.** Migration 0025 added the
//! kind; `get_node_preview` never got an arm; every find-by-ID and every
//! `materializes` edge rendered `schedule #1042` with no fields. Three sprints,
//! production, silent.
//!
//! What makes that worth a suite of its own is *why* nothing caught it. There
//! were two guards, and both reported green:
//!
//! 1. `every_node_kind_resolves_to_a_real_title` (sprint 049) was written for
//!    exactly this failure and its doc comment claimed it covered "every kind
//!    the `node.kind` check constraint admits". It iterated a **hardcoded array
//!    of seven ids**. The constraint had eight entries.
//! 2. sprint 051's own `a_schedule_is_an_ordinary_node_for_the_generic_surfaces`
//!    called `get_node_preview` on a schedule and asserted `preview.kind ==
//!    "schedule"` — a field the *fallback* sets too, from the base query. The
//!    assertion could not fail, so it proved nothing while reading as coverage.
//!
//! The root cause is one line: node kinds were the only closed set in korg not
//! in `vocab.rs`. Every other vocabulary has lived there since #526, fenced by a
//! partition test; kinds lived in whichever migration last touched the CHECK,
//! hand-copied into two tests that then did not grow. `vocab::NODE_KINDS` and
//! `node_kinds_match_the_check_constraint` (tests/schema.rs) close that, and the
//! tests below hold the behaviour the closure was for.
//!
//! #810's `parked` rides along because it is the same kind of change — a
//! vocabulary value whose real weight is rendering — and because it exposed a
//! second hardcoded copy of a status set, this one load-bearing (see
//! `parking_a_materialized_item_keeps_its_schedule_quiet`).

use korg_core::repo::{
    create_schedule, create_work_item, get_node_preview, list_schedules, materialize_schedule,
    update_work_item, ArchivedFilter, NewSchedule, WorkItemPatch,
};
use korg_core::vocab;
use korg_test_support::{fresh_korg, new, test_project};

// --- #870: the audit's finding ----------------------------------------------

/// The gap itself, asserted on the fields a reader actually needs.
///
/// `every_node_kind_resolves_to_a_real_title` now fails without the arm, but it
/// only checks the *title* — enough to catch the fallback, not enough to say the
/// preview is useful. A schedule whose panel showed a title and nothing else
/// would pass that fence and still fail the WI, which asked that "most fields
/// should be viewable somehow".
#[tokio::test]
async fn a_schedule_previews_its_shape_not_just_its_id() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        NewSchedule {
            notes: Some("Restores must be rehearsed or they are not backups.".into()),
            ..new::schedule("korg restore drill — {QUARTER} {YEAR}", "quarterly", None)
        },
    )
    .await
    .unwrap();

    let p = get_node_preview(&pool, s.node_id)
        .await
        .unwrap()
        .expect("a schedule previews like any node");

    assert_eq!(p.kind, "schedule");
    // Substituted, like every other schedule read: the panel answers "what is
    // this", and the template is not what Materialise would create.
    assert!(
        p.title.starts_with("korg restore drill — Q"),
        "the preview title must be substituted, got {:?}",
        p.title
    );
    assert!(
        !p.title.contains("{QUARTER}"),
        "an unsubstituted placeholder reached the panel: {:?}",
        p.title
    );
    // The raw template stays reachable, so the substitution is never invisible.
    let labels: Vec<&str> = p.fields.iter().map(|f| f.label.as_str()).collect();
    assert!(
        labels.contains(&"Template"),
        "the unsubstituted template must stay visible when it differs, got {labels:?}"
    );
    assert!(
        labels.contains(&"Anchor") && labels.contains(&"Anchored") && labels.contains(&"Creates"),
        "cadence mechanics must be readable from the panel, got {labels:?}"
    );
    assert!(
        p.badges.contains(&"quarterly".to_string()),
        "the cadence is the schedule's headline fact, got {:?}",
        p.badges
    );
    assert_eq!(
        p.body.as_deref(),
        Some("Restores must be rehearsed or they are not backups."),
        "`notes` is the long form and belongs in the body"
    );
    assert_eq!(p.body_label.as_deref(), Some("Notes"));
}

/// A schedule with no notes falls back to the work-item template as its body —
/// the same "long form if there is one, else the next most useful thing" rule
/// a proposal and a program already follow.
#[tokio::test]
async fn a_schedule_without_notes_previews_the_item_it_would_create() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        NewSchedule {
            notes: None,
            template: Some("1. Restore to a scratch host\n2. Diff the corpus".into()),
            ..new::schedule("a drill", "yearly", None)
        },
    )
    .await
    .unwrap();

    let p = get_node_preview(&pool, s.node_id).await.unwrap().unwrap();
    assert_eq!(p.body_label.as_deref(), Some("Work item template"));
    assert!(p.body.unwrap().contains("scratch host"));
    // Nothing to disambiguate, so no Template field competing with the title.
    assert!(
        !p.fields.iter().any(|f| f.label == "Template"),
        "a title with no substitutions must not repeat itself as a field"
    );
}

/// The other half of #870's specific ask: the proposal pop-out shows the *new*
/// proposal shape, and the piece still missing was the one a proposal is
/// defined by — what it covers.
#[tokio::test]
async fn a_proposal_preview_names_the_work_it_covers() {
    use korg_core::repo::{create_proposal, relate};

    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let a = create_work_item(&pool, new::work_item("first"))
        .await
        .unwrap();
    let b = create_work_item(&pool, new::work_item("second"))
        .await
        .unwrap();
    let p = create_proposal(&pool, new::proposal("a bundle"))
        .await
        .unwrap();

    for wi in [&a, &b] {
        relate(&pool, p.row.node_id, wi.node_id, "covers", None, None)
            .await
            .unwrap();
    }

    let preview = get_node_preview(&pool, p.row.node_id)
        .await
        .unwrap()
        .unwrap();
    let covers = preview
        .fields
        .iter()
        .find(|f| f.label == "Covers")
        .expect("a proposal preview must say what it covers");
    assert_eq!(
        covers.value,
        format!("#{}, #{}", a.wi_number, b.wi_number),
        "ordered by wi_number, as `covered` is"
    );
}

/// A proposal covering nothing says nothing, rather than showing an empty row.
#[tokio::test]
async fn an_empty_proposal_preview_omits_the_covers_field() {
    use korg_core::repo::create_proposal;

    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let p = create_proposal(&pool, new::proposal("covers nothing"))
        .await
        .unwrap();
    let preview = get_node_preview(&pool, p.row.node_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !preview.fields.iter().any(|f| f.label == "Covers"),
        "an empty `Covers` row is noise, not information"
    );
}

// --- #810: parked -----------------------------------------------------------

/// The load-bearing consequence of adding a status, and the reason #810 was not
/// the one-line change it looked like.
///
/// `SCHEDULE_DUE_SQL` and `schedule_select` both hardcoded the outstanding set
/// as `('open', 'resolved')` — written before sprint 053 named that exact set
/// `WI_UNFINISHED_STATUSES`. Adding `parked` to the vocabulary without touching
/// them would have left a schedule believing a parked work item was finished:
/// it would come due again and materialise a **duplicate** of the item somebody
/// had just deliberately deferred. Silent, and only visible as mysterious
/// recurring work items months later.
///
/// This is the test that would have failed. The two statements now derive the
/// set from the vocabulary, so a sixth status cannot reintroduce it.
#[tokio::test]
async fn parking_a_materialized_item_keeps_its_schedule_quiet() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    // A drill that is already overdue, so due-ness turns purely on whether its
    // outstanding item counts.
    let s = create_schedule(
        &pool,
        new::schedule(
            "a drill",
            "quarterly",
            Some(time::OffsetDateTime::now_utc() - time::Duration::days(200)),
        ),
    )
    .await
    .unwrap();
    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    // Park the item it produced: "waiting on a condition", not done.
    update_work_item(
        &pool,
        out.work_item.node_id,
        WorkItemPatch {
            wi_status: Some("parked".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let list = list_schedules(&pool, None, None, false, ArchivedFilter::default())
        .await
        .unwrap();
    let row = list
        .items
        .iter()
        .find(|r| r.node_id == s.node_id)
        .expect("the schedule is still listed");

    assert!(
        row.outstanding,
        "a parked work item is still the surface — the schedule must not \
         consider itself free to fire again"
    );
    assert!(
        !row.due,
        "a schedule with an outstanding item is not due; firing here would \
         duplicate work that was deliberately deferred"
    );

    // And the refusal is real, not just a flag on the row.
    assert!(
        materialize_schedule(&pool, s.node_id, false).await.is_err(),
        "materialising over an outstanding parked item must be refused"
    );
}

/// `parked` is in the vocabulary the write path enforces, and it round-trips.
/// Cheap, but it is the assertion that `WI_STATUSES` and `update_work_item`'s
/// validation agree — they are the same list, and this proves it rather than
/// assuming it.
#[tokio::test]
async fn parked_is_an_accepted_work_item_status() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    assert!(
        vocab::WI_STATUSES.contains(&"parked"),
        "the vocabulary is the authority; wi_status has no DB CHECK to fall back on"
    );

    let wi = create_work_item(&pool, new::work_item("wait for a condition"))
        .await
        .unwrap();
    let updated = update_work_item(
        &pool,
        wi.node_id,
        WorkItemPatch {
            wi_status: Some("parked".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.wi_status, "parked");
}

/// Parked work stays in the default listing. The status exists to keep things
/// *in view* — #810's whole framing is "WIs we need to keep aware of" — so a
/// lean read hiding them would defeat it, and would make `parked` a slower
/// spelling of `closed`.
#[tokio::test]
async fn parked_items_stay_in_the_default_listing() {
    use korg_core::repo::list_work_items_lean;

    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let wi = create_work_item(&pool, new::work_item("deferred but wanted"))
        .await
        .unwrap();
    update_work_item(
        &pool,
        wi.node_id,
        WorkItemPatch {
            wi_status: Some("parked".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let page = list_work_items_lean(&pool, None, None, ArchivedFilter::default(), 50, 0)
        .await
        .unwrap();
    assert!(
        page.items.iter().any(|i| i.wi_number == wi.wi_number),
        "a lean list must still show parked work — the divider de-prioritises \
         it, hiding it would be `closed`"
    );
    assert_eq!(
        page.omitted.closed, 0,
        "parked must not be counted as hidden-by-default"
    );
}
