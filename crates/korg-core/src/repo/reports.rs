//! Daily reports (kmon et al.), and the staleness age-out that watches for the
//! report that stops arriving (#950).

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::vocab::{self, REPORT_STATUSES};

use super::common::{report_date_fmt, require_non_empty, touch_node, validate_status};
use super::work_items::node_id_for_wi;

// --- daily reports (kmon et al.) --------------------------------------------

/// `create_report`. `report_date` is `YYYY-MM-DD`; both transports used to
/// parse it by hand into a `time::Date` with their own error message.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewReport {
    /// reporter id, e.g. 'kmon'
    #[schemars(schema_with = "schema::non_empty")]
    pub source: String,
    #[serde(with = "report_date_fmt")]
    #[schemars(schema_with = "schema::report_date")]
    pub report_date: time::Date,
    #[schemars(schema_with = "schema::report_status")]
    pub status: String,
    /// one-liner for the list view
    #[schemars(schema_with = "schema::non_empty")]
    pub summary: String,
    /// full markdown report
    pub body: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub escalated: bool,
    /// wi_numbers of finding work items; numbers that don't resolve are dropped.
    #[serde(default, rename = "finding_work_items")]
    #[schemars(rename = "finding_work_items", schema_with = "schema::wi_numbers")]
    pub findings: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportRef {
    pub node_id: i64,
    pub replaced: bool,
    pub findings_linked: Vec<i64>,
}

/// Create or replace the report for (source, report_date). A same-day re-run
/// updates content in place and KEEPS the node_id, so relationships and
/// comments survive. The finding edge set (label 'finding') is *replaced*, not
/// accumulated (D-7): a corrected re-run that drops a finding drops its edge,
/// so `get_report.findings` reflects the latest run only.
pub async fn upsert_report(pool: &PgPool, new: NewReport) -> Result<ReportRef> {
    validate_status(&new.status, &REPORT_STATUSES, "report status")?;
    let mut resolved = Vec::with_capacity(new.findings.len());
    for wi in &new.findings {
        if let Some(n) = node_id_for_wi(pool, *wi).await? {
            resolved.push(n);
        }
    }

    let mut tx = pool.begin().await?;
    let existing: Option<i64> =
        sqlx::query("SELECT node_id FROM report WHERE source = $1 AND report_date = $2")
            .bind(&new.source)
            .bind(new.report_date)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| r.get("node_id"));

    let (node_id, replaced) = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE report SET status = $2, summary = $3, body = $4, model = $5, \
                 escalated = $6 WHERE node_id = $1",
            )
            .bind(id)
            .bind(&new.status)
            .bind(&new.summary)
            .bind(&new.body)
            .bind(&new.model)
            .bind(new.escalated)
            .execute(&mut *tx)
            .await?;
            touch_node(&mut *tx, id).await?;
            (id, true)
        }
        None => {
            let id: i64 = sqlx::query("INSERT INTO node (kind) VALUES ('report') RETURNING id")
                .fetch_one(&mut *tx)
                .await?
                .get("id");
            sqlx::query(
                "INSERT INTO report \
                 (node_id, source, report_date, status, summary, body, model, escalated) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(id)
            .bind(&new.source)
            .bind(new.report_date)
            .bind(&new.status)
            .bind(&new.summary)
            .bind(&new.body)
            .bind(&new.model)
            .bind(new.escalated)
            .execute(&mut *tx)
            .await?;
            (id, false)
        }
    };

    // Drop finding edges this run didn't produce. Matching on "the other end"
    // rather than on left_id keeps this correct for any pre-0014 edge that a
    // database might still carry unoriented.
    sqlx::query(
        "DELETE FROM relationship \
         WHERE relationship = 'finding' AND (left_id = $1 OR right_id = $1) \
           AND (CASE WHEN left_id = $1 THEN right_id ELSE left_id END) <> ALL($2)",
    )
    .bind(node_id)
    .bind(&resolved)
    .execute(&mut *tx)
    .await?;

    // Semantic orientation: report -> work item (WI #531). Provenance (D-17):
    // origin is this writer's operation name; ON CONFLICT preserves the
    // original created/origin on re-report.
    for &target in &resolved {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'finding', now(), 'create_report') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(node_id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(ReportRef {
        node_id,
        replaced,
        findings_linked: resolved,
    })
}

