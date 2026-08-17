//! Schedules: work that a date makes appear (#581, sprint 051)
//!
//! korg had no time-derived state at all. A quarterly restore drill, a monthly
//! check, a one-shot "look at this again after the DST transition" — each could
//! only be written as prose on a node that was often already `done`, i.e. out of
//! every default view. #1036 needed a whole work item created just to keep
//! "deploy this board later" alive after its proposal closed.
//!
//! The durable thing is the SCHEDULE: a work-item template, a cadence, and an
//! anchor. **"Due" is a read-time predicate** — `anchor_at + interval <= now()`
//! — and materialising the work item is an explicit write (D-2). #581's own
//! sketch assumed a daily engine; korg:1079 argued that out, and the argument is
//! worth keeping in front of whoever changes this: #950, the work item this one
//! shipped beside, exists precisely because an unattended scheduled thing died
//! quietly for eleven days. Building korg's answer to time on another unattended
//! tick would be an uncomfortable joke, and it would add the duplicate-
//! materialisation failure a double firing creates.

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Transaction};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::vocab::{self, WI_FINISHED_STATUSES};

use super::awaiting::settle_awaiting;
use super::comments::Comment;
use super::common::{require_kind, require_non_empty, touch_node, validate_status};
use super::page::ArchivedFilter;
use super::relationships::{related_context, RelatedRef};
use super::selectors::resolve_project;
use super::work_items::{get_work_item, WorkItemRow, WORKITEM_COMMENT_CAP};

/// How long each cadence waits lives **only** in the SQL `CASE` below and in
/// [`schedule_select`], deliberately: a Rust-side lookup table would be a second
/// home for one fact, and the two would drift the way every hand-kept copy in
/// this codebase has. `once` is **zero**, which is the whole reason the one-shot
/// needed no second storage shape — its `anchor_at` simply *is* its fire date.
///
/// The exhaustiveness risk is real and is fenced by a test rather than by the
/// type system: a cadence added to [`vocab::SCHEDULE_CADENCES`] without a `CASE`
/// arm here makes `due_at` NULL, which would fail to deserialize rather than
/// quietly mis-sort. `every_cadence_has_an_interval` (sprint051) drives every
/// vocabulary value through the real query.
///
/// The SQL fragment that decides due-ness, shared by every read so the
/// definition cannot fork. Depends on `s` (schedule) and a `now` expression.
///
/// Three clauses, and the middle one is the interesting one:
///
/// 1. the schedule is `active` — `paused` and `done` never come due;
/// 2. no **outstanding** materialisation — while the drill's work item is open,
///    the drill has already surfaced, and the work item IS the surface. This is
///    the anti-duplicate rule and the honesty rule at once;
/// 3. the interval has elapsed since `anchor_at`.
///
/// "Outstanding" is [`vocab::WI_UNFINISHED_STATUSES`], derived rather than
/// spelled — see [`outstanding_sql`].
///
/// `resolved` counts as outstanding here. A restore drill that is "implemented,
/// may still need a user test" has not been performed, and re-raising it because
/// the status *looks* terminal would be the feature nagging about work already
/// in flight. That used to read "unlike everywhere else in korg", which stopped
/// being true in sprint 053 when #978 named this exact set
/// `WI_UNFINISHED_STATUSES`; the two now agree by construction instead of by
/// coincidence.
fn schedule_due_sql() -> String {
    format!(
        "s.status = 'active' \
     AND NOT EXISTS (SELECT 1 FROM workitem lw WHERE lw.node_id = s.last_wi_id \
                       AND {}) \
     AND s.anchor_at + (CASE s.cadence \
            WHEN 'once'      THEN interval '0 days' \
            WHEN 'weekly'    THEN interval '7 days' \
            WHEN 'monthly'   THEN interval '1 month' \
            WHEN 'quarterly' THEN interval '3 months' \
            WHEN 'yearly'    THEN interval '1 year' \
         END) <= now()",
        outstanding_sql("lw.wi_status")
    )
}

