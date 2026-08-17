//! Work items — create, the read views, the lean list (WI #861), and update.

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::vocab::{self, WI_LIVE_STATUSES, WI_STATUSES};

use super::attachments::{list_attachments, AttachmentRow};
use super::awaiting::settle_awaiting;
use super::comments::Comment;
use super::common::{record_transition, touch_node, validate_status};
use super::page::{archived_default, ArchivedFilter, Page, PageQuery};
use super::relationships::{related_context, RelatedRef};
use super::schedules::advance_completed_anchor;
use super::selectors::{resolve_area, resolve_area_patch, resolve_project, resolve_project_patch};

// --- work items -----------------------------------------------------------

/// `create_work_item` / `POST /api/work-items`. Both transports deserialize
/// this exact type, and the MCP input schema is derived from it (WI #539/#540).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewWorkItem {
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both. Resolved by exact name, and an unknown name returns `not_found`
    /// rather than mis-filing, so pass a name you are confident in directly.
    /// Call `list_projects` only when the name is genuinely unknown or
    /// ambiguous — the roster in this server's instructions already names
    /// every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub area_id: Option<i64>,
    /// Area name — the alternative to `area_id`, resolved within the item's project.
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default = "ops::default_task")]
    #[schemars(schema_with = "schema::wi_type")]
    pub wi_type: String,
    #[serde(default = "ops::default_open")]
    #[schemars(schema_with = "schema::wi_status")]
    pub wi_status: String,
    #[serde(default = "ops::default_unknown")]
    #[schemars(schema_with = "schema::wi_tshirt")]
    pub wi_tshirt: String,
    #[serde(default)]
    pub sprint: Option<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
}

/// Create a work item and return the row a read would return (WI #525) — a
/// superset of the old `{node_id, wi_number}` acknowledgement.
pub async fn create_work_item(pool: &PgPool, new: NewWorkItem) -> Result<WorkItemRow> {
    validate_status(&new.wi_status, &WI_STATUSES, "wi_status")?;
    validate_status(&new.wi_type, &vocab::WI_TYPES, "wi_type")?;
    validate_status(&new.wi_tshirt, &vocab::WI_TSHIRTS, "wi_tshirt")?;
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let area_id = resolve_area(pool, project_id, new.area_id, new.area.as_deref()).await?;
    // An area belongs to exactly one project; `update_work_item` has always
    // enforced that, `create_work_item` did not (WI #526). Resolving by name
    // satisfies this by construction; an explicit `area_id` still has to be
    // checked.
    if let Some(area_id) = area_id {
        let area_pid: Option<i64> = sqlx::query_scalar("SELECT project_id FROM area WHERE id = $1")
            .bind(area_id)
            .fetch_optional(pool)
            .await?;
        match area_pid {
            None => {
                return Err(RepoError::InvalidInput(format!(
                    "no area with id {area_id} — call list_areas for the available areas"
                ))
                .into());
            }
            Some(pid) if Some(pid) != project_id => {
                return Err(RepoError::InvalidInput(format!(
                    "area {area_id} does not belong to the work item's project"
                ))
                .into());
            }
            Some(_) => {}
        }
    }
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('workitem', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    // Since 0009_identity, wi_number IS the node id — one number everywhere.
    let wi_number: i64 = sqlx::query(
        "INSERT INTO workitem \
         (node_id, wi_number, area_id, wi_type, wi_status, wi_tshirt, sprint, title, content, details) \
         VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING wi_number",
    )
    .bind(node_id)
    .bind(area_id)
    .bind(&new.wi_type)
    .bind(&new.wi_status)
    .bind(&new.wi_tshirt)
    .bind(&new.sprint)
    .bind(&new.title)
    .bind(&new.content)
    .bind(&new.details)
    .fetch_one(&mut *tx)
    .await?
    .get("wi_number");

    tx.commit().await?;
    get_work_item(pool, wi_number)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_number}")).into())
}

