//! Sprint 079 (#2151–#2154, korg:2162) — `soaking`, the `soaks` edge, the two
//! soak fields, and `reviewed` on reports.
//!
//! Slice 1 of program korg:2167, designed in korg:2149 / handoff korg:2150.
//!
//! The problem: a program whose engineering is finished but whose acceptance
//! can only be satisfied by the passage of days had nowhere to sit. It stayed
//! `active`, and kfdc renders `active` as "wants your attention", so for two or
//! three days the board generated demand nobody could satisfy. Closing it was
//! the wrong answer — the work is not complete.
//!
//! What this suite pins:
//!
//! * **`soaking` is live**, on the same argument `parked` won in sprint 072:
//!   the program somebody must still act on is exactly the one a default read
//!   must not hide. Moving it to a calmer *panel* is the consumer's job.
//! * **One entry rule, and only on entry.** `soaking` asserts a fact about the
//!   slices the way `queued` does, so korg checks it. Leaving is free in all
//!   three directions the design needs.
//! * **The lift out of `soaking` mirrors `queued`'s**, which is what makes the
//!   failure path work without every skill re-implementing it — and `parked`
//!   still does not lift, which is the fence sprint 072 left behind.
//! * **The `soaks` edge refuses twice**, and both refusals are the design's
//!   content rather than defensive noise: the soak fields are what stopped
//!   kmon #2058 being checkable, and the live-proposal refusal is what makes
//!   extracting a test out of a slice two deliberate calls.
//! * **The wire format of `check_after`** on the way in and out. 0010's
//!   `report_date` was parsed by hand in each transport until a shared format
//!   description ended the divergence; a soak date that is a string on one
//!   transport and a date on the other is that defect with a new column name.
//! * **`reviewed` resets on a same-day re-run** — the one piece of #2154 that
//!   is not a plain column.

use korg_core::relationships::SOAKS_LABEL;
use korg_core::repo::{
    board_rollup, create_program, create_proposal, create_work_item, get_program,
    get_program_detail, get_report, get_work_item, list_programs, list_reports, relate,
    set_report_reviewed, unrelate, update_program, update_proposal, update_work_item,
    upsert_report as create_report, ArchivedFilter, NewProgram, NewWorkItem, ProgramPatch,
    ProposalPatch, WorkItemPatch,
};
use korg_core::vocab::{
    PROGRAM_LIFTABLE_STATUSES, PROGRAM_LIVE_STATUSES, PROGRAM_TERMINAL_STATUSES, SOAKING_STATUS,
};
use korg_test_support::{fresh_korg, new, test_project};
use rust_decimal::Decimal;
use sqlx::PgPool;
use time::macros::date;

// --- scaffolding ------------------------------------------------------------

/// A work item carrying both soak fields — what `soaks` requires.
async fn soak_wi(pool: &PgPool, title: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            check_after: Some(date!(2026 - 09 - 13)),
            invalidated_if: Some("kai's baseline is regenerated".into()),
            // Filed, so the rollup's project join is actually exercised — a
            // soak's project is what a Delayed Ops row groups by.
            project: Some(korg_test_support::TEST_PROJECT.into()),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number
}

/// A work item with neither soak field — an ordinary one.
async fn plain_wi(pool: &PgPool, title: &str) -> i64 {
    create_work_item(pool, new::work_item(title))
        .await
        .unwrap()
        .wi_number
}

async fn proposal(pool: &PgPool, title: &str) -> i64 {
    create_proposal(pool, new::proposal(title))
        .await
        .unwrap()
        .row
        .node_id
}

async fn program_over(pool: &PgPool, title: &str, slices: Vec<i64>) -> i64 {
    create_program(
        pool,
        NewProgram {
            slices,
            ..new::program(title)
        },
    )
    .await
    .unwrap()
    .row
    .node_id
}

async fn set_program_status(pool: &PgPool, node_id: i64, status: &str) -> anyhow::Result<()> {
    update_program(
        pool,
        node_id,
        ProgramPatch {
            status: Some(status.into()),
            ..Default::default()
        },
    )
    .await
    .map(|_| ())
}