/// `<column> IN (…)` over [`vocab::WI_UNFINISHED_STATUSES`] — the one place a
/// schedule decides whether the item it already produced is still the surface.
///
/// Written as a helper because the set was hardcoded as `('open', 'resolved')`
/// in two statements, and #810 made that latently wrong: `parked` is unfinished,
/// so parking a materialised drill would have made its schedule read "nothing
/// outstanding" and fire again, quietly producing a duplicate work item for work
/// deliberately deferred. Values are from the vocabulary and contain no user
/// input, so inlining them is not an injection surface.
fn outstanding_sql(column: &str) -> String {
    let list = vocab::WI_UNFINISHED_STATUSES
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{column} IN ({list})")
}

/// `create_schedule` / `POST /api/schedules`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewSchedule {
    /// The **template** title of the work item this materialises. May carry the
    /// substitutions in [`vocab::SCHEDULE_SUBSTITUTIONS`], e.g.
    /// `"korg restore drill - {MONTH} {YEAR} (Quarterly)"`. An unrecognised
    /// `{PLACEHOLDER}` is refused, not rendered literally.
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    /// The template body of the materialised work item — the checklist, the
    /// procedure, the link to the runbook. Substituted the same way.
    #[serde(default)]
    pub template: Option<String>,
    /// Why this schedule exists and what "done" means for it. Stays on the
    /// schedule; not copied into the materialised item.
    #[serde(default)]
    pub notes: Option<String>,
    #[schemars(schema_with = "schema::schedule_cadence")]
    pub cadence: String,
    /// Which event advances the anchor — see [`vocab::SCHEDULE_ANCHORS`].
    /// Defaults to `completed`, the style #581 listed first and the one that
    /// matches how maintenance is actually reasoned about ("it's been three
    /// months since we last *did* it").
    #[serde(default = "ops::default_completed")]
    #[schemars(schema_with = "schema::schedule_anchor")]
    pub anchor_mode: String,
    /// When the interval starts counting. **Seed this with the real last-done
    /// date** — the restore drill was last verified 2026-07-08, so a quarterly
    /// schedule anchored there comes due 2026-10-08 rather than immediately.
    /// Omitted means now, which is right for something being started today and
    /// wrong for anything with history.
    ///
    /// For cadence `once` this IS the fire date.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schemars(schema_with = "schema::timestamp_opt")]
    pub anchor_at: Option<OffsetDateTime>,
    /// What the materialised item is filed as. Defaults to `maintenance`, the
    /// type #581 asked for so automation can find generated items as a group.
    #[serde(default = "ops::default_maintenance")]
    #[schemars(schema_with = "schema::wi_type")]
    pub wi_type: String,
    #[serde(default = "ops::default_unknown")]
    #[schemars(schema_with = "schema::wi_tshirt")]
    pub wi_tshirt: String,
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name — the alternative to `project_id`, never both. A schedule
    /// carries a project (unlike a program): maintenance is maintenance *of*
    /// something.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
}

