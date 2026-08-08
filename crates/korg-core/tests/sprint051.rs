//! Sprint 051 (#581 + #950, korg:1079) — korg's first time-derived state.
//!
//! Two work items, one mechanism: **a date crossing now makes something
//! appear.** #581 is a stored date arriving; #950 is the *absence* of a write
//! past a stored cadence.
//!
//! The rules this suite pins, and why each is a rule rather than a habit:
//!
//! 1. **Nothing runs unattended.** Due-ness is computed on every read; a
//!    schedule that is due materialises only when someone asks. #950 exists
//!    because an unattended scheduled thing died quietly for eleven days, so
//!    korg's answer to time must not itself be one.
//! 2. **A due schedule with an outstanding item is not due.** The work item IS
//!    the surface; the schedule stops competing with it. This is the
//!    duplicate-materialisation failure a scheduler would have had, and `force`
//!    deliberately does not lift it.
//! 3. **`once` is not a special case.** It is the cadence whose interval is
//!    zero, which is what let the one-shot and the quarterly drill share a node
//!    shape — the saving korg:1079 argued the rejected slice would have lost.
//! 4. **The two anchor styles differ only in which event moves the anchor.**
//!    Both are one column.
//! 5. **An unknown substitution is refused at write time**, not rendered
//!    literally into a title months later.
//! 6. **A stale source asserts UNKNOWN.** Never its last known status — the
//!    single most important assertion in the file, and the one that replays the
//!    July 2026 timeline directly.

use korg_core::repo::{
    board_rollup, create_schedule, get_schedule, get_schedule_detail, list_report_sources,
    list_schedules, materialize_schedule, set_report_source, update_schedule, update_work_item,
    ArchivedFilter, NewSchedule, ReportSourcePatch, SchedulePatch, WorkItemPatch,
};
use korg_core::vocab;
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};

fn days_ago(n: i64) -> OffsetDateTime {
    OffsetDateTime::now_utc() - Duration::days(n)
}

/// `create_report` is the public writer; reports are keyed by (source, date).
async fn file_report(pool: &PgPool, source: &str, date: Date, status: &str) {
    let mut r = new::report(source, date);
    r.status = status.into();
    korg_core::repo::upsert_report(pool, r)
        .await
        .expect("file a report");
}

fn day(y: i32, m: u8, d: u8) -> Date {
    Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap()
}

// === #581: schedules ========================================================