// --- read views -----------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemRow {
    pub wi_number: i64,
    pub node_id: i64,
    pub project: Option<String>,
    pub area: Option<String>,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub sprint: Option<String>,
    pub title: String,
    pub content: String,
    pub details: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub parent: Option<i64>,
    pub archived: bool,
    /// Number of comments on this work item (WI #392) — the hint that tells an
    /// agent "this row has discussion; fetch it".
    pub comment_count: i64,
    /// True when a `has_handoff` edge leaves this item (WI #813) — durable
    /// context another session left, waiting to be read.
    pub has_handoff: bool,
    /// The live proposal covering this item, or `None` when nothing does
    /// (WI #824). See `membership_select!` for why "live" and why one id.
    pub proposal_node_id: Option<i64>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// The two membership markers (sprint 042), spliced into both row tiers — the
/// full REST row and the lean MCP summary. Written once because the whole
/// point of bundling #813/#824/#823 was to pay the contract cost *once*; two
/// copies of this SQL is how the tiers drift on what "spoken for" means.
///
/// Both read the `relationship` table, and the two labels point **opposite
/// ways** — the trap #813 flagged and both drafts still got wrong:
///
/// - `covers` is proposal -> work item, so the item is the edge's **right**
///   end. Drafts wrote `left_node_id`/`label`; the columns are
///   `left_id`/`right_id`/`relationship`.
/// - `has_handoff` is node -> handoff, so the item is the **left** end.
///
/// **Why only live proposals.** The question the rows are answering is "is
/// this already spoken for?" — the one that cost 17 `get_proposal` calls in
/// the 2026-07-31 backlog review. A `declined` proposal speaks for nothing,
/// and a `done` one has already had its say; either would paint an open item
/// in the "claimed" colour and answer that question wrong. `min` picks a
/// single id when more than one live proposal covers an item — rare, and a
/// stable pick beats an arbitrary one.
///
/// **Why joins and not the correlated subqueries the WIs sketched.** Both WIs
/// proposed `EXISTS (SELECT 1 FROM relationship …)` per row, and #813 asked
/// for a measurement rather than an assumption. The measurement
/// (`tests/sprint042_measure.rs`, 1000 items / 100 proposals / 60 handoffs)
/// said the assumption was wrong: correlated subqueries cost **+324%** on the
/// REST read and **+448%** on the lean one, because a 500-row page does 500
/// index lookups plus 500 joins to `sprint_proposal`.
///
/// These pre-aggregate instead. Both edge sets are *small* — coverage is
/// hundreds of rows, handoffs dozens — so Postgres scans each once behind
/// `relationship_label_idx` and hash-joins the result, turning per-row work
/// into per-*query* work. Same answers, and the cost drops to noise. Grouping
/// in the subquery (rather than `DISTINCT` on the join) keeps the join
/// one-to-one, so no row can be duplicated by a second covering edge.
macro_rules! membership_columns {
    () => {
        "(ho.left_id IS NOT NULL) AS has_handoff, cov.proposal_node_id"
    };
}

/// The `FROM` half of [`membership_columns!`] — kept adjacent because the two
/// are meaningless apart, and a select list referencing a join that is not
/// there fails at runtime rather than compile time.
macro_rules! membership_joins {
    () => {
        "LEFT JOIN (SELECT r.right_id AS node_id, min(r.left_id) AS proposal_node_id \
                      FROM relationship r \
                      JOIN sprint_proposal sp ON sp.node_id = r.left_id \
                     WHERE r.relationship = 'covers' \
                       AND sp.status::text IN ('proposed', 'active') \
                     GROUP BY r.right_id) cov ON cov.node_id = w.node_id \
         LEFT JOIN (SELECT DISTINCT r.left_id FROM relationship r \
                     WHERE r.relationship = 'has_handoff') ho ON ho.left_id = w.node_id"
    };
}

pub(super) const WORKITEM_SELECT: &str = concat!(
    "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, \
        (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, ",
    membership_columns!(),
    ", n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id ",
    membership_joins!()
);

