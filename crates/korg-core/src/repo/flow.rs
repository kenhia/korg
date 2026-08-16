//! Backlog flow: opened and closed per day, the one time-series read (#1318).
//!
//! Split out of the board rollup it shipped beside — same sprint-059 surface,
//! but a series over the transition log rather than a snapshot of now.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use ts_rs::TS;

use super::common::report_date_fmt;

/// How many days of flow [`work_item_flow`] serves when the caller does not
/// say (#1318).
///
/// Six, not ten, and the number is a decision, not a guess (Ken, 2026-08-15):
/// a 6-day window sat entirely inside the transition log's coverage on the day
/// this shipped and only gets safer, so `closed` reads from transitions alone
/// — correct by construction, no `node.updated` close-time proxy, no fallback
/// branch. After 2026-08-18 a 10-day window is likewise fully covered;
/// widening is an edit to this line plus the fixtures, made deliberately.
pub const FLOW_DAYS_DEFAULT: i64 = 6;

/// The day the transition log begins — migration 0026, applied to production
/// 2026-08-08.
///
/// Days before this cannot answer `closed` honestly: the log was deliberately
/// not backfilled, so a series reaching further back would emit zeros that
/// read as "nothing closed" — the silent-wrong-answer failure mode #1318
/// exists to end. A window reaching past the horizon is clamped to it and the
/// response names the horizon, so the clamp is visible. A database created
/// after this date has a complete log for its whole life, so the clamp never
/// cuts real data there either.
pub const FLOW_TRANSITION_HORIZON: time::Date = time::macros::date!(2026 - 08 - 08);

/// What makes a work item durable rather than churn (#1318): it lived (or has
/// so far lived) **more than** this many days.
///
/// Measured on the live corpus 2026-08-15: 77.6% of all closed work items
/// lived under one day — sprint-internal task lists opened and closed in a
/// single session — so the raw added/closed series reads ~1:1 forever and the
/// durable split is where the signal is. Seven ≈ "crossed a week boundary",
/// the unit the backlog review that filed #1318 reasoned in.
///
/// A consequence worth knowing: `added_durable` is structurally zero for the
/// newest seven days of any series — an arrival's durability is only knowable
/// in retrospect — so at the 6-day default the whole column is zero until the
/// window widens past the threshold.
pub const FLOW_DURABLE_AFTER_DAYS: i64 = 7;

/// One day of work-item flow (#1318): what arrived, what closed, and where
/// the backlog stood at that day's end, with the churn-vs-durable split.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemFlowDay {
    /// The calendar day, in the timezone the series was computed in.
    #[serde(with = "report_date_fmt")]
    #[ts(type = "string")]
    pub day: time::Date,
    /// Work items created this day — from `node.created`, which is complete.
    /// Never from transitions: creating a work item writes no transition row,
    /// so an `added` derived from the log would be silently all zeros.
    pub added: i64,
    /// Work items that moved to `closed` this day, from the transition log.
    /// An item that closes on two different days in the window counts on both;
    /// two closes on one day count once.
    pub closed: i64,
    /// Open (non-`closed`) work items at this day's end, reconstructed from
    /// the transition log. Today's value equals what `list_work_items`
    /// reports as `total`.
    pub backlog: i64,
    /// The `added` that lived (or have so far lived) more than
    /// [`FLOW_DURABLE_AFTER_DAYS`] days.
    pub added_durable: i64,
    /// The `closed` that had lived more than [`FLOW_DURABLE_AFTER_DAYS`] days
    /// when they closed.
    pub closed_durable: i64,
}

/// The [`work_item_flow`] response (#1318) — korg's one time-series read.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemFlowSeries {
    /// One row per day, oldest first, ending today. Consumers take the series
    /// length from here rather than assuming the default window (kfdc #1319
    /// does), so widening the default needs no consumer edit.
    pub days: Vec<WorkItemFlowDay>,
    /// Where the transition log begins. A requested window reaching past this
    /// is clamped to it — days the log cannot answer are absent, never zeros.
    #[serde(with = "report_date_fmt")]
    #[ts(type = "string")]
    pub horizon: time::Date,
    /// The IANA timezone the days were bucketed in (`KORG_TIMEZONE`).
    pub timezone: String,
    /// The durable threshold, so a renderer can label the split without
    /// hardcoding it: [`FLOW_DURABLE_AFTER_DAYS`].
    pub durable_after_days: i64,
    /// Postgres's clock at generation — the same one every timestamp in the
    /// series came from, so ages computed against it are right.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub generated: OffsetDateTime,
}