/// The originating case, end to end: the quarterly restore drill, seeded with
/// the date it was actually last verified (2026-07-08, WI #234), does **not**
/// come due the moment it is filed.
///
/// This is the cold-start problem, and getting it wrong would make the feature
/// unusable on day one — every schedule filed for existing maintenance would
/// stampede at once, which is precisely the "queue that lies" #581 set out to
/// avoid.
#[tokio::test]
async fn a_seeded_quarterly_drill_is_not_due_on_the_day_it_is_filed() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule(
            "korg restore drill - {MONTH} {YEAR} (Quarterly)",
            "quarterly",
            Some(days_ago(31)),
        ),
    )
    .await
    .unwrap();

    assert!(!s.due, "a drill run 31 days ago is not due again quarterly");
    assert!(s.due_at > OffsetDateTime::now_utc());
    assert_eq!(s.last_wi_number, None);
    assert_eq!(s.materialized_count, 0);

    // ...and it IS due once the quarter has passed.
    let aged = update_schedule(
        &pool,
        s.node_id,
        SchedulePatch {
            anchor_at: Some(days_ago(100)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(aged.due, "100 days > one quarter, so the drill is due");
}

/// Materialising renders the substitutions, files the item under the schedule's
/// project with the `maintenance` type, and records provenance.
#[tokio::test]
async fn materializing_renders_the_template_and_records_provenance() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let mut spec = new::schedule(
        "korg restore drill - {MONTH} {YEAR} (Quarterly)",
        "quarterly",
        Some(days_ago(100)),
    );
    spec.template = Some("Restore the {DATE} dump into a scratch database.".into());
    let s = create_schedule(&pool, spec).await.unwrap();
    assert!(s.due);

    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    // The title rendered — no braces survive, and the month is a name.
    assert!(
        !out.work_item.title.contains('{'),
        "an unsubstituted placeholder survived: {}",
        out.work_item.title
    );
    let year = OffsetDateTime::now_utc().year().to_string();
    assert!(
        out.work_item.title.contains(&year),
        "{{YEAR}} did not render: {}",
        out.work_item.title
    );
    assert!(
        out.work_item.title.starts_with("korg restore drill - "),
        "the literal part of the template was mangled: {}",
        out.work_item.title
    );

    // The distinct type #581 asked for, so automation can find generated items.
    assert_eq!(out.work_item.wi_type, "maintenance");
    assert_eq!(out.work_item.wi_status, "open");
    assert_eq!(out.work_item.project.as_deref(), Some(TEST_PROJECT));

    // Provenance is the edge; `last_wi_id` is only the newest pointer.
    let detail = get_schedule_detail(&pool, s.node_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.materialized.len(), 1);
    assert_eq!(detail.materialized[0].wi_number, out.work_item.wi_number);
    assert_eq!(detail.schedule.materialized_count, 1);
    assert_eq!(
        detail.schedule.last_wi_number,
        Some(out.work_item.wi_number)
    );
}

/// **Rule 2.** A schedule whose materialised item is still open is not due
/// again, and `force` does not lift it — that is the duplicate-materialisation
/// failure mode a daily tick would have had, and it is the whole reason korg
/// runs no tick.
#[tokio::test]
async fn an_outstanding_item_suppresses_the_schedule_and_force_does_not_lift_it() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("monthly check", "monthly", Some(days_ago(400))),
    )
    .await
    .unwrap();
    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    // Anchored 400 days ago, so the interval is long past — and yet:
    assert!(
        !out.schedule.due,
        "a schedule with an open materialisation must not read as due"
    );
    assert!(out.schedule.outstanding);

    let err = materialize_schedule(&pool, s.node_id, true)
        .await
        .expect_err("force must not produce a second copy of an open drill");
    let msg = err.to_string();
    assert!(
        msg.contains("still") && msg.contains("force"),
        "the refusal should say what it is and that force does not lift it: {msg}"
    );

    // Closing the item releases the schedule.
    update_work_item(
        &pool,
        out.work_item.wi_number,
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let after = get_schedule(&pool, s.node_id).await.unwrap().unwrap();
    assert!(!after.outstanding);
}