/// Filters for a work-item listing. `project` is a name, matching the other
/// project-keyed surfaces.
#[derive(Debug, Clone)]
pub struct WorkItemQuery {
    pub project: Option<String>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for WorkItemQuery {
    fn default() -> Self {
        Self {
            project: None,
            archived: archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Full work-item rows for one project (or all), enveloped and bounded
/// (WI #534). **REST only since #861** — `GET /api/work-items`, which the Work
/// Items page walks to completion (035 D-1) and then filters, searches and
/// derives tag chips from in memory, all of which need `content` and `details`.
/// The MCP `list_work_items` is [`list_work_items_lean`]; the full tier there is
/// `get_work_item`, one row at a time.
pub async fn list_work_items(pool: &PgPool, query: WorkItemQuery) -> Result<Page<WorkItemRow>> {
    let (limit, offset) = query.page.resolve();
    let sql = format!(
        "{WORKITEM_SELECT} \
         WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::bool IS NULL OR n.archived = $2) \
         ORDER BY w.wi_number \
         LIMIT $3 OFFSET $4"
    );
    let items = sqlx::query_as::<_, WorkItemRow>(&sql)
        .bind(query.project.as_deref())
        .bind(query.archived)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workitem w JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::bool IS NULL OR n.archived = $2)",
    )
    .bind(query.project.as_deref())
    .bind(query.archived)
    .fetch_one(pool)
    .await?;
    Ok(Page::new(items, total, limit, offset))
}

pub async fn get_work_item(pool: &PgPool, wi_number: i64) -> Result<Option<WorkItemRow>> {
    let sql = format!("{WORKITEM_SELECT} WHERE w.wi_number = $1");
    Ok(sqlx::query_as::<_, WorkItemRow>(&sql)
        .bind(wi_number)
        .fetch_optional(pool)
        .await?)
}

/// Max comments inlined into a single-item detail fetch (WI #392). A
/// pathological thread past this is truncated with `comments_truncated`, and
/// the caller fetches the whole thread from `list_comments` — which is
/// unpaginated, so it is a re-read rather than a tail (WI #890).
pub const WORKITEM_COMMENT_CAP: i64 = 10;

/// A work item plus its comments, capped (WI #392). The single-item detail
/// fetch commits to the full state of one item — and comments frequently hold
/// the payload (resolution rationale, decisions), so agents that only call
/// `get_work_item` should see them without a second round-trip. `item.comment_count`
/// is the true total; `comments` holds at most `WORKITEM_COMMENT_CAP` of them.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub item: WorkItemRow,
    pub comments: Vec<Comment>,
    /// True when there are more comments than were inlined (call `list_comments`).
    pub comments_truncated: bool,
    /// The item's edges, inlined (LB-3): covers-IN reveals which proposal covers
    /// it, plus depends_on / related-to / finding. Capped and label-ordered.
    pub related: Vec<RelatedRef>,
    /// True when there are more edges than were inlined (call `neighbors`).
    pub related_truncated: bool,
    /// The images attached to this item (sprint 056, #1119), with their
    /// dimensions and variant URLs — enough for an agent to decide whether to
    /// fetch one and, if so, which size.
    ///
    /// Carried here rather than left to `related` for the same reason a
    /// proposal carries `covered`: a bare edge ref says an attachment exists
    /// and nothing about what it *is*, and "there is a screenshot on this bug"
    /// is a fact the read should not need a follow-up call to make useful.
    /// `has_attachment` is excluded from `related` accordingly — one fact, one
    /// place in the payload.
    ///
    /// Images pasted into a **comment** appear here too: a korg comment is not
    /// a node, so it cannot own an edge, and the node it hangs off owns the
    /// image instead (0027's header). The comment body carries the placement
    /// token.
    pub attachments: Vec<AttachmentRow>,
}

/// `get_work_item` plus inlined, capped comments (WI #392). `None` if the
/// work item doesn't exist.
pub async fn get_work_item_detail(pool: &PgPool, wi_number: i64) -> Result<Option<WorkItemDetail>> {
    let Some(item) = get_work_item(pool, wi_number).await? else {
        return Ok(None);
    };
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created LIMIT $2",
    )
    .bind(item.node_id)
    .bind(WORKITEM_COMMENT_CAP)
    .fetch_all(pool)
    .await?;
    let comments_truncated = item.comment_count > WORKITEM_COMMENT_CAP;
    // `has_attachment` is the one label inlined elsewhere on a work item —
    // `attachments` below carries it in full, so leaving it in `related` too
    // would report the same edge twice and spend a slot of the cap doing it.
    let (related, related_truncated) =
        related_context(pool, item.node_id, Some("has_attachment")).await?;
    let attachments = list_attachments(pool, item.node_id).await?;
    Ok(Some(WorkItemDetail {
        item,
        comments,
        comments_truncated,
        related,
        related_truncated,
        attachments,
    }))
}