// === staleness age-out: absence of a write is the alert (#950, sprint 051) ===
//
// kmon crashed ~1s into collection every day from 2026-07-23 to 2026-08-03 — 11+
// consecutive runs. korg kept serving the last report kmon had successfully
// filed, a GREEN "OK" from 2026-07-22, so the Today page actively asserted the
// fleet was healthy *because* the thing checking it was broken. Ken found it by
// chance, not by signal.
//
// kmon #900 now files a RED report when it hits a blocking error, which covers
// the case where the process lives long enough to catch something. It does
// nothing for a process killed before its handler runs, a disabled timer, a down
// host, or — the actual July case — a crash early enough that the alerting code
// never ran. An age-out is the only check that catches all four, because it is
// the absence of a write, and absence is exactly what a tool can never report
// about itself.
//
// Two rules from the WI, neither optional, both fenced by tests:
//
//   1. A stale source asserts UNKNOWN, never its last known status.
//   2. It surfaces on the same surface a real problem would use — here,
//      `BoardRollup.sources`, beside Sensor Net's reports.

/// How many recent gaps the cadence inference looks at.
///
/// Long enough that a source which changed cadence months ago is judged on what
/// it does *now*, short enough that one very old irregular stretch cannot drag
/// the median. A median rather than a mean throughout: one missed day in a daily
/// series must not raise the expectation to 1.5 days and quietly widen the
/// window this feature exists to narrow.
const SOURCE_CADENCE_WINDOW: i64 = 30;

/// Below this many reports there is no cadence to infer, and the source is
/// [`vocab::SOURCE_FRESHNESS`]'s `unrated` unless it declares one. Two reports
/// give exactly one gap, which is an anecdote rather than a cadence.
const SOURCE_MIN_HISTORY: i64 = 3;

/// How many cadences the observed history must **span** before that cadence is
/// believed (#1097, sprint 052).
///
/// [`SOURCE_MIN_HISTORY`] counts reports, and counting reports is the wrong
/// question. `kyac` filed four times — 2026-07-09, 07-11, 07-12, 07-14, gaps of
/// 2, 1, 2 — cleared the count gate, and korg declared it 21 days overdue on a
/// 2-day cadence it had invented. kyac is interactive: it files when prompted,
/// and its silence means nothing at all.
///
/// **A variance check would not have caught this.** Gaps of 2, 1, 2 are about as
/// regular as a series gets. A healthy episodic burst and a broken daily source
/// are the same shape, and no amount of statistics recovers the difference,
/// because history cannot tell you a source is *scheduled*.
///
/// Span can. Measured live on 2026-08-08: kmon's history spanned 34 days at an
/// inferred cadence of 1 (34×); kyac's spanned 5 days at 2 (2.5×). Believability
/// comes from having watched the pattern *repeat over time*, not from having
/// collected several samples of it in one afternoon.
///
/// 7 sits well clear of both ends rather than just barely excluding kyac — a
/// threshold tuned to one data point is one that will be wrong on the next.
///
/// This gate is a trade, not a proof. Requiring an explicit declaration before
/// alerting would be airtight and would reintroduce #950's original failure: a
/// genuinely new scheduled source that nobody declared, silently unwatched. If
/// episodic sources become common the answer is an explicit "on-demand"
/// declaration, not a cleverer inference.
const SOURCE_MIN_SPAN_CADENCES: i64 = 7;

/// One reporting source's freshness — the #950 projection.
///
/// Read the two state fields together and note what is *not* here: there is no
/// `last_status` field. The last known status is deliberately unreachable from
/// this row, because a consumer holding it will eventually render it, and
/// rendering 2026-07-22's GREEN in a stale card is the precise failure this
/// exists to end. A caller that genuinely wants the history calls `list_reports`
/// for the source and knows exactly what it is doing.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct SourceHealth {
    pub source: String,
    /// `fresh` | `stale` | `retired` | `unrated` — see [`vocab::SOURCE_FRESHNESS`].
    pub freshness: String,
    /// `ok` | `attention` | `problem` | `unknown`. **`unknown` unless `fresh`.**
    pub asserts: String,
    /// The newest report's date, or `None` for a source declared before it has
    /// ever filed.
    #[serde(with = "report_date_fmt::option")]
    #[ts(type = "string | null")]
    pub last_report_date: Option<time::Date>,
    pub last_report_node_id: Option<i64>,
    /// Whole days between the newest report and today.
    pub days_since: Option<i64>,
    /// The cadence in use — declared if there is one, else the inferred median
    /// gap, else `None` when neither exists.
    pub cadence_days: Option<i64>,
    /// True when `cadence_days` came from `report_source`, false when it was
    /// inferred from history. The difference matters when triaging a false
    /// alarm: an inferred cadence is a guess korg made and can be overridden.
    pub cadence_declared: bool,
    pub grace_days: Option<i64>,
    /// The date by which the next report was expected. `None` when there is no
    /// cadence to project from.
    #[serde(with = "report_date_fmt::option")]
    #[ts(type = "string | null")]
    pub due_by: Option<time::Date>,
    /// Days past `due_by`; 0 when not overdue.
    pub overdue_days: i64,
    pub report_count: i64,
    /// Days from the oldest to the newest report korg looked at (#1097).
    ///
    /// Carried so `unrated` is **explicable rather than mysterious**: a source
    /// with five reports and a two-day span is one korg declined to judge, and
    /// this is the field that says so. It is also the number that moves as a new
    /// source earns a cadence, so a consumer can show progress toward being
    /// rated instead of a flat "unknown".
    pub history_span_days: Option<i64>,
    /// Why this source was retired, or any operator note from `report_source`.
    pub note: Option<String>,
}