/// Daily work-item flow over the last `days` days (#1318), one row per day in
/// timezone `tz`, oldest first, ending today.
///
/// The three sources, and why each is what it is:
///
/// - `added` comes from `node.created` — the one complete record of arrivals.
///   Creating a work item writes **no** transition row (verified on the live
///   corpus: all 170 then-open items had zero), so there is no alternative.
/// - `closed` and the backlog reconstruction come from the transition log,
///   which is only honest back to [`FLOW_TRANSITION_HORIZON`]; the window is
///   clamped there rather than zero-filled past it.
/// - `backlog` at a day's end is the current status un-walked: the status at
///   instant T is the `from_status` of the first transition after T, or the
///   current status when nothing moved since. Archived nodes are excluded
///   throughout, so today's row equals `list_work_items`'s `total`.
///
/// `days` is the caller's window (default [`FLOW_DAYS_DEFAULT`], floor 1);
/// `tz` is an IANA name, validated at config load, applied here in SQL so both
/// transports and every timestamp share Postgres's clock.
pub async fn work_item_flow(
    pool: &PgPool,
    days: Option<i64>,
    tz: &str,
) -> Result<WorkItemFlowSeries> {
    let days = days.unwrap_or(FLOW_DAYS_DEFAULT).max(1);
    let generated: OffsetDateTime = sqlx::query_scalar("SELECT now()").fetch_one(pool).await?;
    let rows = sqlx::query_as::<_, WorkItemFlowDay>(
        "WITH bounds AS ( \
            SELECT b0.today, \
                   b0.today - (LEAST($1, b0.today - $3::date + 1)::int - 1) AS start_day \
              FROM (SELECT (now() AT TIME ZONE $2::text)::date AS today) b0 \
         ), \
         wi AS ( \
            SELECT n.id, \
                   n.created, \
                   (n.created AT TIME ZONE $2::text)::date AS created_day, \
                   w.wi_status, \
                   (SELECT max(t.at) FROM transition t \
                     WHERE t.node_id = n.id AND t.to_status = 'closed') AS last_close_at \
              FROM node n \
              JOIN workitem w ON w.node_id = n.id \
             WHERE NOT n.archived \
         ), \
         closes AS ( \
            SELECT DISTINCT ON (t.node_id, ((t.at AT TIME ZONE $2::text)::date)) \
                   t.node_id, \
                   (t.at AT TIME ZONE $2::text)::date AS close_day, \
                   t.at, \
                   wi.created \
              FROM transition t \
              JOIN wi ON wi.id = t.node_id \
             WHERE t.to_status = 'closed' \
             ORDER BY t.node_id, ((t.at AT TIME ZONE $2::text)::date), t.at \
         ) \
         SELECT d.day, \
                (SELECT count(*) FROM wi WHERE wi.created_day = d.day) AS added, \
                (SELECT count(*) FROM closes c WHERE c.close_day = d.day) AS closed, \
                (SELECT count(*) FROM wi \
                  WHERE wi.created < ((d.day + 1)::timestamp AT TIME ZONE $2::text) \
                    AND COALESCE( \
                          (SELECT tr.from_status FROM transition tr \
                            WHERE tr.node_id = wi.id \
                              AND tr.at >= ((d.day + 1)::timestamp AT TIME ZONE $2::text) \
                            ORDER BY tr.at ASC, tr.id ASC \
                            LIMIT 1), \
                          wi.wi_status) <> 'closed') AS backlog, \
                (SELECT count(*) FROM wi \
                  WHERE wi.created_day = d.day \
                    AND (CASE WHEN wi.wi_status = 'closed' \
                              THEN COALESCE(wi.last_close_at, now()) \
                              ELSE now() END) - wi.created \
                        > make_interval(days => $4::int)) AS added_durable, \
                (SELECT count(*) FROM closes c \
                  WHERE c.close_day = d.day \
                    AND c.at - c.created > make_interval(days => $4::int)) AS closed_durable \
           FROM (SELECT generate_series(b.start_day, b.today, interval '1 day')::date AS day \
                   FROM bounds b) d \
          ORDER BY d.day",
    )
    .bind(days)
    .bind(tz)
    .bind(FLOW_TRANSITION_HORIZON)
    .bind(FLOW_DURABLE_AFTER_DAYS)
    .fetch_all(pool)
    .await?;
    Ok(WorkItemFlowSeries {
        days: rows,
        horizon: FLOW_TRANSITION_HORIZON,
        timezone: tz.to_owned(),
        durable_after_days: FLOW_DURABLE_AFTER_DAYS,
        generated,
    })
}