// --- the lean work-item list (WI #861) --------------------------------------
//
// `list_work_items` above is the REST/web read and stays as it is: the Work
// Items page holds its whole collection and filters, searches and derives tag
// chips in memory (035 D-1), which needs `content` and `details`. It is a
// browser, not a token budget — the same split `list_proposals` has had since
// #852 and `list_projects` since #828.
//
// The MCP read is the one that measured ~890k chars of content+details across
// the corpus, with 78% of the rows `closed` while `update_work_item`'s own
// schema claimed closed items were "hidden by default". Everything below is
// that read: survey's projection, terminal statuses excluded by default, and an
// `omitted` envelope so both narrowings are visible.

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemSummary {
    pub wi_number: i64,
    pub node_id: i64,
    pub project: Option<String>,
    pub title: String,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    /// Comment count (WI #392) — signals which rows carry discussion worth fetching.
    pub comment_count: i64,
    /// True when this item has a non-empty `details` section (WI #813). The
    /// projection carries no bodies by design, so the Review page had to show
    /// no 📝 rather than a false negative; a boolean is the honest fix and
    /// costs no join.
    pub has_details: bool,
    /// True when a `has_handoff` edge leaves this item (WI #813) — "which of
    /// these already has durable context waiting", which is precisely what the
    /// survey could not answer without N follow-up `neighbors` calls.
    pub has_handoff: bool,
    /// The live proposal covering this item (WI #824) — "which of these is
    /// already spoken for", the other question that cost N follow-up calls.
    pub proposal_node_id: Option<i64>,
    /// When the item last changed (WI #1411) — the staleness signal a sweep
    /// could not get from a lean row.
    ///
    /// kfo's Scouting Report proxied age with `wi_number` (creation order) and
    /// said so in every run; last-touched needed one `get_work_item` per row.
    /// `updated` only, deliberately: creation age already has a proxy, and the
    /// missing half was "has anyone come back to this".
    ///
    /// The signal is only worth reading while it stays *work* activity —
    /// korg+ GP-3: an agent-assigned priority must never ride a path that bumps
    /// it, or a ranked stale item reads as a fresh one.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// What the lean list's defaults hid (WI #851, extended by #861), computed as a
/// **cascade** so no row is counted twice: `archived` is what the archived
/// filter excluded, and `closed` is counted only over the rows that *passed* it.
/// An archived closed item therefore lands in `archived` and nowhere else.
///
/// A field is 0 when the caller asked to see that class — `wi_status: "all"`
/// (or `"closed"`) zeroes `closed`, `archived: null`/`true` zeroes `archived`.
/// Naming each field for what it counts keeps it honest under every setting,
/// rather than meaning "unarchived" half the time.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemOmitted {
    pub closed: i64,
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemListLean {
    pub items: Vec<WorkItemSummary>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    /// The rows the defaults hid. `total` is the count *after* filtering, so
    /// without this a sweep used to decide "is this project drained?" cannot
    /// tell a narrowed answer from a complete one — the same silent-truncation
    /// failure `list_projects`' `omitted` exists to treat (WI #828).
    pub omitted: WorkItemOmitted,
}

/// Resolve the `wi_status` argument into the statuses a lean list returns.
///
/// Absent → the non-terminal ones (`open` + `resolved` + `done`), which is what
/// makes `update_work_item`'s "closed … hidden by default" true for the first
/// time. `Some("all")` → no filter. Anything else is validated against
/// `WI_STATUSES` and returned alone. Identical in shape to
/// `proposal_status_predicate` (#852) and `project_status_predicate` (#828).
fn wi_status_predicate(status: Option<&str>) -> Result<Option<Vec<String>>> {
    match status {
        None => Ok(Some(
            WI_LIVE_STATUSES.iter().map(|s| s.to_string()).collect(),
        )),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &WI_STATUSES, "work item status")?;
            Ok(Some(vec![s.to_string()]))
        }
    }
}

