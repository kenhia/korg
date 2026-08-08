//! Sprint 052 (#1097, korg:1098) — cadence inference gates on history *span*.
//!
//! Sprint 051 shipped staleness age-out and, on first contact with production,
//! raised exactly one alert: **`kyac`, stale, 21 days overdue.** It was wrong.
//!
//! kyac is interactive — it files a report when Ken prompts it, and there is no
//! schedule behind it, so its silence means nothing at all. korg saw four
//! reports over five days (2026-07-09, 07-11, 07-12, 07-14; gaps 2, 1, 2),
//! cleared `SOURCE_MIN_HISTORY`'s count gate, inferred a 2-day cadence and
//! declared it late.
//!
//! **The thing to understand before changing any of this:** a variance or
//! dispersion check would not have helped. Gaps of 2, 1, 2 are about as regular
//! as a series gets. A healthy episodic burst and a broken daily source are the
//! same shape, and history simply cannot tell you a source is *scheduled*. What
//! separates them is how long the pattern held — span, not regularity.
//!
//! Both real timelines are reproduced here verbatim rather than invented, so
//! the threshold stays pinned to the cases that actually motivated it.

use korg_core::repo::{list_report_sources, set_report_source, ReportSourcePatch};
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

/// `n` daily reports ending `ending_days_ago` — the kmon shape.
async fn daily_run(pool: &PgPool, source: &str, n: i64, ending_days_ago: i64) {
    let today = OffsetDateTime::now_utc().date();
    for i in 0..n {
        file_report(
            pool,
            source,
            today - Duration::days(ending_days_ago + i),
            "ok",
        )
        .await;
    }
}

/// **The regression.** kyac's exact July timeline, shifted so the burst is
/// recent enough that the old count-only gate would still have called it stale.
///
/// Four reports, evenly spaced, spanning five days. The old rule inferred a
/// 2-day cadence from them; the span rule refuses, because five days is 2.5
/// cadences and believing a cadence takes seven.
#[tokio::test]
async fn a_regular_burst_is_not_a_cadence() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();

    // 2026-07-09, 07-11, 07-12, 07-14 — gaps 2, 1, 2 — then silence.
    for back in [25, 27, 28, 30] {
        file_report(&pool, "kyac", today - Duration::days(back), "ok").await;
    }

    let sources = list_report_sources(&pool).await.unwrap();
    let kyac = sources.iter().find(|s| s.source == "kyac").unwrap();

    assert_eq!(
        kyac.freshness, "unrated",
        "four evenly-spaced reports across five days are a burst, not a cadence — \
         korg cannot judge this source and must say so"
    );
    assert!(
        !kyac.alerts(),
        "and it must NOT alert. This is the live false positive #1097 was filed \
         for: kyac is interactive, its silence means nothing, and an alerting \
         panel whose first-ever alert is wrong is a panel nobody will read."
    );
    assert_eq!(kyac.cadence_days, None, "no cadence was believed");
    assert_eq!(kyac.asserts, "unknown");

    // The count gate alone would have passed it — that is the whole point.
    assert_eq!(kyac.report_count, 4);
    assert_eq!(
        kyac.history_span_days,
        Some(5),
        "the span is what disqualified it, and it is carried so `unrated` is \
         explicable rather than mysterious"
    );
}

/// The other half: kmon's shape must keep working, or the fix has broken the
/// feature it is protecting. 23 daily reports spanning 34 days — 34 cadences,
/// comfortably past the threshold.
#[tokio::test]
async fn a_sustained_daily_source_still_earns_its_cadence() {
    let (_pg, pool) = fresh_korg().await;
    daily_run(&pool, "kmon", 23, 0).await;

    let sources = list_report_sources(&pool).await.unwrap();
    let kmon = sources.iter().find(|s| s.source == "kmon").unwrap();

    assert_eq!(kmon.freshness, "fresh");
    assert_eq!(
        kmon.asserts, "ok",
        "a fresh source still carries its own status"
    );
    assert_eq!(kmon.cadence_days, Some(1));
    assert_eq!(kmon.history_span_days, Some(22));
}