/// **Rule 4, `completed` anchor.** Finishing the work item is what restarts the
/// clock — the korg-core write rule (D-4) that 0025 deliberately did not make a
/// trigger.
#[tokio::test]
async fn completing_the_item_advances_a_completed_anchor() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("weekly sweep", "weekly", Some(days_ago(30))),
    )
    .await
    .unwrap();
    let before = s.anchor_at;
    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    // Materialising alone must NOT move a `completed` anchor.
    assert_eq!(
        out.schedule.anchor_at, before,
        "a `completed` anchor waits for the work to be done, not merely raised"
    );

    // `resolved` is deliberately not completion — the drill has not been run.
    update_work_item(
        &pool,
        out.work_item.wi_number,
        WorkItemPatch {
            wi_status: Some("resolved".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mid = get_schedule(&pool, s.node_id).await.unwrap().unwrap();
    assert_eq!(
        mid.anchor_at, before,
        "`resolved` means 'may still need a user test' — a drill in that state \
         has not been performed, so the quarter must not restart from it"
    );
    assert!(mid.outstanding, "and it still counts as outstanding");

    update_work_item(
        &pool,
        out.work_item.wi_number,
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let after = get_schedule(&pool, s.node_id).await.unwrap().unwrap();
    assert!(
        after.anchor_at > before,
        "completing the item is what advances a `completed` anchor"
    );
    assert!(!after.due, "and the next occurrence is a week out again");
}

/// **Rule 4, `created` anchor.** The other style advances at materialisation,
/// without waiting for anyone to finish anything.
#[tokio::test]
async fn a_created_anchor_advances_at_materialization() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let mut spec = new::schedule("monthly report", "monthly", Some(days_ago(60)));
    spec.anchor_mode = "created".into();
    let s = create_schedule(&pool, spec).await.unwrap();
    let before = s.anchor_at;

    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();
    assert!(
        out.schedule.anchor_at > before,
        "a `created` anchor moves the moment the item is raised"
    );
}

/// **Rule 3.** A one-shot is the zero-interval cadence: due at its anchor, and
/// finished the moment it fires. This is the DST-recheck case from #581.
#[tokio::test]
async fn a_one_shot_fires_at_its_date_and_is_then_done() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let future = create_schedule(
        &pool,
        new::schedule(
            "recheck the timers after the DST transition",
            "once",
            Some(OffsetDateTime::now_utc() + Duration::days(30)),
        ),
    )
    .await
    .unwrap();
    assert!(!future.due, "a one-shot dated forward is not due yet");
    assert_eq!(
        future.due_at, future.anchor_at,
        "`once` has a zero interval, so its anchor IS its fire date"
    );

    let err = materialize_schedule(&pool, future.node_id, false)
        .await
        .expect_err("a schedule that is not due is refused");
    assert!(
        err.to_string().contains("force"),
        "the refusal must say what would lift it: {err}"
    );
    // ...and force is exactly what lifts *this* one (unlike the outstanding
    // check above), because bringing a drill forward is a real need.
    materialize_schedule(&pool, future.node_id, true)
        .await
        .expect("force materializes a not-yet-due schedule");

    let done = get_schedule(&pool, future.node_id).await.unwrap().unwrap();
    assert_eq!(
        done.status, "done",
        "a one-shot has no second occurrence, so firing it finishes it"
    );

    // A done schedule refuses to fire again even forced. Close the item it
    // produced first, so the refusal under test is the *status* one rather than
    // the outstanding-item one, which is checked earlier and would mask it.
    update_work_item(
        &pool,
        done.last_wi_number.unwrap(),
        WorkItemPatch {
            wi_status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let err = materialize_schedule(&pool, future.node_id, true)
        .await
        .expect_err("a done one-shot must not fire twice");
    let msg = err.to_string();
    assert!(
        msg.contains("done") && msg.contains("active"),
        "the refusal should name the state and what would resume it: {msg}"
    );
}

/// **Rule 5.** A typo'd placeholder is refused at write time, with the full
/// allowed set in the message — korg's standard vocabulary-error idiom. The
/// alternative is `{MONTL}` rendering literally into a title three months from
/// now, which is the silent-wrong class this sprint removes.
#[tokio::test]
async fn an_unknown_substitution_is_refused_with_the_allowed_set() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let err = create_schedule(
        &pool,
        new::schedule("drill - {MONTL} {YEAR}", "quarterly", None),
    )
    .await
    .expect_err("an unknown placeholder must be refused");
    let msg = err.to_string();
    assert!(msg.contains("{MONTL}"), "name the offender: {msg}");
    assert!(msg.contains("{MONTH}"), "list what IS allowed: {msg}");

    // Prose braces are not placeholders and must stay writable — a JSON snippet
    // in a template body would otherwise be unstorable.
    let mut spec = new::schedule("check the config", "monthly", None);
    spec.template = Some("Confirm {\"retain\": 30} is still set.".into());
    create_schedule(&pool, spec)
        .await
        .expect("prose braces are not substitutions");
}

/// Every cadence in the vocabulary must have an interval arm in the SQL `CASE`.
///
/// The fence [`repo::SCHEDULE_DUE_SQL`]'s doc comment promises: a cadence added
/// to `vocab::SCHEDULE_CADENCES` without a `CASE` arm yields a NULL `due_at`,
/// and this drives every value through the real query rather than a Rust-side
/// copy of the mapping (which would be the second home for one fact).
#[tokio::test]
async fn every_cadence_has_an_interval() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    for cadence in vocab::SCHEDULE_CADENCES {
        let s = create_schedule(
            &pool,
            new::schedule(&format!("{cadence} thing"), cadence, Some(days_ago(1))),
        )
        .await
        .unwrap_or_else(|e| panic!("create a {cadence} schedule: {e}"));
        // Deserializing at all proves due_at was non-NULL; the ordering below
        // proves it is a real instant rather than the anchor echoed back.
        assert!(
            s.due_at >= s.anchor_at,
            "{cadence}: due_at must not precede the anchor"
        );
        if cadence == "once" {
            assert_eq!(s.due_at, s.anchor_at, "once has a zero interval");
        } else {
            assert!(s.due_at > s.anchor_at, "{cadence} must advance the anchor");
        }
    }
}

/// `list_schedules` is soonest-due-first, and `omitted.not_due` keeps "nothing
/// is due" distinguishable from "I filtered it out".
#[tokio::test]
async fn the_list_is_soonest_due_first_and_counts_what_due_only_hid() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    create_schedule(
        &pool,
        new::schedule("overdue", "weekly", Some(days_ago(60))),
    )
    .await
    .unwrap();
    create_schedule(&pool, new::schedule("not yet", "yearly", Some(days_ago(1))))
        .await
        .unwrap();

    let all = list_schedules(&pool, None, None, false, ArchivedFilter::default())
        .await
        .unwrap();
    assert_eq!(all.items.len(), 2);
    assert_eq!(
        all.items[0].title, "overdue",
        "soonest-due first: the row that wants doing is the first one"
    );
    assert_eq!(
        all.omitted.not_due, 0,
        "nothing was filtered, so nothing hid"
    );

    let due = list_schedules(&pool, None, None, true, ArchivedFilter::default())
        .await
        .unwrap();
    assert_eq!(due.items.len(), 1);
    assert_eq!(due.items[0].title, "overdue");
    assert_eq!(
        due.omitted.not_due, 1,
        "a due_only read must say how many rows it hid"
    );
}