/// A schedule row, with due-ness already computed (D-2).
///
/// `due` and `due_at` are **derived on every read**, never stored. A stored
/// `due` flag is a cached predicate, and a cached predicate over a clock is the
/// thing that goes stale silently — which is the failure this whole sprint is
/// about.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ScheduleRow {
    pub node_id: i64,
    /// The template title, verbatim and unsubstituted. `preview_title` is what
    /// it renders to right now.
    pub title: String,
    pub template: Option<String>,
    pub notes: Option<String>,
    pub cadence: String,
    pub anchor_mode: String,
    pub status: String,
    pub wi_type: String,
    pub wi_tshirt: String,
    pub project: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub anchor_at: OffsetDateTime,
    /// When this schedule comes due: `anchor_at` + the cadence interval. For a
    /// `once` schedule it equals `anchor_at`.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub due_at: OffsetDateTime,
    /// The read-time predicate. See [`SCHEDULE_DUE_SQL`] for the three clauses.
    pub due: bool,
    /// The newest materialised work item, or `None` if this has never fired.
    pub last_wi_number: Option<i64>,
    /// Whether that item is still unfinished — [`vocab::WI_UNFINISHED_STATUSES`],
    /// so `parked` counts too (#810) — the clause that stops a schedule
    /// competing with the work item it already produced. This doc said
    /// `open`/`resolved` until #1387: the set it names moved in 054 and the
    /// sentence did not.
    pub outstanding: bool,
    /// How many work items this schedule has produced, over all time (the
    /// `materializes` edges, not the `last_wi_id` pointer).
    pub materialized_count: i64,
    /// The title as it would render **right now**, substitutions applied. Lets a
    /// UI show what pressing Materialise will actually create without a dry-run
    /// endpoint.
    pub preview_title: String,
    pub archived: bool,
    pub comment_count: i64,
    pub tags: Vec<String>,
    pub category: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// The `replace(...)` nest that renders [`vocab::SCHEDULE_SUBSTITUTIONS`] over a
/// column, from **Postgres's** `now()`.
///
/// Rendered database-side for the reason [`BoardRollup::generated`] gives: a
/// date derived from a different clock than the rows beside it goes subtly wrong
/// on a cached or proxied consumer. `{MONTH}` uses `to_char(…,'Month')`, which
/// blank-pads to nine characters, hence the `trim`.
pub(super) fn substituted(column: &str) -> String {
    format!(
        "replace(replace(replace(replace(replace({column}, \
            '{{YEAR}}',    to_char(now(), 'YYYY')), \
            '{{MONTH}}',   trim(to_char(now(), 'Month'))), \
            '{{DAY}}',     to_char(now(), 'DD')), \
            '{{DATE}}',    to_char(now(), 'YYYY-MM-DD')), \
            '{{QUARTER}}', 'Q' || to_char(now(), 'Q'))"
    )
}

/// Every schedule read goes through this projection, so `due` has exactly one
/// definition.
fn schedule_select() -> String {
    format!(
        "SELECT s.node_id, s.title, s.template, s.notes, s.cadence, s.anchor_mode, \
                s.status, s.wi_type, s.wi_tshirt, pj.name AS project, s.anchor_at, \
                s.anchor_at + (CASE s.cadence \
                    WHEN 'once'      THEN interval '0 days' \
                    WHEN 'weekly'    THEN interval '7 days' \
                    WHEN 'monthly'   THEN interval '1 month' \
                    WHEN 'quarterly' THEN interval '3 months' \
                    WHEN 'yearly'    THEN interval '1 year' \
                 END) AS due_at, \
                ({}) AS due, \
                lw.wi_number AS last_wi_number, \
                coalesce({}, false) AS outstanding, \
                (SELECT count(*) FROM relationship mr \
                  WHERE mr.left_id = s.node_id AND mr.relationship = 'materializes') \
                  AS materialized_count, \
                {} AS preview_title, \
                n.archived, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = s.node_id) AS comment_count, \
                n.tags, n.category, n.created, n.updated \
         FROM schedule s \
         JOIN node n ON n.id = s.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         LEFT JOIN workitem lw ON lw.node_id = s.last_wi_id",
        schedule_due_sql(),
        outstanding_sql("lw.wi_status"),
        substituted("s.title")
    )
}