/// A slim, paginated projection of work items (no content/details), excluding
/// terminal and archived rows by default and saying what that hid.
///
/// This is `list_work_items` over MCP and `GET /api/work-items/survey` over
/// REST. `total` is the full filtered count (before LIMIT/OFFSET), so callers
/// can page.
pub async fn list_work_items_lean(
    pool: &PgPool,
    project: Option<&str>,
    wi_status: Option<&str>,
    archived: ArchivedFilter,
    limit: i64,
    offset: i64,
) -> Result<WorkItemListLean> {
    let shown = wi_status_predicate(wi_status)?;
    #[derive(sqlx::FromRow)]
    struct Row {
        wi_number: i64,
        node_id: i64,
        project: Option<String>,
        title: String,
        wi_type: String,
        wi_status: String,
        wi_tshirt: String,
        comment_count: i64,
        has_details: bool,
        has_handoff: bool,
        proposal_node_id: Option<i64>,
        updated: OffsetDateTime,
    }
    let rows = sqlx::query_as::<_, Row>(concat!(
        "SELECT w.wi_number, w.node_id, pj.name AS project, w.title, \
                w.wi_type, w.wi_status, w.wi_tshirt, n.updated, \
                (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
                (w.details IS NOT NULL AND w.details <> '') AS has_details, ",
        membership_columns!(),
        " FROM workitem w \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id ",
        membership_joins!(),
        " WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::text[] IS NULL OR w.wi_status = ANY($2)) \
           AND ($3::bool IS NULL OR n.archived = $3) \
         ORDER BY w.wi_number \
         LIMIT $4 OFFSET $5",
    ))
    .bind(project)
    .bind(shown.as_deref())
    .bind(archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| WorkItemSummary {
            wi_number: r.wi_number,
            node_id: r.node_id,
            project: r.project,
            title: r.title,
            wi_type: r.wi_type,
            wi_status: r.wi_status,
            wi_tshirt: r.wi_tshirt,
            comment_count: r.comment_count,
            has_details: r.has_details,
            has_handoff: r.has_handoff,
            proposal_node_id: r.proposal_node_id,
            updated: r.updated,
        })
        .collect();

    let (total, omitted) = wi_counts(pool, project, archived, shown.as_deref()).await?;
    Ok(WorkItemListLean {
        items,
        total,
        limit,
        offset,
        omitted,
    })
}

/// The lean list's corpus size and what its defaults hid, in one round trip.
///
/// `total` is deliberately *not* a `count(*) OVER()` on the page query. That
/// window count rides on the returned rows, so a page whose `offset` overshoots
/// the last row returns no rows, no count, and `total: 0` — the envelope
/// claiming an empty corpus exactly when a pager runs off the end (WI #883).
/// Counting in this statement, whose result set is one row whatever the paging
/// did, is what makes `total` mean the same thing on every page.
///
/// The `omitted` counts are taken over the `project`-filtered corpus —
/// narrowing to a project is the caller *choosing* a scope, not a default
/// hiding rows from them, so it is not something `omitted` should report. Same
/// reasoning as `proposal_omitted`. `total` narrows further than that: it is
/// the count after *every* filter, which is what a pager needs.
async fn wi_counts(
    pool: &PgPool,
    project: Option<&str>,
    archived: ArchivedFilter,
    shown: Option<&[String]>,
) -> Result<(i64, WorkItemOmitted)> {
    let (total, archived_hidden, closed) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND ($3::text[] IS NULL OR w.wi_status = ANY($3))), \
           count(*) FILTER (WHERE n.archived AND $2::bool IS NOT NULL AND NOT $2), \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND w.wi_status = 'closed') \
         FROM workitem w \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1)",
    )
    .bind(project)
    .bind(archived)
    .bind(shown)
    .fetch_one(pool)
    .await?;
    // A status the caller asked for isn't omitted, however many rows it has.
    let closed_hidden = match shown {
        Some(list) if list.iter().any(|s| s == "closed") => 0,
        None => 0,
        _ => closed,
    };
    Ok((
        total,
        WorkItemOmitted {
            closed: closed_hidden,
            archived: archived_hidden,
        },
    ))
}