/// `paused` stops a schedule surfacing without losing its anchor or history —
/// the schedule-side analogue of retiring a report source.
#[tokio::test]
async fn pausing_stops_a_schedule_surfacing_without_losing_its_anchor() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("noisy check", "weekly", Some(days_ago(60))),
    )
    .await
    .unwrap();
    assert!(s.due);

    let paused = update_schedule(
        &pool,
        s.node_id,
        SchedulePatch {
            status: Some("paused".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!paused.due, "a paused schedule never comes due");
    assert_eq!(paused.anchor_at, s.anchor_at, "and it keeps its anchor");

    materialize_schedule(&pool, s.node_id, true)
        .await
        .expect_err("a paused schedule cannot be materialized, even forced");

    // Resuming restores due-ness from the anchor it kept.
    let resumed = update_schedule(
        &pool,
        s.node_id,
        SchedulePatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(resumed.due);
}

/// A schedule that is due on the day it is filed still waits for an explicit
/// write. **Nothing runs unattended** (rule 1) — the design claim this whole
/// sprint rests on, asserted rather than assumed.
#[tokio::test]
async fn creating_a_due_schedule_materializes_nothing() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("already due", "weekly", Some(days_ago(90))),
    )
    .await
    .unwrap();
    assert!(s.due, "it is due...");
    assert_eq!(
        s.materialized_count, 0,
        "...and nothing was created for it. korg runs no scheduler: that is the \
         point, because #950 exists because an unattended scheduled thing died \
         quietly for eleven days."
    );
    let items = korg_core::repo::list_work_items(&pool, Default::default())
        .await
        .unwrap();
    assert_eq!(items.total, 0, "no work item appeared on its own");
}

// === #950: staleness age-out ================================================

/// **The July 2026 timeline, replayed.** kmon filed daily, then stopped on
/// 2026-07-22 and was not noticed until 2026-08-03.
///
/// This is the assertion the work item exists for: the stale source asserts
/// `unknown`, **not** the GREEN it last filed. Restating that GREEN is exactly
/// how korg told Ken the fleet was healthy for eleven days *because* the thing
/// checking it was broken.
#[tokio::test]
async fn a_source_that_filed_daily_and_stopped_asserts_unknown_not_its_last_green() {
    let (_pg, pool) = fresh_korg().await;

    // Fourteen consecutive daily "ok" reports, the last of them long ago.
    // Dates are fixed history, so the gap to `current_date` is enormous — which
    // is what "stopped filing" means.
    for d in 9..=22 {
        file_report(&pool, "kmon", day(2026, 7, d), "ok").await;
    }

    let sources = list_report_sources(&pool).await.unwrap();
    let kmon = sources.iter().find(|s| s.source == "kmon").unwrap();

    assert_eq!(kmon.freshness, "stale");
    assert_eq!(
        kmon.asserts, "unknown",
        "THE #950 RULE: a stale source asserts UNKNOWN. Serving its last 'ok' \
         here is precisely the failure that let korg report a healthy fleet for \
         eleven days while kmon was crashing 1s into every run."
    );
    assert!(kmon.alerts(), "and it is the alert, not a quiet row");
    assert_eq!(
        kmon.cadence_days,
        Some(1),
        "daily filing is inferred from the history itself — nothing to configure"
    );
    assert!(!kmon.cadence_declared);
    assert_eq!(kmon.last_report_date, Some(day(2026, 7, 22)));
    assert!(kmon.overdue_days > 0);

    // The structural half of the same rule: the projection has no field that
    // could carry the last status at all. This is checked through the serialized
    // shape, because that is what a consumer actually receives.
    let json = serde_json::to_value(kmon).unwrap();
    for forbidden in ["last_status", "status", "last_report_status"] {
        assert!(
            json.get(forbidden).is_none(),
            "SourceHealth must not expose `{forbidden}` — a consumer holding a \
             stale source's last status will eventually render it"
        );
    }
}

