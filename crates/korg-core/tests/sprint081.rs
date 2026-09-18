//! Sprint 081 (#2183, korg:2814) — an explicit `on_demand` declaration, for a
//! source that has no cadence by design.
//!
//! Sprint 052 gated cadence inference on history *span* to stop korg inventing
//! a schedule for `kyac`, an interactive tool whose silence means nothing. That
//! gate worked for five weeks and then stopped working, and **how** it stopped
//! is the reason this sprint exists.
//!
//! `SOURCE_MIN_SPAN_CADENCES` is a ratio — span ≥ 7 × median gap — and only one
//! side of it was ever bounded. kyac's median gap stayed at 2 while its history
//! span grew from 5 days to 64, so the ratio crossed on its own, with no change
//! to kyac's behaviour and no change to korg. Read live on 2026-09-17: kyac
//! `stale`, 3 days overdue, on a 2-day cadence with `cadence_declared: false`.
//!
//! **The guard did not fail. It expired** — and every episodic source that keeps
//! filing occasionally will eventually cross it, so this is a property of the
//! approach and not of the threshold. That is exactly what 052's own comment
//! predicted when it wrote that the answer, if episodic sources became common,
//! was "an explicit 'on-demand' declaration, not a cleverer inference".
//!
//! The first test below reproduces the expired gate on the live timeline, so
//! that the thing this declaration fixes stays pinned in the suite rather than
//! being remembered as an argument.

use korg_core::error::RepoError;
use korg_core::repo::{list_report_sources, set_report_source, ReportSourcePatch, SourceHealth};
use korg_test_support::{fresh_korg, new};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};

async fn file_report(pool: &PgPool, source: &str, date: Date, status: &str) {
    let mut r = new::report(source, date);
    r.status = status.into();
    korg_core::repo::upsert_report(pool, r)
        .await
        .expect("file a report");
}

/// `kyac` as it actually stood on 2026-09-17, reproduced rather than invented:
/// six reports, median gap 2, history span 64 days, newest 7 days old.
///
/// The gaps are 56, 2, 2, 2, 2 — a long silence followed by a burst, which is
/// precisely the shape an episodic source produces and precisely the shape a
/// *continuous* median reads as "every 2 days". Span 64 ≥ 7 × 2, so the 052
/// gate passes it.
async fn kyac_shape(pool: &PgPool, source: &str) {
    let today = OffsetDateTime::now_utc().date();
    for back in [71, 15, 13, 11, 9, 7] {
        file_report(pool, source, today - Duration::days(back), "ok").await;
    }
}

fn source_of(rows: &[SourceHealth], name: &str) -> SourceHealth {
    rows.iter()
        .find(|s| s.source == name)
        .unwrap_or_else(|| panic!("no source {name}"))
        .clone()
}

/// The failure this sprint fixes, on the timeline it actually happened on.
///
/// This test asserts korg is **wrong**, deliberately. It is the before half of
/// the before/after, and if a future change to inference makes it fail, that
/// change has fixed something real and this test should be rewritten rather
/// than deleted — the declaration is still the right answer, because no
/// inference can ever establish that a source is *scheduled*.
#[tokio::test]
async fn the_expired_span_gate_invents_a_cadence_for_an_episodic_source() {
    let (_pg, pool) = fresh_korg().await;
    kyac_shape(&pool, "kyac").await;

    let kyac = source_of(&list_report_sources(&pool).await.unwrap(), "kyac");

    assert_eq!(
        kyac.freshness, "stale",
        "this is the live 2026-09-17 reading: the 052 span gate no longer \
         excludes kyac, because span grew while the median gap did not"
    );
    assert_eq!(kyac.cadence_days, Some(2));
    assert!(
        !kyac.cadence_declared,
        "and the cadence it is being judged against is one korg invented — \
         nobody declared 2 days for an interactive tool"
    );
    assert_eq!(kyac.history_span_days, Some(64));
    assert!(kyac.alerts(), "so it alerts, which is the false alarm");
}