/// Resolve a work item's node id from its user-facing wi_number.
pub async fn node_id_for_wi(pool: &PgPool, wi_number: i64) -> Result<Option<i64>> {
    let id: Option<i64> = sqlx::query_scalar("SELECT node_id FROM workitem WHERE wi_number = $1")
        .bind(wi_number)
        .fetch_optional(pool)
        .await?;
    Ok(id)
}

// --- work item update (Edit + Archive) ------------------------------------

/// `update_work_item` / `PATCH /api/work-items/:wi_number`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WorkItemPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub details: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_type")]
    pub wi_type: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_status")]
    pub wi_status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_tshirt")]
    pub wi_tshirt: Option<String>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub sprint: Option<Option<String>>,
    /// Move to this project (id); null unassigns. Get ids from list_projects.
    // `Some(Some(id))` moves, `Some(None)` unassigns, `None` leaves it (WI
    // #291). A move clears an area that no longer belongs to the target project
    // unless a valid `area_id` is given in the same call.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project_id: Option<Option<i64>>,
    /// Project name — the alternative to `project_id`; null unassigns. Never pass both.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub area_id: Option<Option<i64>>,
    /// Area name — the alternative to `area_id`; null clears. Resolved in the new project.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub area: Option<Option<String>>,
    /// Parent work item's wi_number; null clears the parent.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub parent: Option<Option<i64>>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub category: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