async fn set_proposal_status(pool: &PgPool, node_id: i64, status: &str) {
    update_proposal(
        pool,
        node_id,
        ProposalPatch {
            status: Some(status.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn program_status(pool: &PgPool, node_id: i64) -> String {
    get_program(pool, node_id).await.unwrap().unwrap().status
}

async fn transitions(pool: &PgPool, node_id: i64) -> Vec<(String, String)> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT from_status, to_status FROM transition WHERE node_id = $1 ORDER BY id",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// A program in `soaking`, built the only legal way: one terminal slice and one
/// live soak.
async fn soaking_program(pool: &PgPool, title: &str) -> (i64, i64) {
    let slice = proposal(pool, &format!("{title} slice")).await;
    let program = program_over(pool, title, vec![slice]).await;
    set_proposal_status(pool, slice, "done").await;
    let wi = soak_wi(pool, &format!("{title} soak")).await;
    relate(pool, program, wi, SOAKS_LABEL, None, None)
        .await
        .unwrap();
    set_program_status(pool, program, SOAKING_STATUS)
        .await
        .unwrap();
    (program, wi)
}

// --- #2151: the vocabulary landed on the right side -------------------------

/// The partition fences in `vocab.rs` refuse an *unclassified* status; this
/// pins which side `soaking` fell on, because that single choice is the whole
/// difference between the feature and a slower spelling of `done`.
///
/// Filed terminal it would partition just as cleanly, drop out of
/// `list_programs` and `board.programs`, and disappear on exactly the day it
/// most needs watching.
#[tokio::test]
async fn soaking_is_live_not_terminal() {
    assert!(PROGRAM_LIVE_STATUSES.contains(&SOAKING_STATUS));
    assert!(!PROGRAM_TERMINAL_STATUSES.contains(&SOAKING_STATUS));
    assert!(
        PROGRAM_LIFTABLE_STATUSES.contains(&SOAKING_STATUS),
        "a slice starting under a soaking program lifts it — the failure path"
    );
}

// --- #2151: the entry rule --------------------------------------------------

#[tokio::test]
async fn entering_soaking_needs_every_slice_terminal() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let slice = proposal(&pool, "still going").await;
    let program = program_over(&pool, "half done", vec![slice]).await;
    let wi = soak_wi(&pool, "the soak").await;
    relate(&pool, program, wi, SOAKS_LABEL, None, None)
        .await
        .unwrap();

    // The slice is still `proposed`, so the program has work left that is not a
    // clock.
    let err = set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("not"), "{err}");
    assert!(err.contains("terminal"), "{err}");
    assert!(
        err.contains("holding"),
        "the refusal must name the status that IS right for 'between slices': {err}"
    );
    assert_eq!(program_status(&pool, program).await, "queued");

    // Declining it counts as terminal just as much as finishing it — the rule
    // is about work outstanding, not work succeeded.
    set_proposal_status(&pool, slice, "declined").await;
    set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .unwrap();
    assert_eq!(program_status(&pool, program).await, SOAKING_STATUS);
}

#[tokio::test]
async fn entering_soaking_needs_at_least_one_live_soak() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let slice = proposal(&pool, "finished").await;
    let program = program_over(&pool, "nothing soaking", vec![slice]).await;
    set_proposal_status(&pool, slice, "done").await;

    // Every slice terminal but no soak at all: this program is `done`, not
    // waiting on anything.
    let err = set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains(SOAKS_LABEL), "{err}");
    assert!(
        err.contains("check_after") && err.contains("invalidated_if"),
        "the refusal should say what a soak needs, so the retry works: {err}"
    );

    // A soak that is already FINISHED does not count either — otherwise a
    // program could re-enter soaking forever on the strength of tests it has
    // already judged.
    let wi = soak_wi(&pool, "already judged").await;
    relate(&pool, program, wi, SOAKS_LABEL, None, None)
        .await
        .unwrap();
    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .is_err());

    // Reopen it and the same call succeeds.
    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            wi_status: Some("open".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .unwrap();
    assert_eq!(program_status(&pool, program).await, SOAKING_STATUS);
}