/// The after half: one declaration, and every part of the invented schedule
/// goes with it.
///
/// The assertions past `freshness` are the point. Pinning only the literal
/// would leave `cadence_days` at 2 and `overdue_days` at 3, so the row would
/// read "on-demand, 3 days overdue" — a contradiction, and a worse one than the
/// plain `stale` it replaced, because it looks deliberate.
#[tokio::test]
async fn an_on_demand_declaration_removes_the_invented_schedule_entirely() {
    let (_pg, pool) = fresh_korg().await;
    kyac_shape(&pool, "kyac").await;

    let declared = set_report_source(
        &pool,
        "kyac",
        ReportSourcePatch {
            on_demand: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("declare on-demand");

    assert_eq!(declared.freshness, "on-demand");
    assert!(declared.on_demand);
    assert_eq!(
        declared.asserts, "unknown",
        "#950's rule is unchanged: anything not `fresh` asserts unknown"
    );
    assert!(
        !declared.alerts(),
        "an on-demand source has no cadence to be overdue against, so it never alerts"
    );
    assert_eq!(declared.cadence_days, None, "the invented cadence is gone");
    assert_eq!(declared.grace_days, None);
    assert_eq!(declared.due_by, None, "nothing is due, ever");
    assert_eq!(declared.overdue_days, 0);

    // The history is still reported — the declaration says korg must not judge
    // this source, not that it must forget it.
    assert_eq!(declared.report_count, 6);
    assert_eq!(declared.history_span_days, Some(64));
}

/// The difference from `unrated`, which is the whole reason this is a fifth
/// literal and not a pin to the fourth.
///
/// `unrated` means *korg cannot judge yet, and more reports will fix that* —
/// `history_span_days` exists so a consumer can show progress toward being
/// rated. An on-demand source is progressing toward nothing. Filing more
/// reports must never promote it, however schedule-shaped the history looks.
#[tokio::test]
async fn more_reports_never_promote_an_on_demand_source() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();

    set_report_source(
        &pool,
        "kfo-soak",
        ReportSourcePatch {
            on_demand: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("declare on-demand before the first report");

    // A textbook daily run — the most promotable history there is, and one that
    // clears both the count gate and the span gate with room to spare.
    for back in 0..40 {
        file_report(&pool, "kfo-soak", today - Duration::days(back), "ok").await;
    }

    let after = source_of(&list_report_sources(&pool).await.unwrap(), "kfo-soak");
    assert_eq!(
        after.freshness, "on-demand",
        "40 daily reports would promote any undeclared source; a declaration is \
         not evidence that can be outvoted by history"
    );
    assert_eq!(after.cadence_days, None);
    assert!(!after.alerts());
}

/// A declaration may be lifted: `on_demand: false` returns the source to
/// inference, which is what `null` does for the other overrides.
#[tokio::test]
async fn lifting_the_declaration_returns_the_source_to_inference() {
    let (_pg, pool) = fresh_korg().await;
    kyac_shape(&pool, "kyac").await;

    set_report_source(
        &pool,
        "kyac",
        ReportSourcePatch {
            on_demand: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let lifted = set_report_source(
        &pool,
        "kyac",
        ReportSourcePatch {
            on_demand: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        lifted.freshness, "stale",
        "back to what inference says — the declaration suppressed the judgement, \
         it did not erase the history behind it"
    );
    assert_eq!(lifted.cadence_days, Some(2));
}

/// `retired` outranks `on_demand`: a source may carry both flags, and "this
/// ended" is the stronger claim. `on_demand` describes how a *live* source
/// files.
#[tokio::test]
async fn retired_outranks_on_demand() {
    let (_pg, pool) = fresh_korg().await;
    kyac_shape(&pool, "probe").await;

    let both = set_report_source(
        &pool,
        "probe",
        ReportSourcePatch {
            on_demand: Some(true),
            retired: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(both.freshness, "retired");
    assert!(
        both.on_demand,
        "the flag is still reported — korg does not quietly drop a declaration \
         it is not currently acting on"
    );
}

/// Ordering: on-demand sits below the line with the other non-alerting states,
/// and above `retired` because it is live.
#[tokio::test]
async fn on_demand_sorts_below_the_line_and_above_retired() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();

    // One of each: stale, fresh, unrated, on-demand, retired.
    for back in 0..10 {
        file_report(&pool, "daily", today - Duration::days(back), "ok").await;
    }
    for back in 20..30 {
        file_report(&pool, "stopped", today - Duration::days(back), "ok").await;
    }
    file_report(&pool, "once", today - Duration::days(3), "ok").await;
    file_report(&pool, "episodic", today - Duration::days(3), "ok").await;
    file_report(&pool, "ended", today - Duration::days(3), "ok").await;
    set_report_source(
        &pool,
        "episodic",
        ReportSourcePatch {
            on_demand: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    set_report_source(
        &pool,
        "ended",
        ReportSourcePatch {
            retired: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let rows = list_report_sources(&pool).await.unwrap();
    let order: Vec<&str> = rows.iter().map(|s| s.freshness.as_str()).collect();
    assert_eq!(
        order,
        vec!["stale", "fresh", "unrated", "on-demand", "retired"],
        "most-alarming first, then the three korg is not worried about"
    );
}

/// Declaring `on_demand` and a cadence in one call is `invalid_input`: they are
/// contradictory claims about the same source, not two settings that disagree.
#[tokio::test]
async fn on_demand_and_a_cadence_cannot_both_be_declared() {
    let (_pg, pool) = fresh_korg().await;

    let err = set_report_source(
        &pool,
        "confused",
        ReportSourcePatch {
            on_demand: Some(true),
            cadence_days: Some(Some(7)),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::InvalidInput(_))
        ),
        "on_demand + cadence_days is invalid input, got {err:?}"
    );
    let msg = err.to_string();
    assert!(
        msg.contains("on_demand") && msg.contains("cadence"),
        "the refusal must name both fields — the caller has to know which of \
         their two claims to drop: {msg}"
    );

    // And nothing was written: a refused contradiction must not leave half of
    // itself behind.
    assert!(
        list_report_sources(&pool)
            .await
            .unwrap()
            .iter()
            .all(|s| s.source != "confused"),
        "the refused call created a source row anyway"
    );
}

/// The same contradiction assembled over two calls is refused the same way.
///
/// This is the case a same-call guard misses, and it is the likelier one in
/// practice: somebody declared a cadence months ago and is now correcting it.
/// The refusal names the fix rather than just the problem.
#[tokio::test]
async fn on_demand_refuses_a_row_that_already_declares_a_cadence() {
    let (_pg, pool) = fresh_korg().await;

    set_report_source(
        &pool,
        "weekly",
        ReportSourcePatch {
            cadence_days: Some(Some(7)),
            ..Default::default()
        },
    )
    .await
    .expect("declare a cadence");

    let err = set_report_source(
        &pool,
        "weekly",
        ReportSourcePatch {
            on_demand: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::InvalidInput(_))
        ),
        "got {err:?}"
    );
    assert!(
        msg.contains("cadence_days: null"),
        "the refusal must say how to proceed, not just that it refused: {msg}"
    );

    // Clearing the cadence in the same call is the documented way through.
    let ok = set_report_source(
        &pool,
        "weekly",
        ReportSourcePatch {
            on_demand: Some(true),
            cadence_days: Some(None),
            ..Default::default()
        },
    )
    .await
    .expect("clearing the cadence in the same call is accepted");
    assert_eq!(ok.freshness, "on-demand");
    assert_eq!(ok.cadence_days, None);
}

/// And the mirror: declaring a cadence on a source already declared on-demand.
#[tokio::test]
async fn a_cadence_refuses_a_row_that_is_already_on_demand() {
    let (_pg, pool) = fresh_korg().await;

    set_report_source(
        &pool,
        "episodic",
        ReportSourcePatch {
            on_demand: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let err = set_report_source(
        &pool,
        "episodic",
        ReportSourcePatch {
            cadence_days: Some(Some(3)),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    let msg = err.to_string();
    assert!(
        matches!(
            err.downcast_ref::<RepoError>(),
            Some(RepoError::InvalidInput(_))
        ),
        "got {err:?}"
    );
    assert!(
        msg.contains("on_demand: false"),
        "the refusal must name the lift: {msg}"
    );
}

/// Two contradictory declarations racing cannot build the row the check refuses
/// (overseer round 2 on korg:2814).
///
/// The check reads the row's resulting state rather than the call's fields,
/// which closes the two-**call** assembly path. Without a row lock the identical
/// row is still reachable by **timing**: both callers read the clean state, both
/// pass, both write, and the result carries `on_demand` *and* a cadence. It is
/// then invisible, because `judged` short-circuits on `on_demand` and never
/// consults the cadence again.
///
/// **The interleave is forced, not hoped for.** The obvious version of this test
/// — spawn both calls and see what happens — was written first and passed 5/5
/// against the *unfixed* code, because two fast calls do not collide on their
/// own. A concurrency test that cannot fail is the mirror-image failure GP-14
/// warns about: it asserts the opposite of its own name and nothing ever says
/// so. So this holds the row lock explicitly and proves both halves — that the
/// call blocks while the lock is held, and that it is refused once it can read
/// the committed state.
///
/// The row is created first on purpose. `FOR UPDATE` locks nothing when the row
/// does not exist, so starting from a missing row would exercise the one residue
/// the lock deliberately does not close.
#[tokio::test]
async fn a_contradictory_declaration_racing_another_cannot_land() {
    let (_pg, pool) = fresh_korg().await;

    set_report_source(
        &pool,
        "contended",
        ReportSourcePatch {
            note: Some(Some("neither on-demand nor on a cadence".into())),
            ..Default::default()
        },
    )
    .await
    .expect("seed the row");

    // Stand in for the winning caller, holding exactly the lock
    // `set_report_source` takes.
    let mut winner = pool.begin().await.expect("begin");
    sqlx::query("SELECT cadence_days, on_demand FROM report_source WHERE source = $1 FOR UPDATE")
        .bind("contended")
        .execute(&mut *winner)
        .await
        .expect("take the row lock");

    let loser = {
        let pool = pool.clone();
        tokio::spawn(async move {
            set_report_source(
                &pool,
                "contended",
                ReportSourcePatch {
                    cadence_days: Some(Some(7)),
                    ..Default::default()
                },
            )
            .await
        })
    };

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        !loser.is_finished(),
        "the competing call ran to completion while the row lock was held — it \
         never waited, so it read a state that was about to change under it"
    );

    // The winner declares on-demand and commits. The loser can now proceed.
    sqlx::query("UPDATE report_source SET on_demand = true WHERE source = $1")
        .bind("contended")
        .execute(&mut *winner)
        .await
        .expect("declare on-demand");
    winner.commit().await.expect("commit");

    let result = loser.await.expect("task");
    assert!(
        result.is_err(),
        "the losing call must re-read under the lock and be refused; instead it \
         landed a cadence on a source that is now on-demand, which is exactly \
         the row the check exists to prevent"
    );

    let row = source_of(&list_report_sources(&pool).await.unwrap(), "contended");
    assert!(
        !(row.on_demand && row.cadence_declared),
        "the row carries both on_demand and a declared cadence: {row:?}"
    );
    assert_eq!(row.freshness, "on-demand");
}