/// A source filing on time is fresh and *does* carry its real status — the
/// other half of the rule, without which the projection would be useless.
#[tokio::test]
async fn a_current_source_asserts_what_it_actually_said() {
    let (_pg, pool) = fresh_korg().await;

    let today = OffsetDateTime::now_utc().date();
    for back in (0..5).rev() {
        file_report(&pool, "kmon", today - Duration::days(back), "attention").await;
    }

    let sources = list_report_sources(&pool).await.unwrap();
    let kmon = sources.iter().find(|s| s.source == "kmon").unwrap();
    assert_eq!(kmon.freshness, "fresh");
    assert_eq!(
        kmon.asserts, "attention",
        "a fresh source's own status is exactly what it asserts"
    );
    assert!(!kmon.alerts());
    assert_eq!(kmon.overdue_days, 0);
}

/// Grace: a daily source goes stale on the **third** silent day, not the first.
/// One missed run is a blip; three days of silence is a dead tool.
#[tokio::test]
async fn a_daily_source_survives_one_missed_day_and_goes_stale_on_the_third() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();

    // History establishing a daily cadence, ending two days ago.
    for back in 2..8 {
        file_report(&pool, "daily", today - Duration::days(back), "ok").await;
    }
    let s = list_report_sources(&pool).await.unwrap();
    let d = s.iter().find(|s| s.source == "daily").unwrap();
    assert_eq!(d.cadence_days, Some(1));
    assert_eq!(
        d.grace_days,
        Some(1),
        "grace defaults to the cadence, floor 1"
    );
    assert_eq!(
        d.freshness, "fresh",
        "two days silent is cadence(1) + grace(1) — not yet stale"
    );

    // A source whose last report is three days old has crossed the line.
    for back in 3..9 {
        file_report(&pool, "quiet", today - Duration::days(back), "ok").await;
    }
    let s = list_report_sources(&pool).await.unwrap();
    let q = s.iter().find(|s| s.source == "quiet").unwrap();
    assert_eq!(q.freshness, "stale");
    assert_eq!(q.asserts, "unknown");
}