/// Leaving is unconstrained in all three directions the design needs, and each
/// one is recorded. The failure path especially must stay frictionless: a soak
/// that fails at 04:00 becomes new work without arguing with a state machine.
#[tokio::test]
async fn leaving_soaking_is_free_and_logged() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let (program, _) = soaking_program(&pool, "failing soak").await;
    set_program_status(&pool, program, "holding").await.unwrap();
    set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .unwrap();
    set_program_status(&pool, program, "done").await.unwrap();

    let log = transitions(&pool, program).await;
    assert!(
        log.contains(&(SOAKING_STATUS.into(), "holding".into())),
        "a failed soak must be readable in the transition log: {log:?}"
    );
    assert!(
        log.contains(&(SOAKING_STATUS.into(), "done".into())),
        "{log:?}"
    );
}

/// Re-asserting `soaking` on a program already soaking must not re-run the
/// entry check. Otherwise an unrelated `update_program` — retitling it, say —
/// starts failing the moment the first soak is judged, for a reason that has
/// nothing to do with the call.
#[tokio::test]
async fn re_asserting_soaking_does_not_recheck() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let (program, wi) = soaking_program(&pool, "settling").await;
    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Entry would be refused now; re-asserting the status it already holds is
    // not an entry.
    set_program_status(&pool, program, SOAKING_STATUS)
        .await
        .unwrap();
    assert_eq!(program_status(&pool, program).await, SOAKING_STATUS);
}

// --- #2151: the derived lift ------------------------------------------------

/// The other half of decision 6: the fix for a failed soak arrives as a new
/// slice, and that slice starting is what makes the program `active` again.
/// korg does it, so no skill has to.
#[tokio::test]
async fn a_slice_starting_lifts_a_soaking_program() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let (program, _) = soaking_program(&pool, "soak then fix").await;

    // The fix, inserted as a new slice.
    let fix = proposal(&pool, "the fix").await;
    relate(
        &pool,
        program,
        fix,
        "includes",
        None,
        Some(Decimal::new(2, 0)),
    )
    .await
    .unwrap();
    assert_eq!(
        program_status(&pool, program).await,
        SOAKING_STATUS,
        "attaching a slice is not starting one"
    );

    set_proposal_status(&pool, fix, "active").await;
    assert_eq!(program_status(&pool, program).await, "active");

    let log = transitions(&pool, program).await;
    assert!(
        log.contains(&(SOAKING_STATUS.into(), "active".into())),
        "the lift must record the status it left, not a hardcoded 'queued': {log:?}"
    );
}

/// Sprint 072's fence, re-asserted now that the liftable set has two members.
/// Parking is a declaration that this line of work is dormant regardless of
/// what its slices do, so widening the lift must not have caught it.
#[tokio::test]
async fn parked_programs_are_still_never_auto_promoted() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let slice = proposal(&pool, "live slice").await;
    let program = program_over(&pool, "parked", vec![slice]).await;
    set_program_status(&pool, program, "parked").await.unwrap();
    set_proposal_status(&pool, slice, "active").await;
    assert_eq!(program_status(&pool, program).await, "parked");
}

// --- #2153: the soak fields -------------------------------------------------

#[tokio::test]
async fn soak_fields_round_trip_and_clear() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let wi = soak_wi(&pool, "a soak").await;
    let row = get_work_item(&pool, wi).await.unwrap().unwrap();
    assert_eq!(row.check_after, Some(date!(2026 - 09 - 13)));
    assert_eq!(
        row.invalidated_if.as_deref(),
        Some("kai's baseline is regenerated")
    );

    // Independently clearable: a soak whose window moves has not stopped being
    // invalidated by the same condition.
    let row = update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            check_after: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(row.check_after, None);
    assert_eq!(
        row.invalidated_if.as_deref(),
        Some("kai's baseline is regenerated"),
        "clearing one soak field must not touch the other"
    );

    // And an omitted field is left alone, the `details`/`sprint` convention.
    let row = update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            title: Some("renamed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        row.invalidated_if.as_deref(),
        Some("kai's baseline is regenerated")
    );

    // An ordinary work item carries neither.
    let plain = plain_wi(&pool, "ordinary").await;
    let row = get_work_item(&pool, plain).await.unwrap().unwrap();
    assert_eq!(row.check_after, None);
    assert_eq!(row.invalidated_if, None);
}