/// Reject a template that uses a placeholder korg does not know (D-6).
///
/// The alternative — rendering `{MONTL}` literally into a work-item title three
/// months from now — is exactly the silent-wrong class this sprint removes.
/// Anything that is not `{ALLCAPS_}` is left alone: prose legitimately contains
/// braces, and a JSON snippet in a template body must not become unwritable.
fn check_substitutions(template: &str, what: &str) -> Result<()> {
    let bytes = template.as_bytes();
    let mut i = 0;
    while let Some(open) = template[i..].find('{').map(|p| p + i) {
        let Some(close) = template[open..].find('}').map(|p| p + open) else {
            break;
        };
        let name = &template[open + 1..close];
        let placeholder_shaped = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'_' || b.is_ascii_digit());
        if placeholder_shaped && !vocab::SCHEDULE_SUBSTITUTIONS.contains(&name) {
            return Err(RepoError::InvalidInput(format!(
                "unknown substitution '{{{name}}}' in {what} — korg substitutes exactly: {}. \
                 A placeholder korg does not know would render literally into a work-item \
                 title months from now, so it is refused here instead.",
                vocab::SCHEDULE_SUBSTITUTIONS
                    .iter()
                    .map(|s| format!("{{{s}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .into());
        }
        i = close + 1;
        let _ = bytes;
    }
    Ok(())
}

/// Create a schedule. Nothing is materialised here — a schedule that is already
/// due on the day it is filed still waits for an explicit `materialize_schedule`.
pub async fn create_schedule(pool: &PgPool, new: NewSchedule) -> Result<ScheduleRow> {
    require_non_empty(&new.title, "title")?;
    validate_status(&new.cadence, &vocab::SCHEDULE_CADENCES, "cadence")?;
    validate_status(&new.anchor_mode, &vocab::SCHEDULE_ANCHORS, "anchor_mode")?;
    validate_status(&new.wi_type, &vocab::WI_TYPES, "wi_type")?;
    validate_status(&new.wi_tshirt, &vocab::WI_TSHIRTS, "wi_tshirt")?;
    check_substitutions(&new.title, "title")?;
    if let Some(body) = &new.template {
        check_substitutions(body, "template")?;
    }
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('schedule', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    // `coalesce($6, now())` rather than a Rust-side default: the anchor and the
    // predicate that reads it must come from the same clock.
    sqlx::query(
        "INSERT INTO schedule \
         (node_id, title, template, notes, cadence, anchor_mode, anchor_at, wi_type, wi_tshirt) \
         VALUES ($1, $2, $3, $4, $5, $6, coalesce($7, now()), $8, $9)",
    )
    .bind(node_id)
    .bind(&new.title)
    .bind(&new.template)
    .bind(&new.notes)
    .bind(&new.cadence)
    .bind(&new.anchor_mode)
    .bind(new.anchor_at)
    .bind(&new.wi_type)
    .bind(&new.wi_tshirt)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    get_schedule(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no schedule with node_id {node_id}")).into())
}

/// One schedule, or `None`.
pub async fn get_schedule(pool: &PgPool, node_id: i64) -> Result<Option<ScheduleRow>> {
    schedule_row(pool, node_id).await
}

/// [`get_schedule`] over any executor, so `materialize_schedule` can re-read the
/// row it has just locked **inside** its transaction (#1390). Under READ
/// COMMITTED each statement takes a fresh snapshot, so this sees whatever a
/// racing materialisation committed while we waited on the lock — which is the
/// entire point of asking again.
async fn schedule_row<'e, E>(executor: E, node_id: i64) -> Result<Option<ScheduleRow>>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    Ok(
        sqlx::query_as::<_, ScheduleRow>(&format!("{} WHERE s.node_id = $1", schedule_select()))
            .bind(node_id)
            .fetch_optional(executor)
            .await?,
    )
}

/// A schedule with its comments and edges — the focused read (LB-3), where a
/// `has_handoff` ref surfaces.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ScheduleDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub schedule: ScheduleRow,
    /// Every work item this schedule has materialised, newest first — the
    /// answer to "when was this drill *actually* run?", which `last_wi_number`
    /// alone cannot give.
    pub materialized: Vec<ScheduleMaterialization>,
    pub comments: Vec<Comment>,
    pub comments_truncated: bool,
    pub related: Vec<RelatedRef>,
    pub related_truncated: bool,
}

/// One past materialisation of a schedule.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ScheduleMaterialization {
    pub wi_number: i64,
    pub title: String,
    pub wi_status: String,
    /// When the edge was written — i.e. when this schedule fired.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub materialized: OffsetDateTime,
}

/// `get_schedule` — the focused read.
pub async fn get_schedule_detail(pool: &PgPool, node_id: i64) -> Result<Option<ScheduleDetail>> {
    let Some(schedule) = get_schedule(pool, node_id).await? else {
        return Ok(None);
    };
    let materialized = sqlx::query_as::<_, ScheduleMaterialization>(
        "SELECT w.wi_number, w.title, w.wi_status, r.created AS materialized \
         FROM relationship r JOIN workitem w ON w.node_id = r.right_id \
         WHERE r.left_id = $1 AND r.relationship = 'materializes' \
         ORDER BY r.created DESC, w.wi_number DESC",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created LIMIT $2",
    )
    .bind(node_id)
    .bind(WORKITEM_COMMENT_CAP)
    .fetch_all(pool)
    .await?;
    let comments_truncated = schedule.comment_count > WORKITEM_COMMENT_CAP;
    let (related, related_truncated) = related_context(pool, node_id, Some("materializes")).await?;
    Ok(Some(ScheduleDetail {
        schedule,
        materialized,
        comments,
        comments_truncated,
        related,
        related_truncated,
    }))
}

/// What `list_schedules`' defaults hid. Same cascade rule as [`ProgramOmitted`].
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ScheduleOmitted {
    pub done: i64,
    pub archived: i64,
    /// How many rows `due_only` **hid** for not being due yet — never a count
    /// of what came back (the returned rows are, by definition, due). Zero when
    /// `due_only` was not asked for, so a consumer can always tell "nothing is
    /// due" from "I filtered it out".
    pub not_due: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ScheduleList {
    pub items: Vec<ScheduleRow>,
    pub omitted: ScheduleOmitted,
}

/// List schedules, live-by-default, **soonest-due first**.
///
/// The ordering is the feature: the first row is the next thing that wants
/// doing, and everything already due sorts above everything that does not.
pub async fn list_schedules(
    pool: &PgPool,
    status: Option<&str>,
    project: Option<&str>,
    due_only: bool,
    archived: ArchivedFilter,
) -> Result<ScheduleList> {
    let shown = schedule_status_predicate(status)?;
    let items = sqlx::query_as::<_, ScheduleRow>(&format!(
        "{} WHERE ($1::text[] IS NULL OR s.status = ANY($1)) \
            AND ($2::bool IS NULL OR n.archived = $2) \
            AND ($3::text IS NULL OR pj.name = $3) \
            AND (NOT $4::bool OR ({})) \
          ORDER BY due_at ASC, s.node_id ASC",
        schedule_select(),
        schedule_due_sql()
    ))
    .bind(shown.as_deref())
    .bind(archived)
    .bind(project)
    .bind(due_only)
    .fetch_all(pool)
    .await?;

    let (archived_hidden, done, not_due) = sqlx::query_as::<_, (i64, i64, i64)>(&format!(
        "SELECT count(*) FILTER (WHERE n.archived AND $1::bool IS NOT NULL AND NOT $1), \
                count(*) FILTER (WHERE ($1::bool IS NULL OR n.archived = $1) \
                                   AND s.status = 'done'), \
                count(*) FILTER (WHERE ($1::bool IS NULL OR n.archived = $1) \
                                   AND ($2::text[] IS NULL OR s.status = ANY($2)) \
                                   AND ($3::text IS NULL OR pj.name = $3) \
                                   AND $4::bool AND NOT ({})) \
         FROM schedule s JOIN node n ON n.id = s.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id",
        schedule_due_sql()
    ))
    .bind(archived)
    .bind(shown.as_deref())
    .bind(project)
    .bind(due_only)
    .fetch_one(pool)
    .await?;
    let done_hidden = match shown.as_deref() {
        Some(list) if list.iter().any(|s| s == "done") => 0,
        None => 0,
        _ => done,
    };
    Ok(ScheduleList {
        items,
        omitted: ScheduleOmitted {
            done: done_hidden,
            archived: archived_hidden,
            not_due,
        },
    })
}

/// Absent → the live set (`active` + `paused`); `"all"` → no filter; anything
/// else validated and returned alone. Same contract as
/// [`program_status_predicate`].
fn schedule_status_predicate(status: Option<&str>) -> Result<Option<Vec<String>>> {
    match status {
        None => Ok(Some(
            vocab::SCHEDULE_LIVE_STATUSES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &vocab::SCHEDULE_STATUSES, "schedule status")?;
            Ok(Some(vec![s.to_string()]))
        }
    }
}

/// `update_schedule` / `PATCH /api/schedules/:node_id`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct SchedulePatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty_opt")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub template: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::schedule_cadence")]
    pub cadence: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::schedule_anchor")]
    pub anchor_mode: Option<String>,
    /// Move the anchor by hand — e.g. after doing the maintenance outside korg,
    /// or to defer a drill a fortnight without touching its cadence.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schemars(schema_with = "schema::timestamp_opt")]
    pub anchor_at: Option<OffsetDateTime>,
    /// `paused` is how a schedule stops surfacing without losing its anchor or
    /// its history.
    #[serde(default)]
    #[schemars(schema_with = "schema::schedule_status")]
    pub status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_type")]
    pub wi_type: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_tshirt")]
    pub wi_tshirt: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

pub async fn update_schedule(
    pool: &PgPool,
    node_id: i64,
    patch: SchedulePatch,
) -> Result<ScheduleRow> {
    if let Some(v) = &patch.title {
        require_non_empty(v, "title")?;
        check_substitutions(v, "title")?;
    }
    if let Some(Some(v)) = &patch.template {
        check_substitutions(v, "template")?;
    }
    if let Some(v) = &patch.cadence {
        validate_status(v, &vocab::SCHEDULE_CADENCES, "cadence")?;
    }
    if let Some(v) = &patch.anchor_mode {
        validate_status(v, &vocab::SCHEDULE_ANCHORS, "anchor_mode")?;
    }
    if let Some(v) = &patch.status {
        validate_status(v, &vocab::SCHEDULE_STATUSES, "schedule status")?;
    }
    if let Some(v) = &patch.wi_type {
        validate_status(v, &vocab::WI_TYPES, "wi_type")?;
    }
    if let Some(v) = &patch.wi_tshirt {
        validate_status(v, &vocab::WI_TSHIRTS, "wi_tshirt")?;
    }

    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "schedule", "schedule").await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE schedule SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.template {
        sqlx::query("UPDATE schedule SET template = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v.as_deref())
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.notes {
        sqlx::query("UPDATE schedule SET notes = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v.as_deref())
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.cadence {
        sqlx::query("UPDATE schedule SET cadence = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.anchor_mode {
        sqlx::query("UPDATE schedule SET anchor_mode = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.anchor_at {
        sqlx::query("UPDATE schedule SET anchor_at = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.status {
        sqlx::query("UPDATE schedule SET status = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_type {
        sqlx::query("UPDATE schedule SET wi_type = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_tshirt {
        sqlx::query("UPDATE schedule SET wi_tshirt = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    settle_awaiting(&mut *tx, node_id).await?;
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_schedule(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no schedule with node_id {node_id}")).into())
}

/// The three refusals `materialize_schedule` owes a caller, in one place so the
/// pre-transaction check and the in-transaction re-check (#1390) cannot answer
/// differently — a second copy of these conditions is how the promise would
/// rot again.
fn materialize_gate(schedule: &ScheduleRow, node_id: i64, force: bool) -> Result<()> {
    if schedule.outstanding {
        return Err(RepoError::InvalidInput(format!(
            "schedule {node_id} already materialized work item #{} and it is still \
             {} — that item IS this schedule's current surface. Close it before \
             materializing again; `force` deliberately does not lift this.",
            schedule.last_wi_number.unwrap_or_default(),
            if schedule.last_wi_number.is_some() {
                "open"
            } else {
                "outstanding"
            }
        ))
        .into());
    }
    if schedule.status != "active" {
        return Err(RepoError::InvalidInput(format!(
            "schedule {node_id} is '{}', not 'active' — a paused schedule is one \
             deliberately not surfacing, and a done one has already fired its \
             single occurrence. Set status back to 'active' to resume it.",
            schedule.status
        ))
        .into());
    }
    if !schedule.due && !force {
        return Err(RepoError::InvalidInput(format!(
            "schedule {node_id} is not due until {} (cadence '{}', anchored {}). \
             Pass force:true to materialize it early — e.g. a restore drill \
             brought forward because the schema shape changed.",
            schedule.due_at, schedule.cadence, schedule.anchor_at
        ))
        .into());
    }
    Ok(())
}

/// What `materialize_schedule` produced.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Materialized {
    /// The work item that now exists, as a read would return it.
    pub work_item: WorkItemRow,
    /// The schedule after firing — new `anchor_at` for a `created` anchor, and
    /// `status: "done"` if the cadence was `once`.
    pub schedule: ScheduleRow,
}

/// Turn a due schedule into a work item (D-2) — the explicit write that stands
/// in for the daily tick #581 sketched.
///
/// `force` materialises a schedule that is not yet due. It exists for the
/// "we're doing the drill early because the schema just changed" case that
/// `docs/operations.md` names as a second trigger, and it is a parameter rather
/// than a separate tool so the refusal can *say* what would lift it. It never
/// bypasses the outstanding-item check: producing a second copy of a drill whose
/// first copy is still open is not something a caller can want, and that is the
/// duplicate-materialisation failure a scheduler would have had.
///
/// **The gates run twice** (#1390). Once before the transaction, so the common
/// refusal is cheap and takes no lock; then again on the row this transaction
/// has locked, because until #1390 the pre-check was the *only* check. Two
/// callers — realistically one agent retrying a call that timed out — could both
/// pass the pre-checks, queue on the `FOR UPDATE`, and both insert: two open
/// copies of the drill, `last_wi_id` pointing at the second. That is precisely
/// the duplicate the no-scheduler design (051 D-2) exists to prevent, and the
/// one thing `force` never lifts, so the promise is now kept where it can be.
pub async fn materialize_schedule(
    pool: &PgPool,
    node_id: i64,
    force: bool,
) -> Result<Materialized> {
    let Some(schedule) = get_schedule(pool, node_id).await? else {
        return Err(RepoError::NotFound(format!("no schedule with node_id {node_id}")).into());
    };
    materialize_gate(&schedule, node_id, force)?;

    let mut tx = pool.begin().await?;
    // Rendered by Postgres, from the same clock the predicate above read. The
    // template columns are substituted identically; `template` may be NULL, in
    // which case the item's content falls back to naming its origin.
    let (title, content): (String, Option<String>) = sqlx::query_as(
        "SELECT replace(replace(replace(replace(replace(s.title, \
             '{YEAR}',    to_char(now(), 'YYYY')), \
             '{MONTH}',   trim(to_char(now(), 'Month'))), \
             '{DAY}',     to_char(now(), 'DD')), \
             '{DATE}',    to_char(now(), 'YYYY-MM-DD')), \
             '{QUARTER}', 'Q' || to_char(now(), 'Q')), \
                replace(replace(replace(replace(replace(s.template, \
             '{YEAR}',    to_char(now(), 'YYYY')), \
             '{MONTH}',   trim(to_char(now(), 'Month'))), \
             '{DAY}',     to_char(now(), 'DD')), \
             '{DATE}',    to_char(now(), 'YYYY-MM-DD')), \
             '{QUARTER}', 'Q' || to_char(now(), 'Q')) \
         FROM schedule s WHERE s.node_id = $1 FOR UPDATE",
    )
    .bind(node_id)
    .fetch_one(&mut *tx)
    .await?;

    // The row is ours now. Ask the gates again on what it actually says — a
    // racing materialisation that committed while we queued on that lock has
    // already moved `last_wi_id` and `anchor_at`, and the pre-transaction
    // answer is stale (#1390).
    let Some(locked) = schedule_row(&mut *tx, node_id).await? else {
        return Err(RepoError::NotFound(format!("no schedule with node_id {node_id}")).into());
    };
    materialize_gate(&locked, node_id, force)?;

    let project_id: Option<i64> = sqlx::query_scalar("SELECT project_id FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await?;

    let wi_node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, tags) VALUES ('workitem', $1, $2) RETURNING id",
    )
    .bind(project_id)
    .bind(vec![format!("schedule-{node_id}")])
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    let body = content.unwrap_or_else(|| {
        format!("Materialized from schedule {node_id}. See the schedule for the procedure.")
    });
    sqlx::query(
        "INSERT INTO workitem \
         (node_id, wi_number, wi_type, wi_status, wi_tshirt, title, content) \
         VALUES ($1, $1, $2, 'open', $3, $4, $5)",
    )
    .bind(wi_node_id)
    .bind(&schedule.wi_type)
    .bind(&schedule.wi_tshirt)
    .bind(&title)
    .bind(&body)
    .execute(&mut *tx)
    .await?;

    // Provenance (D-17): the edge is the history, `last_wi_id` is only the
    // pointer to the newest.
    sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
         VALUES ($1, $2, 'materializes', now(), 'materialize_schedule')",
    )
    .bind(node_id)
    .bind(wi_node_id)
    .execute(&mut *tx)
    .await?;

    // Three effects in one statement, each conditional on the schedule's own
    // configuration:
    //   * the pointer always moves;
    //   * a `created` anchor advances now — a `completed` one waits for the item
    //     to be finished, which `update_work_item` handles (D-4);
    //   * a `once` schedule is finished the moment it fires.
    sqlx::query(
        "UPDATE schedule \
            SET last_wi_id = $2, \
                anchor_at  = CASE WHEN anchor_mode = 'created' THEN now() ELSE anchor_at END, \
                status     = CASE WHEN cadence = 'once' THEN 'done' ELSE status END \
          WHERE node_id = $1",
    )
    .bind(node_id)
    .bind(wi_node_id)
    .execute(&mut *tx)
    .await?;
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;

    let work_item = get_work_item(pool, wi_node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_node_id}")))?;
    let schedule = get_schedule(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no schedule with node_id {node_id}")))?;
    Ok(Materialized {
        work_item,
        schedule,
    })
}

/// Advance the anchor of any `completed`-mode schedule whose newest
/// materialisation just reached a completed status (D-4).
///
/// This is the write rule 0025 deliberately did not make a trigger: 0023 settled
/// that lifecycle rules live in korg-core, the one path both transports share,
/// never in a trigger that can drift from it.
///
/// [`WI_FINISHED_STATUSES`] only. `resolved` is excluded on purpose and for the
/// same reason [`SCHEDULE_DUE_SQL`] treats it as outstanding — a restore drill
/// that is "implemented, may still need a user test" has not been performed, and
/// advancing the anchor there would restart the quarter from a drill nobody ran.
///
/// Sprint 053 replaced the literal pair here with the named vocabulary the
/// board's blocker derivation also reads (#978), so "the work is finished" has
/// one definition rather than two that happen to agree today.
pub(super) async fn advance_completed_anchor(
    tx: &mut Transaction<'_, Postgres>,
    wi_node_id: i64,
    new_status: &str,
) -> Result<()> {
    if !WI_FINISHED_STATUSES.contains(&new_status) {
        return Ok(());
    }
    sqlx::query(
        "UPDATE schedule SET anchor_at = now() \
          WHERE last_wi_id = $1 AND anchor_mode = 'completed' AND status = 'active'",
    )
    .bind(wi_node_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