impl SourceHealth {
    /// Whether this row should raise an alert on a board.
    ///
    /// Exactly one freshness value does. `retired` is a declared end, and
    /// `unrated` is korg admitting it cannot judge yet — alerting on either
    /// trains people to ignore the panel, and a channel nobody looks at is not a
    /// channel (`vocab::exactly_one_freshness_is_the_alert` fences the set).
    pub fn alerts(&self) -> bool {
        self.freshness == "stale"
    }
}

/// `list_report_sources` / `GET /api/report-sources` — every known source with
/// its freshness, most-alarming first.
///
/// Order is **stale → fresh → unrated → retired**, then oldest report first
/// inside each bucket. Ranking by freshness rather than by date is #1398's fix:
/// the key used to be `is_stale DESC, last_report_date ASC`, which was right
/// about stale and then sorted unrated against fresh *by date* — and an unrated
/// source is unrated precisely because it filed once and stopped, so its date is
/// always old. Six one-off probes therefore rendered above kmon, the only source
/// still filing. The panel led with noise and buried its own signal.
///
/// The whole derivation is one query, and it is a read: no state is stored about
/// staleness, which is what makes it immune to the failure mode it detects. A
/// stored `is_stale` flag needs something to maintain it, and that something can
/// die exactly the way kmon did.
///
/// The CTEs, in order: `gaps` takes each source's consecutive `report_date`
/// differences; `inferred` medians the most recent [`SOURCE_CADENCE_WINDOW`] of
/// them; `latest` and `agg` pick up the newest report and the count; `base`
/// FULL OUTER JOINs `report_source` so a declared-but-never-filed source still
/// appears; `judged` lets a declared cadence beat an inferred one and refuses to
/// infer below [`SOURCE_MIN_HISTORY`]; `graced` defaults grace to the cadence
/// itself, floor one day; `rated` resolves the freshness.
///
/// `rated` exists so freshness is decided **once**. It used to be an expression
/// repeated three times — in the SELECT, in the `asserts` guard and in the
/// ORDER BY — and three copies of a rule is three places for it to drift.
///
/// That collapse is also what makes **the #950 rule** readable rather than
/// merely true: `asserts` is now literally "fresh sources assert their status,
/// everything else asserts [`vocab::SOURCE_ASSERTS_UNKNOWN`]", which is the rule
/// as written. The old spelling re-derived freshness from its four parts and
/// happened to agree.
///
/// **Do not put `--` comments in this string.** The literal is `\`-continued, so
/// no newline survives into the SQL and a `--` silently comments out the entire
/// remainder of the query — it arrives as `syntax error at end of input`, which
/// points nowhere near the cause. Explanations belong here, in the doc comment.
pub async fn list_report_sources(pool: &PgPool) -> Result<Vec<SourceHealth>> {
    let rows = sqlx::query_as::<_, SourceHealth>(&format!(
        "WITH gaps AS ( \
             SELECT source, report_date, \
                    report_date - lag(report_date) OVER w AS gap, \
                    row_number() OVER (PARTITION BY source ORDER BY report_date DESC) AS recency \
               FROM report \
             WINDOW w AS (PARTITION BY source ORDER BY report_date) \
         ), \
         inferred AS ( \
             SELECT source, \
                    ceil(percentile_cont(0.5) WITHIN GROUP (ORDER BY gap))::bigint AS median_gap \
               FROM gaps WHERE gap IS NOT NULL AND recency <= {SOURCE_CADENCE_WINDOW} \
              GROUP BY source \
         ), \
         observed AS ( \
             SELECT source, \
                    (max(report_date) - min(report_date))::bigint AS span_days \
               FROM gaps WHERE recency <= {SOURCE_CADENCE_WINDOW} \
              GROUP BY source \
         ), \
         latest AS ( \
             SELECT DISTINCT ON (r.source) r.source, r.report_date, r.node_id, r.status \
               FROM report r ORDER BY r.source, r.report_date DESC, r.node_id DESC \
         ), \
         agg AS ( \
             SELECT source, count(*)::bigint AS report_count FROM report GROUP BY source \
         ), \
         base AS ( \
             SELECT coalesce(l.source, rs.source)                        AS source, \
                    l.report_date                                        AS last_report_date, \
                    l.node_id                                            AS last_report_node_id, \
                    l.status                                             AS last_status, \
                    coalesce(a.report_count, 0)                          AS report_count, \
                    coalesce(rs.retired, false)                          AS retired, \
                    rs.note                                              AS note, \
                    rs.cadence_days                                      AS declared_cadence, \
                    rs.grace_days                                        AS declared_grace, \
                    i.median_gap                                         AS inferred_cadence, \
                    o.span_days                                          AS history_span_days \
               FROM latest l \
               FULL OUTER JOIN report_source rs ON rs.source = l.source \
               LEFT JOIN agg      a ON a.source = coalesce(l.source, rs.source) \
               LEFT JOIN inferred i ON i.source = coalesce(l.source, rs.source) \
               LEFT JOIN observed o ON o.source = coalesce(l.source, rs.source) \
         ), \
         judged AS ( \
             SELECT b.*, \
                    coalesce(b.declared_cadence::bigint, \
                             CASE WHEN b.report_count >= {SOURCE_MIN_HISTORY} \
                                   AND b.history_span_days >= \
                                       {SOURCE_MIN_SPAN_CADENCES} * b.inferred_cadence \
                                  THEN b.inferred_cadence END)          AS cadence_days \
               FROM base b \
         ), \
         graced AS ( \
             SELECT j.*, \
                    CASE WHEN j.cadence_days IS NULL THEN NULL \
                         ELSE coalesce(j.declared_grace::bigint, greatest(1, j.cadence_days)) \
                    END                                                  AS grace_days \
               FROM judged j \
         ), \
         rated AS ( \
             SELECT g.*, \
                    CASE WHEN g.retired                     THEN 'retired' \
                         WHEN g.cadence_days IS NULL \
                           OR g.last_report_date IS NULL     THEN 'unrated' \
                         WHEN (current_date - g.last_report_date) \
                                > (g.cadence_days + g.grace_days) THEN 'stale' \
                         ELSE 'fresh' END                                AS freshness \
               FROM graced g \
         ) \
         SELECT r.source, r.freshness, \
                CASE WHEN r.freshness = 'fresh' \
                     THEN r.last_status ELSE '{}' END                    AS asserts, \
                r.last_report_date, r.last_report_node_id, \
                (current_date - r.last_report_date)::bigint              AS days_since, \
                r.cadence_days, \
                (r.declared_cadence IS NOT NULL)                         AS cadence_declared, \
                r.grace_days, \
                (r.last_report_date + (r.cadence_days + r.grace_days)::int) AS due_by, \
                greatest(0, coalesce((current_date - r.last_report_date) \
                            - (r.cadence_days + r.grace_days), 0))::bigint AS overdue_days, \
                r.report_count, r.history_span_days, r.note \
           FROM rated r \
          ORDER BY CASE r.freshness WHEN 'stale'   THEN 0 \
                                    WHEN 'fresh'   THEN 1 \
                                    WHEN 'unrated' THEN 2 \
                                    ELSE                3 END, \
                   r.last_report_date ASC NULLS FIRST, r.source ASC",
        vocab::SOURCE_ASSERTS_UNKNOWN
    ))
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `set_report_source` / `PATCH /api/report-sources/:source` — declare a
/// cadence, a grace window, or retirement.
///
/// Every field is an override; leaving one unset keeps the derivation. The row
/// may be written **before** the source's first report, which is how a new daily
/// source skips `unrated` on day one rather than spending three days in it.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ReportSourcePatch {
    /// Expected days between reports. `null` clears the override and returns the
    /// source to inference from its own history.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub cadence_days: Option<Option<i64>>,
    /// Slack past the cadence before `stale`. `null` returns it to the default,
    /// which is the cadence itself (floor 1 day).
    #[serde(default, deserialize_with = "ops::double_option")]
    pub grace_days: Option<Option<i64>>,
    /// Mark a source deliberately ended. This is the answer to #950's open
    /// question — how a retired source stops nagging without also silencing a
    /// broken one. It is an explicit declaration, so "we turned this off" stays
    /// distinguishable from "this died", which silence alone can never do.
    #[serde(default)]
    pub retired: Option<bool>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub note: Option<Option<String>>,
}