/// The wire format, not just the core value (the proposal's explicit
/// watch-out). `check_after` must serialize as `YYYY-MM-DD` — the same
/// `report_date_fmt` 0010 settled on — and must deserialize from that string
/// through the patch type both transports share.
#[tokio::test]
async fn check_after_is_yyyy_mm_dd_on_the_wire() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let wi = soak_wi(&pool, "wire").await;
    let row = get_work_item(&pool, wi).await.unwrap().unwrap();
    let json = serde_json::to_value(&row).unwrap();
    assert_eq!(json["check_after"], serde_json::json!("2026-09-13"));

    // Inbound, through the patch type — the shape an MCP or REST caller sends.
    let patch: WorkItemPatch =
        serde_json::from_value(serde_json::json!({ "check_after": "2026-10-01" })).unwrap();
    let row = update_work_item(&pool, wi, patch).await.unwrap();
    assert_eq!(row.check_after, Some(date!(2026 - 10 - 01)));

    // Explicit null clears, and is distinguishable from an absent key.
    let patch: WorkItemPatch =
        serde_json::from_value(serde_json::json!({ "check_after": null })).unwrap();
    assert_eq!(
        update_work_item(&pool, wi, patch)
            .await
            .unwrap()
            .check_after,
        None
    );

    let patch: WorkItemPatch = serde_json::from_value(serde_json::json!({})).unwrap();
    assert!(patch.check_after.is_none(), "absent must not mean clear");

    // And the create side takes the same string.
    let created: NewWorkItem = serde_json::from_value(serde_json::json!({
        "title": "from the wire",
        "content": "",
        "check_after": "2026-11-02",
        "invalidated_if": "the runner is rebuilt",
    }))
    .unwrap();
    let row = create_work_item(&pool, created).await.unwrap();
    assert_eq!(row.check_after, Some(date!(2026 - 11 - 02)));
}

// --- #2152: the edge and its two refusals -----------------------------------