pub async fn update_work_item(
    pool: &PgPool,
    wi_number: i64,
    patch: WorkItemPatch,
) -> Result<WorkItemRow> {
    let node_id = node_id_for_wi(pool, wi_number)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_number}")))?;
    if let Some(v) = &patch.wi_status {
        validate_status(v, &WI_STATUSES, "wi_status")?;
    }
    if let Some(v) = &patch.wi_type {
        validate_status(v, &vocab::WI_TYPES, "wi_type")?;
    }
    if let Some(v) = &patch.wi_tshirt {
        validate_status(v, &vocab::WI_TSHIRTS, "wi_tshirt")?;
    }
    // Resolve parent wi_number -> node id before the transaction. An
    // unresolvable number used to fall through to `Some(None)` — silently
    // *clearing* the parent (F-06).
    let parent_node: Option<Option<i64>> = match &patch.parent {
        Some(Some(num)) => Some(Some(node_id_for_wi(pool, *num).await?.ok_or_else(
            || RepoError::InvalidInput(format!("no work item #{num} to use as parent")),
        )?)),
        Some(None) => Some(None),
        None => None,
    };
    // Selectors resolve before the transaction, like `parent` above: a name
    // that doesn't resolve must change nothing (WI #575). The area name is the
    // exception — it resolves inside the transaction, because it is only
    // meaningful relative to the project the item will have *after* this
    // update, which isn't known until then.
    let project_id = resolve_project_patch(pool, patch.project_id, patch.project).await?;
    let mut tx = pool.begin().await?;

    if let Some(v) = &patch.title {
        sqlx::query("UPDATE workitem SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.content {
        sqlx::query("UPDATE workitem SET content = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.details {
        sqlx::query("UPDATE workitem SET details = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_type {
        sqlx::query("UPDATE workitem SET wi_type = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_status {
        // Read-before-write (#977), as in `update_proposal`.
        let before: String =
            sqlx::query_scalar("SELECT wi_status FROM workitem WHERE node_id = $1")
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("UPDATE workitem SET wi_status = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        record_transition(&mut *tx, node_id, &before, v).await?;
    }
    if let Some(v) = &patch.wi_tshirt {
        sqlx::query("UPDATE workitem SET wi_tshirt = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.sprint {
        sqlx::query("UPDATE workitem SET sprint = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    // Project move + area consistency (WI #291). An area must belong to the
    // work item's project; moving projects drops an area that no longer fits
    // (unless a valid area_id is supplied in the same call).
    {
        let current_pid: Option<i64> =
            sqlx::query_scalar("SELECT project_id FROM node WHERE id = $1")
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        // Project the work item will have after this update.
        let effective_pid = match &project_id {
            Some(v) => *v,
            None => current_pid,
        };

        // An area name resolves against that effective project, then joins the
        // id path below — so `area` and `area_id` are validated identically.
        let area_id =
            resolve_area_patch(&mut *tx, effective_pid, patch.area_id, patch.area).await?;

        // Decide the area to leave in place. Some(Some(id)) = set+validate,
        // Some(None) = explicit clear, None = keep (auto-clearing on a move
        // when the current area no longer fits).
        let new_area: Option<Option<i64>> = match &area_id {
            Some(Some(aid)) => {
                let area_pid: Option<i64> =
                    sqlx::query_scalar("SELECT project_id FROM area WHERE id = $1")
                        .bind(aid)
                        .fetch_optional(&mut *tx)
                        .await?;
                if effective_pid.is_some() && area_pid == effective_pid {
                    Some(Some(*aid))
                } else {
                    return Err(RepoError::InvalidInput(format!(
                        "area {aid} does not belong to the work item's project"
                    ))
                    .into());
                }
            }
            Some(None) => Some(None),
            None => {
                if project_id.is_some() {
                    let cur_area: Option<i64> =
                        sqlx::query_scalar("SELECT area_id FROM workitem WHERE node_id = $1")
                            .bind(node_id)
                            .fetch_one(&mut *tx)
                            .await?;
                    match cur_area {
                        Some(aid) => {
                            let area_pid: Option<i64> =
                                sqlx::query_scalar("SELECT project_id FROM area WHERE id = $1")
                                    .bind(aid)
                                    .fetch_optional(&mut *tx)
                                    .await?;
                            if effective_pid.is_some() && area_pid == effective_pid {
                                None
                            } else {
                                Some(None)
                            }
                        }
                        None => None,
                    }
                } else {
                    None
                }
            }
        };

        if let Some(v) = &project_id {
            sqlx::query("UPDATE node SET project_id = $2 WHERE id = $1")
                .bind(node_id)
                .bind(*v)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(v) = new_area {
            sqlx::query("UPDATE workitem SET area_id = $2 WHERE node_id = $1")
                .bind(node_id)
                .bind(v)
                .execute(&mut *tx)
                .await?;
        }
    }
    if let Some(v) = parent_node {
        sqlx::query("UPDATE workitem SET parent_node_id = $2 WHERE node_id = $1")
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
    if let Some(v) = &patch.category {
        sqlx::query("UPDATE node SET category = $2 WHERE id = $1")
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
    // D-7: `closed` is Ken-only (see `vocab::WI_STATUSES`), so reaching it means
    // the ask this item was waiting on has been answered. `resolved`/`done`
    // deliberately do NOT clear — "implemented, needs your user test" is the
    // canonical awaiting-Ken state.
    settle_awaiting(&mut *tx, node_id).await?;
    // Sprint 051 (D-4): finishing a materialised maintenance item is what
    // restarts a `completed`-anchored schedule's clock. A write rule here rather
    // than a trigger, per 0023's precedent — and inside the same transaction, so
    // the anchor cannot advance for a status change that then rolls back.
    if let Some(v) = &patch.wi_status {
        advance_completed_anchor(&mut tx, node_id, v).await?;
    }
    touch_node(&mut *tx, node_id).await?;

    tx.commit().await?;
    get_work_item(pool, wi_number)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_number}")).into())
}