pub async fn set_report_source(
    pool: &PgPool,
    source: &str,
    patch: ReportSourcePatch,
) -> Result<SourceHealth> {
    require_non_empty(source, "source")?;
    if let Some(Some(v)) = patch.cadence_days {
        if v <= 0 {
            return Err(RepoError::invalid(format!(
                "cadence_days must be positive, got {v} — pass null to return this \
                 source to cadence inference from its own report history"
            ))
            .into());
        }
    }
    if let Some(Some(v)) = patch.grace_days {
        if v < 0 {
            return Err(
                RepoError::invalid(format!("grace_days must not be negative, got {v}")).into(),
            );
        }
    }

    sqlx::query(
        "INSERT INTO report_source (source, cadence_days, grace_days, retired, note) \
         VALUES ($1, $2, $3, coalesce($4, false), $5) \
         ON CONFLICT (source) DO UPDATE SET \
             cadence_days = CASE WHEN $6 THEN EXCLUDED.cadence_days ELSE report_source.cadence_days END, \
             grace_days   = CASE WHEN $7 THEN EXCLUDED.grace_days   ELSE report_source.grace_days   END, \
             retired      = coalesce($4, report_source.retired), \
             note         = CASE WHEN $8 THEN EXCLUDED.note         ELSE report_source.note         END, \
             updated      = now()",
    )
    .bind(source)
    .bind(patch.cadence_days.flatten().map(|v| v as i32))
    .bind(patch.grace_days.flatten().map(|v| v as i32))
    .bind(patch.retired)
    .bind(patch.note.clone().flatten())
    .bind(patch.cadence_days.is_some())
    .bind(patch.grace_days.is_some())
    .bind(patch.note.is_some())
    .execute(pool)
    .await?;

    list_report_sources(pool)
        .await?
        .into_iter()
        .find(|s| s.source == source)
        .ok_or_else(|| RepoError::NotFound(format!("no report source '{source}'")).into())
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportRow {
    pub node_id: i64,
    pub source: String,
    #[serde(with = "report_date_fmt")]
    #[ts(type = "string")]
    pub report_date: time::Date,
    pub status: String,
    pub summary: String,
    pub model: Option<String>,
    pub escalated: bool,
    /// Comments on this report (WI #535).
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// Newest first; summary fields only (the list view).
pub async fn list_reports(
    pool: &PgPool,
    source: Option<&str>,
    limit: i64,
) -> Result<Vec<ReportRow>> {
    let rows = sqlx::query_as::<_, ReportRow>(
        "SELECT r.node_id, r.source, r.report_date, r.status, r.summary, r.model, \
                r.escalated, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = r.node_id) AS comment_count, \
                n.updated \
         FROM report r JOIN node n ON n.id = r.node_id \
         WHERE ($1::text IS NULL OR r.source = $1) \
         ORDER BY r.report_date DESC, r.source ASC LIMIT $2",
    )
    .bind(source)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportFinding {
    pub wi_number: i64,
    pub title: String,
    pub wi_status: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportFull {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: ReportRow,
    pub body: String,
    pub findings: Vec<ReportFinding>,
}

/// One report with body + linked findings ('finding' edges to work items).
pub async fn get_report(pool: &PgPool, node_id: i64) -> Result<Option<ReportFull>> {
    let Some(r) = sqlx::query(
        "SELECT r.node_id, r.source, r.report_date, r.status, r.summary, r.model, \
                r.escalated, r.body, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = r.node_id) AS comment_count, \
                n.updated \
         FROM report r JOIN node n ON n.id = r.node_id WHERE r.node_id = $1",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let findings = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT w.wi_number, w.title, w.wi_status \
         FROM relationship rel \
         JOIN workitem w ON w.node_id = CASE WHEN rel.left_id = $1 THEN rel.right_id ELSE rel.left_id END \
         WHERE (rel.left_id = $1 OR rel.right_id = $1) AND rel.relationship = 'finding' \
         ORDER BY w.wi_number",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(wi_number, title, wi_status)| ReportFinding { wi_number, title, wi_status })
    .collect();
    Ok(Some(ReportFull {
        row: ReportRow {
            node_id: r.get("node_id"),
            source: r.get("source"),
            report_date: r.get("report_date"),
            status: r.get("status"),
            summary: r.get("summary"),
            model: r.get("model"),
            escalated: r.get("escalated"),
            comment_count: r.get("comment_count"),
            updated: r.get("updated"),
        },
        body: r.get("body"),
        findings,
    }))
}