#[tokio::test]
async fn soaks_refuses_a_work_item_without_its_soak_fields() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let program = program_over(&pool, "p", vec![]).await;
    let bare = plain_wi(&pool, "no soak fields").await;

    let err = relate(&pool, program, bare, SOAKS_LABEL, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("check_after"), "{err}");
    assert!(err.contains("invalidated_if"), "{err}");
    assert!(
        err.contains(&format!("#{bare}")),
        "the refusal names the item so a retry needs no lookup: {err}"
    );

    // One field alone is still a refusal, and it names only the missing one.
    update_work_item(
        &pool,
        bare,
        WorkItemPatch {
            check_after: Some(Some(date!(2026 - 09 - 13))),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let err = relate(&pool, program, bare, SOAKS_LABEL, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalidated_if"), "{err}");

    // Whitespace is not a statement of what invalidates the test.
    update_work_item(
        &pool,
        bare,
        WorkItemPatch {
            invalidated_if: Some(Some("   ".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        relate(&pool, program, bare, SOAKS_LABEL, None, None)
            .await
            .is_err(),
        "a blank invalidated_if is the field being skipped, which is the whole \
         thing #2058 cost"
    );

    update_work_item(
        &pool,
        bare,
        WorkItemPatch {
            invalidated_if: Some(Some("the baseline moves".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    relate(&pool, program, bare, SOAKS_LABEL, None, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn soaks_refuses_a_work_item_a_live_proposal_still_covers() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let program = program_over(&pool, "p", vec![]).await;
    let wi = soak_wi(&pool, "covered soak").await;
    let slice = proposal(&pool, "the slice").await;
    let cover = relate(&pool, slice, wi, "covers", None, None)
        .await
        .unwrap();

    let err = relate(&pool, program, wi, SOAKS_LABEL, None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains(&format!("korg:{slice}")), "{err}");
    assert!(
        err.contains("unrelate"),
        "the refusal states the two-call move: {err}"
    );

    // A `parked` proposal still claims its work — the same reading the
    // membership markers take, and the reason `parked` is not `declined`.
    set_proposal_status(&pool, slice, "parked").await;
    assert!(relate(&pool, program, wi, SOAKS_LABEL, None, None)
        .await
        .is_err());

    // A finished proposal has had its say and claims nothing.
    set_proposal_status(&pool, slice, "done").await;
    relate(&pool, program, wi, SOAKS_LABEL, None, None)
        .await
        .unwrap();

    // And the documented extract move — unrelate covers, then relate soaks.
    let wi2 = soak_wi(&pool, "extracted soak").await;
    let slice2 = proposal(&pool, "live slice").await;
    let cover2 = relate(&pool, slice2, wi2, "covers", None, None)
        .await
        .unwrap();
    assert!(relate(&pool, program, wi2, SOAKS_LABEL, None, None)
        .await
        .is_err());
    assert!(unrelate(&pool, cover2).await.unwrap());
    relate(&pool, program, wi2, SOAKS_LABEL, None, None)
        .await
        .unwrap();

    assert!(unrelate(&pool, cover).await.unwrap());
}

#[tokio::test]
async fn soaks_pins_both_endpoint_kinds() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let program = program_over(&pool, "p", vec![]).await;
    let wi = soak_wi(&pool, "soak").await;
    let slice = proposal(&pool, "a proposal").await;

    // Right end must be a work item.
    assert!(relate(&pool, program, slice, SOAKS_LABEL, None, None)
        .await
        .is_err());
    // Left end must be a program.
    assert!(relate(&pool, slice, wi, SOAKS_LABEL, None, None)
        .await
        .is_err());
}

/// The near-miss hint is the proposal's named risk: a label spelled one way in
/// the registry and another in the hint is a string that looks right, parses
/// fine, and matches nothing.
#[tokio::test]
async fn a_misspelled_soaks_label_is_suggested() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let program = program_over(&pool, "p", vec![]).await;
    let wi = soak_wi(&pool, "soak").await;
    let err = relate(&pool, program, wi, "soak", None, None)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("did you mean 'soaks'?"),
        "the registry must suggest the real label: {err}"
    );
}

// --- #2152: the rollup ------------------------------------------------------

#[tokio::test]
async fn soaks_roll_up_on_get_program_and_the_board_in_rank_order() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let program = program_over(&pool, "rollup", vec![]).await;
    let first = soak_wi(&pool, "wave one").await;
    let second = soak_wi(&pool, "wave two").await;
    let unranked = soak_wi(&pool, "no rank").await;

    relate(&pool, program, unranked, SOAKS_LABEL, None, None)
        .await
        .unwrap();
    relate(
        &pool,
        program,
        second,
        SOAKS_LABEL,
        None,
        Some(Decimal::new(2, 0)),
    )
    .await
    .unwrap();
    relate(
        &pool,
        program,
        first,
        SOAKS_LABEL,
        None,
        Some(Decimal::new(1, 0)),
    )
    .await
    .unwrap();

    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(
        detail.soaks.iter().map(|s| s.wi_number).collect::<Vec<_>>(),
        vec![first, second, unranked],
        "rank order, unranked last — the `includes` convention"
    );
    let head = &detail.soaks[0];
    assert_eq!(head.title, "wave one");
    assert_eq!(head.node_id, head.wi_number, "0009: one number everywhere");
    assert_eq!(head.check_after, Some(date!(2026 - 09 - 13)));
    assert_eq!(
        head.invalidated_if.as_deref(),
        Some("kai's baseline is regenerated")
    );
    assert_eq!(head.wi_status, "open");
    assert_eq!(
        head.project.as_deref(),
        Some(korg_test_support::TEST_PROJECT)
    );

    // One fact, one place in the payload: `soaks` is not repeated in `related`.
    assert!(
        !detail.related.iter().any(|r| r.label == SOAKS_LABEL),
        "the array already carries these edges: {:?}",
        detail.related
    );

    // The board carries the same array, so a Delayed Ops row needs no
    // follow-up read.
    let board = board_rollup(&pool).await.unwrap();
    let on_board = board
        .programs
        .iter()
        .find(|p| p.program.node_id == program)
        .expect("a live program is on the board");
    assert_eq!(
        on_board
            .soaks
            .iter()
            .map(|s| s.wi_number)
            .collect::<Vec<_>>(),
        vec![first, second, unranked]
    );

    // A program with no soaks reports an empty array, never a missing key.
    let plain = program_over(&pool, "no soaks", vec![]).await;
    assert!(get_program_detail(&pool, plain)
        .await
        .unwrap()
        .unwrap()
        .soaks
        .is_empty());
}

/// A soaking program stays in the default `list_programs` and on the board.
/// This is the read half of `soaking_is_live_not_terminal` — the partition
/// constant being right is necessary, not sufficient.
#[tokio::test]
async fn a_soaking_program_stays_in_the_default_reads() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let (program, _) = soaking_program(&pool, "still watching").await;

    let listed = list_programs(&pool, None, ArchivedFilter::default())
        .await
        .unwrap();
    assert!(
        listed.items.iter().any(|p| p.node_id == program),
        "a soaking program is exactly the one somebody must still act on"
    );
    assert_eq!(
        listed.omitted.done, 0,
        "nothing was hidden, so nothing may be counted as hidden"
    );

    let board = board_rollup(&pool).await.unwrap();
    assert!(board.programs.iter().any(|p| p.program.node_id == program));
}

// --- #2154: reviewed --------------------------------------------------------

#[tokio::test]
async fn reports_are_born_unreviewed_and_toggle() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let r = create_report(&pool, new::report("kfo-soak", date!(2026 - 09 - 13)))
        .await
        .unwrap();
    let got = get_report(&pool, r.node_id).await.unwrap().unwrap();
    assert!(!got.row.reviewed, "nothing recorded that it was acted on");

    let after = set_report_reviewed(&pool, r.node_id, true).await.unwrap();
    assert!(after.row.reviewed);
    assert!(
        !set_report_reviewed(&pool, r.node_id, false)
            .await
            .unwrap()
            .row
            .reviewed,
        "the toggle goes both ways — a review can be taken back"
    );
}

/// The one piece of #2154 that is not a plain column. A same-day re-run keeps
/// the node id so comments survive, but the content is NEW — carrying a review
/// of the text it replaced forward would let a corrected report that raised a
/// fresh problem arrive already marked as dealt with.
#[tokio::test]
async fn a_same_day_rerun_resets_reviewed() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let day = date!(2026 - 09 - 13);
    let r = create_report(&pool, new::report("kfo-soak", day))
        .await
        .unwrap();
    set_report_reviewed(&pool, r.node_id, true).await.unwrap();

    let again = create_report(
        &pool,
        korg_core::repo::NewReport {
            status: "problem".into(),
            summary: "the soak failed after all".into(),
            ..new::report("kfo-soak", day)
        },
    )
    .await
    .unwrap();
    assert!(again.replaced);
    assert_eq!(again.node_id, r.node_id, "the node survives a re-run");
    assert!(
        !get_report(&pool, r.node_id)
            .await
            .unwrap()
            .unwrap()
            .row
            .reviewed,
        "new content has not been reviewed, whatever was true of what it replaced"
    );
}

#[tokio::test]
async fn list_reports_filters_reviewed_three_ways() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let seen = create_report(&pool, new::report("kfo-soak", date!(2026 - 09 - 11)))
        .await
        .unwrap();
    let unseen = create_report(&pool, new::report("kfo-soak", date!(2026 - 09 - 12)))
        .await
        .unwrap();
    set_report_reviewed(&pool, seen.node_id, true)
        .await
        .unwrap();

    let ids = |rows: Vec<korg_core::repo::ReportRow>| {
        let mut v: Vec<i64> = rows.into_iter().map(|r| r.node_id).collect();
        v.sort();
        v
    };
    let mut both = vec![seen.node_id, unseen.node_id];
    both.sort();

    assert_eq!(
        ids(list_reports(&pool, None, None, 50).await.unwrap()),
        both,
        "omitted means both — an existing caller sees what it always saw"
    );
    assert_eq!(
        ids(list_reports(&pool, None, Some(false), 50).await.unwrap()),
        vec![unseen.node_id],
        "the operations question: what still wants attention?"
    );
    assert_eq!(
        ids(list_reports(&pool, None, Some(true), 50).await.unwrap()),
        vec![seen.node_id]
    );

    // The board carries the flag so Sensor Net renders and filters without a
    // second read (GP-19: korg emits, the consumer filters).
    let board = board_rollup(&pool).await.unwrap();
    assert!(board
        .reports
        .iter()
        .any(|r| r.node_id == seen.node_id && r.reviewed));
    assert!(board
        .reports
        .iter()
        .any(|r| r.node_id == unseen.node_id && !r.reviewed));
}

#[tokio::test]
async fn reviewing_a_missing_report_is_not_found() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    assert!(set_report_reviewed(&pool, 999_999, true).await.is_err());
}
