//! Sprint 065 — korg quick wins (korg:1396).
//!
//! Three backlog-tail items with one thing in common: each is a surface that
//! reads worse than the data behind it.
//!
//! - #1398 — the Daily Reports source panel led with six unrated one-off probe
//!   rows and buried the one healthy regular at the bottom.
//! - #1146 — the images engine wrote an `:agent` variant *larger* than the
//!   original it re-encoded (fenced in `korg-img`'s own tests and
//!   `korg-api/tests/images.rs`, where the store and the serve path live).
//! - #466 — convention, not code.

use korg_core::repo::{list_report_sources, set_report_source, ReportSourcePatch};
use korg_test_support::fresh_korg;
use sqlx::PgPool;
use time::Date;

fn day(y: i32, m: u8, d: u8) -> Date {
    Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap()
}

async fn file_report(pool: &PgPool, source: &str, date: Date) {
    korg_core::repo::upsert_report(pool, korg_test_support::new::report(source, date))
        .await
        .expect("file a report");
}

/// A source korg can believe: enough consecutive daily reports to clear both
/// inference gates (`SOURCE_MIN_HISTORY` reports AND a span of
/// `SOURCE_MIN_SPAN_CADENCES` cadences). Ending the run `days_silent` days
/// before today is what decides `fresh` from `stale`.
async fn daily_source(pool: &PgPool, source: &str, days_silent: i64) {
    let last = time::OffsetDateTime::now_utc().date() - time::Duration::days(days_silent);
    for back in 0..8 {
        file_report(pool, source, last - time::Duration::days(back)).await;
    }
}

// === #1398: the healthy regulars lead, the unrateds tail ====================

/// Ordering is **stale → fresh → unrated → retired**, and the middle two are
/// the fix.
///
/// The old key was `is_stale DESC, last_report_date ASC NULLS FIRST`: correct
/// about stale, but it then interleaved unrated and fresh *by date*, and an
/// unrated row is unrated precisely because it reported once and stopped — so
/// its date is always old, and it always sorted above a source reporting every
/// day. On the live page that put six one-off probes above kmon, the only
/// source actually filing. The panel led with noise and buried its own signal.
///
/// Retired rows tail: a declared end is the one freshness that is neither an
/// alert nor a thing to judge.
#[tokio::test]
async fn fresh_sources_lead_the_unrated_ones_whatever_the_dates_say() {
    let (_pg, pool) = fresh_korg().await;

    // The healthy regular, reporting daily and current.
    daily_source(&pool, "kmon", 0).await;
    // The alert: a daily source that has gone quiet.
    daily_source(&pool, "kmon-dead", 9).await;
    // Two one-off probes. One report each, so no cadence can be inferred —
    // and both dated well BEFORE kmon's, which is what used to float them.
    file_report(&pool, "probe-a", day(2026, 1, 2)).await;
    file_report(&pool, "probe-b", day(2026, 1, 3)).await;
    // A declared end. Old dates too, so nothing but the rule can tail it.
    file_report(&pool, "retired-probe", day(2026, 1, 1)).await;
    set_report_source(
        &pool,
        "retired-probe",
        ReportSourcePatch {
            retired: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("retire a source");

    let sources = list_report_sources(&pool).await.unwrap();
    let order: Vec<(&str, &str)> = sources
        .iter()
        .map(|s| (s.source.as_str(), s.freshness.as_str()))
        .collect();

    assert_eq!(
        order,
        vec![
            ("kmon-dead", "stale"),
            ("kmon", "fresh"),
            ("probe-a", "unrated"),
            ("probe-b", "unrated"),
            ("retired-probe", "retired"),
        ],
        "stale first (it is the alert), then the sources korg can actually \
         believe, then the ones it cannot judge, then the ones it was told to \
         stop judging"
    );
}

/// Within a bucket the order is still oldest-first — the fix changed which
/// bucket a row lands in, not how rows inside one compare.
#[tokio::test]
async fn the_least_recently_heard_from_leads_its_own_bucket() {
    let (_pg, pool) = fresh_korg().await;

    daily_source(&pool, "kmon", 0).await;
    daily_source(&pool, "kaed", 1).await;

    let sources = list_report_sources(&pool).await.unwrap();
    let fresh: Vec<&str> = sources
        .iter()
        .filter(|s| s.freshness == "fresh")
        .map(|s| s.source.as_str())
        .collect();

    assert_eq!(
        fresh,
        vec!["kaed", "kmon"],
        "both are fresh; the one closer to its due date is the one worth \
         looking at first"
    );
}