/// **`unrated`, and why it is not a guess.** A source with too little history
/// has no inferable cadence. Calling it `fresh` rebuilds the July failure;
/// calling it `stale` cries wolf on every one-off report. It says what is true
/// and does not alert — and a declared cadence is how a real source leaves the
/// state immediately.
#[tokio::test]
async fn too_little_history_is_unrated_and_a_declared_cadence_settles_it() {
    let (_pg, pool) = fresh_korg().await;

    file_report(&pool, "oneoff", day(2026, 3, 1), "ok").await;
    let s = list_report_sources(&pool).await.unwrap();
    let o = s.iter().find(|s| s.source == "oneoff").unwrap();
    assert_eq!(o.freshness, "unrated");
    assert_eq!(
        o.asserts, "unknown",
        "korg cannot vouch for it, so it says so"
    );
    assert!(
        !o.alerts(),
        "and it must NOT alert — a panel that cries wolf on one-off reports is a \
         panel nobody looks at, which is the failure #950 names directly"
    );
    assert_eq!(o.cadence_days, None);

    // Declaring the cadence resolves it — and here that means stale, correctly:
    // a source declared daily whose only report is from March is dead.
    let declared = set_report_source(
        &pool,
        "oneoff",
        ReportSourcePatch {
            cadence_days: Some(Some(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(declared.freshness, "stale");
    assert!(declared.cadence_declared);
    assert_eq!(declared.cadence_days, Some(1));
}

/// **Retirement, #950's other open question.** A deliberately-ended source must
/// stop nagging *without* the same mechanism silencing a broken one — so it is
/// an explicit declaration, and it is reversible.
#[tokio::test]
async fn retiring_a_source_silences_it_without_silencing_a_broken_one() {
    let (_pg, pool) = fresh_korg().await;

    for d in 9..=22 {
        file_report(&pool, "old-tool", day(2026, 7, d), "ok").await;
        file_report(&pool, "broken-tool", day(2026, 7, d), "ok").await;
    }

    // Both are stale to begin with — identical evidence, because silence alone
    // genuinely cannot tell them apart. That is the point.
    let before = list_report_sources(&pool).await.unwrap();
    assert!(before.iter().all(|s| s.freshness == "stale"));

    set_report_source(
        &pool,
        "old-tool",
        ReportSourcePatch {
            retired: Some(true),
            note: Some(Some("replaced by kmon in sprint 06".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let after = list_report_sources(&pool).await.unwrap();
    let old = after.iter().find(|s| s.source == "old-tool").unwrap();
    let broken = after.iter().find(|s| s.source == "broken-tool").unwrap();

    assert_eq!(old.freshness, "retired");
    assert!(!old.alerts(), "a declared end is not an alert");
    assert_eq!(
        old.asserts, "unknown",
        "retired still asserts unknown — it has no current claim to make"
    );
    assert_eq!(old.note.as_deref(), Some("replaced by kmon in sprint 06"));

    assert_eq!(
        broken.freshness, "stale",
        "the untouched source is still the alert — retiring one must not be a \
         blanket mute"
    );
    assert!(broken.alerts());

    // Reversible: un-retiring restores the alert.
    let revived = set_report_source(
        &pool,
        "old-tool",
        ReportSourcePatch {
            retired: Some(false),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(revived.freshness, "stale");
}

/// A declaration may be written before the source has ever filed — how a new
/// daily source skips `unrated` on day one instead of spending three days in it.
#[tokio::test]
async fn a_source_can_be_declared_before_its_first_report() {
    let (_pg, pool) = fresh_korg().await;

    let s = set_report_source(
        &pool,
        "brand-new",
        ReportSourcePatch {
            cadence_days: Some(Some(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(s.report_count, 0);
    assert_eq!(s.last_report_date, None);
    assert_eq!(
        s.freshness, "unrated",
        "declared but never filed: there is no last report to be late relative to"
    );
    assert_eq!(s.asserts, "unknown");

    // Once it files, the declaration is what judges it.
    file_report(&pool, "brand-new", OffsetDateTime::now_utc().date(), "ok").await;
    let s = list_report_sources(&pool).await.unwrap();
    let n = s.iter().find(|s| s.source == "brand-new").unwrap();
    assert_eq!(n.freshness, "fresh");
    assert_eq!(n.asserts, "ok");
}

/// Overrides are individually clearable back to derivation — `null` means
/// "derive it again", not "set it to nothing".
#[tokio::test]
async fn clearing_a_declared_cadence_returns_the_source_to_inference() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();
    for back in 0..6 {
        file_report(&pool, "kmon", today - Duration::days(back), "ok").await;
    }

    let declared = set_report_source(
        &pool,
        "kmon",
        ReportSourcePatch {
            cadence_days: Some(Some(14)),
            note: Some(Some("keep me".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(declared.cadence_days, Some(14));
    assert!(declared.cadence_declared);

    // Clearing only the cadence must leave the note alone — a patch touches
    // exactly what it names.
    let cleared = set_report_source(
        &pool,
        "kmon",
        ReportSourcePatch {
            cadence_days: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cleared.cadence_days,
        Some(1),
        "back to the inferred daily cadence"
    );
    assert!(!cleared.cadence_declared);
    assert_eq!(cleared.note.as_deref(), Some("keep me"));
}

/// The median is what makes inference robust: one missed day in an otherwise
/// daily series must not widen the window this feature exists to narrow.
#[tokio::test]
async fn cadence_inference_uses_the_median_so_one_gap_cannot_skew_it() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();

    // Daily for a fortnight, except one four-day hole. A mean would read ~1.2
    // and round to 2; the median stays 1.
    for back in 0..15 {
        if (5..8).contains(&back) {
            continue;
        }
        file_report(&pool, "kmon", today - Duration::days(back), "ok").await;
    }

    let s = list_report_sources(&pool).await.unwrap();
    let kmon = s.iter().find(|s| s.source == "kmon").unwrap();
    assert_eq!(
        kmon.cadence_days,
        Some(1),
        "a single hole in a daily series is an outlier, not a new cadence"
    );
}

/// **Rule: it surfaces where a real problem would.** `get_board` is the one
/// call a dashboard makes, so the staleness signal rides it — a channel nobody
/// looks at is not a channel.
#[tokio::test]
async fn the_board_carries_source_freshness_beside_its_reports() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    for d in 9..=22 {
        file_report(&pool, "kmon", day(2026, 7, d), "ok").await;
    }

    let board = board_rollup(&pool).await.unwrap();
    assert!(
        !board.reports.is_empty(),
        "Sensor Net still carries the reports themselves"
    );
    let kmon = board
        .sources
        .iter()
        .find(|s| s.source == "kmon")
        .expect("the board must carry source freshness (#950)");
    assert_eq!(kmon.freshness, "stale");
    assert_eq!(
        kmon.asserts, "unknown",
        "the board is exactly where restating a stale GREEN did the damage"
    );

    // Ordering: the alert sorts first, so a truncating renderer cannot drop it.
    assert!(board.sources[0].alerts() || board.sources.iter().all(|s| !s.alerts()));
}

// === cross-cutting ==========================================================

/// A schedule is a node like any other, so the generic surfaces must accept it.
/// The `materializes` edge in particular has to be reachable from `neighbors`,
/// because that is how a consumer walks provenance without a bespoke read.
#[tokio::test]
async fn a_schedule_is_an_ordinary_node_for_the_generic_surfaces() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("drill", "quarterly", Some(days_ago(200))),
    )
    .await
    .unwrap();
    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    let edges = korg_core::repo::neighbors(&pool, s.node_id, Default::default())
        .await
        .unwrap();
    let m = edges
        .items
        .iter()
        .find(|e| e.label == "materializes")
        .expect("the provenance edge is reachable from neighbors");
    assert_eq!(m.direction, "out", "schedule -> workitem, directed");
    assert!(m.directed, "the orientation carries meaning");
    assert_eq!(m.node_id, out.work_item.node_id);

    // And the node preview resolves it, so an edge to a schedule renders.
    let preview = korg_core::repo::get_node_preview(&pool, s.node_id)
        .await
        .unwrap()
        .expect("a schedule has a preview like any node");
    assert_eq!(preview.kind, "schedule");
}

/// Deleting a materialised work item must forget the generation, not cascade
/// into the schedule — `ON DELETE SET NULL`, asserted rather than assumed,
/// because the alternative silently destroys maintenance history.
#[tokio::test]
async fn deleting_a_materialized_item_does_not_delete_its_schedule() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(
        &pool,
        new::schedule("drill", "quarterly", Some(days_ago(200))),
    )
    .await
    .unwrap();
    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    sqlx::query("DELETE FROM node WHERE id = $1")
        .bind(out.work_item.node_id)
        .execute(&pool)
        .await
        .unwrap();

    let after = get_schedule(&pool, s.node_id)
        .await
        .unwrap()
        .expect("the schedule survives its work item");
    assert_eq!(after.last_wi_number, None);
    assert!(!after.outstanding);
    assert!(
        after.due,
        "and it is due again, having lost its materialisation"
    );
}

/// The default `NewSchedule` path: no anchor given means "starting now", which
/// is right for something being set up today and is the reason the field is
/// optional rather than required.
#[tokio::test]
async fn an_unseeded_schedule_anchors_now() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let s = create_schedule(&pool, new::schedule("new habit", "weekly", None))
        .await
        .unwrap();
    assert!(!s.due, "a weekly schedule started today is not due today");
    let drift = (OffsetDateTime::now_utc() - s.anchor_at).abs();
    assert!(drift < Duration::minutes(1), "anchored at roughly now");
}

/// A schedule created with an explicit non-default type and size passes both
/// through to the item it produces — the template really is a template.
#[tokio::test]
async fn the_template_carries_type_and_size_onto_the_item() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let mut spec: NewSchedule = new::schedule("audit", "yearly", Some(days_ago(400)));
    spec.wi_type = "chore".into();
    spec.wi_tshirt = "L".into();
    let s = create_schedule(&pool, spec).await.unwrap();
    let out = materialize_schedule(&pool, s.node_id, false).await.unwrap();

    assert_eq!(out.work_item.wi_type, "chore");
    assert_eq!(out.work_item.wi_tshirt, "L");
}