/// **The case the whole feature exists for must still fire.** A source that
/// genuinely filed daily for weeks and then stopped is still the alert — the
/// span gate must not have bought its false-positive fix by going quiet.
///
/// This is July 2026 replayed against the new rule: kmon filed daily for a
/// month, then nothing for eleven days.
#[tokio::test]
async fn a_long_running_daily_source_that_stops_is_still_the_alert() {
    let (_pg, pool) = fresh_korg().await;
    // 30 daily reports, the newest of them 11 days ago.
    daily_run(&pool, "kmon", 30, 11).await;

    let sources = list_report_sources(&pool).await.unwrap();
    let kmon = sources.iter().find(|s| s.source == "kmon").unwrap();

    assert_eq!(
        kmon.freshness, "stale",
        "a month of daily filing then eleven days of silence is exactly the \
         outage #950 was filed for — the span gate must not silence it"
    );
    assert!(kmon.alerts());
    assert_eq!(
        kmon.asserts, "unknown",
        "and it still asserts unknown rather than the last GREEN it filed"
    );
    assert!(kmon.overdue_days > 0);
}

/// A declaration still beats inference outright — the span gate applies only to
/// what korg guesses, never to what it was told.
///
/// This is the escape hatch for a real scheduled source that is too young to
/// have earned a cadence: say so, and it is judged from day one.
#[tokio::test]
async fn a_declared_cadence_bypasses_the_span_gate() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();

    // Same disqualifying shape as kyac.
    for back in [25, 27, 28, 30] {
        file_report(&pool, "newthing", today - Duration::days(back), "ok").await;
    }
    let before = list_report_sources(&pool).await.unwrap();
    assert_eq!(
        before
            .iter()
            .find(|s| s.source == "newthing")
            .unwrap()
            .freshness,
        "unrated"
    );

    let declared = set_report_source(
        &pool,
        "newthing",
        ReportSourcePatch {
            cadence_days: Some(Some(1)),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(
        declared.freshness, "stale",
        "a declared cadence is a statement that one is EXPECTED, which is the \
         thing inference can never establish — so it is judged on it immediately"
    );
    assert!(declared.cadence_declared);
    assert_eq!(declared.cadence_days, Some(1));
}

/// `retired` is still the answer for a source that has genuinely ended, and it
/// is deliberately *not* the answer for kyac.
///
/// #1097 called this out because the 051 record's follow-up suggested it: a
/// source silenced with the wrong meaning becomes a landmine the day it gains a
/// real schedule, and a silenced source nobody remembers to un-silence is #950's
/// failure wearing a different hat. The span rule reaches the right state
/// (`unrated`) without anyone declaring anything.
#[tokio::test]
async fn an_episodic_source_reaches_the_right_state_without_being_retired() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();
    for back in [25, 27, 28, 30] {
        file_report(&pool, "kyac", today - Duration::days(back), "ok").await;
    }

    let kyac = list_report_sources(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.source == "kyac")
        .unwrap();

    assert_eq!(kyac.freshness, "unrated");
    assert_ne!(
        kyac.freshness, "retired",
        "kyac has not ended — it files when prompted. `retired` would be a lie \
         that outlives the reason it was told."
    );
    assert!(kyac.note.is_none(), "and nobody had to declare anything");

    // ...and if kyac later gains a real schedule, filing on it is all it takes.
    daily_run(&pool, "kyac", 20, 0).await;
    let kyac = list_report_sources(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.source == "kyac")
        .unwrap();
    assert_eq!(
        kyac.freshness, "fresh",
        "a source that starts keeping a schedule is rated on it automatically — \
         no marker to remember to clear"
    );
    assert_eq!(kyac.cadence_days, Some(1));
}

/// The count gate and the span gate answer different questions and both stay.
/// Two reports give exactly one gap — an anecdote — however far apart they are.
#[tokio::test]
async fn two_distant_reports_are_still_not_a_cadence() {
    let (_pg, pool) = fresh_korg().await;
    let today = OffsetDateTime::now_utc().date();
    file_report(&pool, "sparse", today - Duration::days(90), "ok").await;
    file_report(&pool, "sparse", today - Duration::days(1), "ok").await;

    let sparse = list_report_sources(&pool)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.source == "sparse")
        .unwrap();

    assert_eq!(sparse.report_count, 2);
    assert_eq!(
        sparse.freshness, "unrated",
        "an 89-day span clears the span gate on its own, but one gap is still \
         one gap — SOURCE_MIN_HISTORY is a separate question and still applies"
    );
    assert!(!sparse.alerts());
}
