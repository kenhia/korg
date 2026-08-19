//! Sprint proposals (agent planning), and the lean queue read they are
//! listed through (WI #852).

use anyhow::Result;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::vocab::{PROPOSAL_LIVE_STATUSES, PROPOSAL_STATUSES};

use super::awaiting::settle_awaiting;
use super::comments::Comment;
use super::common::{
    cross_project_covers, node_project, record_transition, require_kind, touch_node,
    validate_status, wi_handle,
};
use super::page::ArchivedFilter;
use super::programs::promote_queued_programs_over;
use super::relationships::{related_context, RelatedRef};
use super::selectors::{project_name_for_id, resolve_project};
use super::work_items::{node_id_for_wi, WORKITEM_COMMENT_CAP};

// --- sprint proposals (agent planning) -------------------------------------

/// The routing contract's only mechanically-enforceable clause (WI #860), the
/// proposal counterpart of [`PROJECT_DESCRIPTION_MAX`].
///
/// 500 rather than projects' 160 because a proposal has more to say than a
/// routing line: what the bundle is, why now, roughly how big. Sized in 036's
/// D-1 against what the *full* row carries — after #852 the lean queue row has
/// no `summary` at all, so this is not sizing the pick.
pub const PROPOSAL_SUMMARY_MAX: usize = 500;

/// Checked here as well as by `sprint_proposal_summary_routing_line` (0021) so
/// an over-long summary is `invalid_input` naming the field and the overage,
/// rather than a raw constraint violation surfacing as `internal`. Characters,
/// not bytes — the constraint uses `char_length` and a summary is prose.
fn check_proposal_summary(summary: &str) -> Result<()> {
    let n = summary.chars().count();
    if n > PROPOSAL_SUMMARY_MAX {
        return Err(RepoError::InvalidInput(format!(
            "proposal summary is {n} characters; the routing contract caps it at \
             {PROPOSAL_SUMMARY_MAX}. Put the analysis in `notes` — it is unbounded, \
             and that is what it exists for."
        ))
        .into());
    }
    Ok(())
}

/// `propose_sprint` / `POST /api/proposals`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewProposal {
    /// **Required** — as either this or `project`, never both. A proposal is a
    /// single-project bundle (#967): it is what tells a session which repo to
    /// branch in, and what `covers` is validated against.
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both, and **one of them is required**. Resolved by exact name, and an
    /// unknown name returns `not_found` rather than mis-filing, so pass a name
    /// you are confident in directly. Call `list_projects` only when the name is
    /// genuinely unknown or ambiguous — the roster in this server's
    /// instructions already names every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    /// The routing contract. **≤500 characters** (rejected above that): what
    /// this bundle is, why now, roughly how big. Written for a session choosing
    /// a sprint from a queue, not for one that has already chosen it.
    pub summary: String,
    /// The analysis — measurements, dependencies, sequencing, alternatives
    /// considered. Unbounded, and the home for everything that will not fit
    /// `summary`. Returned by `get_proposal` and `list_proposals detail:"full"`,
    /// never by the lean queue row.
    #[serde(default)]
    pub notes: Option<String>,
    /// Drag-order position; lower sorts first among unpinned proposals.
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Decimal,
    #[serde(default)]
    pub pinned: bool,
    /// wi_numbers this proposal covers; numbers that don't resolve are dropped.
    /// A number that resolves to a work item in a **different project** is not
    /// dropped — it is `invalid_input` naming both projects (#967), because
    /// silently omitting the item you meant to include is the failure this rule
    /// exists to stop.
    #[serde(default, rename = "work_item_numbers")]
    #[schemars(rename = "work_item_numbers", schema_with = "schema::wi_numbers")]
    pub covers: Vec<i64>,
}

/// The created proposal plus which of the requested wi_numbers resolved.
/// `covered` is the honest echo of a drop-and-report input (F-06): compare it
/// against what you asked for.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalCreated {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: ProposalRow,
    pub covered: Vec<i64>,
}

/// Create a sprint proposal and its `covers` edges to the given work items in
/// one transaction. Mirrors `create_work_item`'s node+detail insert; the
/// wi_number -> node_id resolution happens before the transaction, matching
/// `update_work_item`'s handling of `parent`.
pub async fn create_proposal(pool: &PgPool, new: NewProposal) -> Result<ProposalCreated> {
    check_proposal_summary(&new.summary)?;
    // Sprint 043 (#967): a proposal without a project is what produced almost
    // every cross-project `covers` edge in the corpus — the edges had nothing
    // to be validated against. `resolve_project` returns None for "neither
    // selector given"; for a proposal that is now invalid_input rather than a
    // row nobody can route.
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref())
        .await?
        .ok_or_else(|| {
            RepoError::InvalidInput(
                "a sprint proposal needs a project — pass `project` (the name, e.g. \
                 \"korg\") or `project_id`. A proposal is a single-project bundle: it is \
                 what tells a session which repo to branch in, and what the covered work \
                 items are checked against. list_projects has the roster."
                    .into(),
            )
        })?;
    let project_name = project_name_for_id(pool, project_id).await?;

    let mut covered = Vec::with_capacity(new.covers.len());
    for wi in &new.covers {
        let Some(n) = node_id_for_wi(pool, *wi).await? else {
            continue;
        };
        // The bundled insert below does not go through `relate`, so the
        // single-project rule is applied here too — and before the transaction
        // opens, so a refusal leaves nothing behind. Unlike an unresolvable
        // wi_number (dropped and reported via `covered`, F-06), this is a hard
        // refusal: silently dropping the work item Ken meant to include is the
        // exact failure #967 exists to stop.
        if let Some(wp) = node_project(pool, n).await? {
            if wp != project_name {
                let (_, title) = wi_handle(pool, n).await?;
                return Err(cross_project_covers(&project_name, *wi, &wp, &title));
            }
        }
        covered.push(n);
    }

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('sprint_proposal', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query(
        "INSERT INTO sprint_proposal (node_id, title, summary, notes, rank, pinned) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(node_id)
    .bind(&new.title)
    .bind(&new.summary)
    .bind(&new.notes)
    .bind(new.rank)
    .bind(new.pinned)
    .execute(&mut *tx)
    .await?;

    // Semantic orientation: proposal -> work item (WI #531). This used to
    // insert (least, greatest), which recorded id ordering instead of meaning.
    // Provenance (D-17): origin is this writer's operation name; the ON CONFLICT
    // no-op preserves created/origin on a re-propose.
    for &target in &covered {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'covers', now(), 'propose_sprint') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(node_id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let row = get_proposal(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no proposal with node_id {node_id}")))?;
    Ok(ProposalCreated { row, covered })
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalRow {
    pub node_id: i64,
    pub title: String,
    /// The routing contract, ≤500 chars (WI #860) — what the bundle is, why
    /// now, roughly how big. It used to hold the whole analysis, which is why
    /// the unfiltered `list_proposals` measured ~46k tokens (#852).
    pub summary: String,
    /// The analysis the summary used to carry. Unbounded, `None` on a proposal
    /// whose summary always fit. Migration 0021 moved every over-cap summary
    /// here verbatim.
    pub notes: Option<String>,
    pub status: String,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub pinned: bool,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this proposal (WI #535).
    pub comment_count: i64,
    /// How many work items this proposal covers (WI #536) — the signal that
    /// saves the Planning page a `neighbors` call per row just to show chips.
    pub covered_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const PROPOSAL_SELECT: &str =
    "SELECT p.node_id, p.title, p.summary, p.notes, p.status::text AS status, p.rank, p.pinned, \
            pj.name AS project, n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = p.node_id) AS comment_count, \
            (SELECT count(*) FROM relationship r JOIN node wn ON wn.id = r.right_id \
              WHERE r.left_id = p.node_id AND r.relationship = 'covers' \
                AND wn.kind = 'workitem') AS covered_count, \
            n.created, n.updated \
     FROM sprint_proposal p \
     JOIN node n ON n.id = p.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

#[derive(Debug, Clone, Default)]
pub struct ProposalQuery {
    pub status: Option<String>,
    /// Project name (WI #565) — the queue spans repos, so "show me korg's
    /// sprints" is the common ask.
    pub project: Option<String>,
}

/// List proposals ordered pinned-first, then by rank — the drag-order a user
/// or agent leaves them in — with a `node_id` tie-breaker so equal ranks stop
/// shuffling between calls (F-19).
pub async fn list_proposals(pool: &PgPool, query: ProposalQuery) -> Result<Vec<ProposalRow>> {
    if let Some(status) = &query.status {
        validate_status(status, &PROPOSAL_STATUSES, "proposal status")?;
    }
    let rows = sqlx::query_as::<_, ProposalRow>(&format!(
        "{PROPOSAL_SELECT} WHERE ($1::text IS NULL OR p.status::text = $1) \
           AND ($2::text IS NULL OR pj.name = $2) \
         ORDER BY p.pinned DESC, p.rank ASC, p.node_id ASC"
    ))
    .bind(query.status.as_deref())
    .bind(query.project.as_deref())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// --- the lean proposal queue (WI #852) --------------------------------------
//
// `list_proposals` above is the REST/web read and stays as it is: the Planning
// page fetches unfiltered on purpose (WI #622) and renders `done` and
// `declined` columns out of the summaries. It is a browser, not a token budget.
//
// The MCP read is the one that measured 110 rows / ~185,500 chars / ~46k tokens
// unfiltered, 71% of it `done`. Everything below is that read: a lean
// projection, live statuses by default, and an `omitted` envelope so the
// narrowing is visible. Same split `list_projects` has had since #828.

/// A proposal as the lean queue read reports it — WI #852's field list exactly.
///
/// No `summary`: proposal summaries are the longest prose in korg (they are
/// written as plans) and they are the entire payload problem. `get_proposal` is
/// the full read, and `start-sprint` calls it immediately after the pick anyway
/// — the two-tier read this row completes was already the intended pattern.
///
/// No `archived` either: the lean list excludes archived rows by default, so the
/// flag would be constant. Ask for them and `detail: "full"` carries it.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalLeanRow {
    pub node_id: i64,
    pub title: String,
    pub status: String,
    pub project: Option<String>,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub pinned: bool,
    pub covered_count: i64,
    pub comment_count: i64,
}

/// What the queue read's filters hid (WI #852), computed as a **cascade** so no
/// row is counted twice: `archived` is what the archived filter excluded, and
/// `done`/`declined` are counted only over the rows that *passed* it. An
/// archived `done` proposal therefore lands in `archived` and nowhere else.
///
/// A field is 0 when the caller asked to see that class — `status: "all"` zeroes
/// both terminal counts, `archived: null` zeroes `archived`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalOmitted {
    pub done: i64,
    pub declined: i64,
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalListLean {
    pub items: Vec<ProposalLeanRow>,
    pub omitted: ProposalOmitted,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalListFull {
    pub items: Vec<ProposalRow>,
    pub omitted: ProposalOmitted,
}

/// Resolve the `status` argument into the statuses a queue read returns.
///
/// Absent → the live queue (`proposed` + `active`). The same principle #828
/// applied to projects: *the default must include every row that could be a
/// correct answer to the question the tool is for.* The question here is "what
/// should I work on", and a `done` proposal is never that.
///
/// `Some("all")` → no filter. Anything else is validated against
/// `PROPOSAL_STATUSES` and returned alone.
fn proposal_status_predicate(status: Option<&str>) -> Result<Option<Vec<String>>> {
    match status {
        None => Ok(Some(
            PROPOSAL_LIVE_STATUSES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &PROPOSAL_STATUSES, "proposal status")?;
            Ok(Some(vec![s.to_string()]))
        }
    }
}

const PROPOSAL_LEAN_SELECT: &str =
    "SELECT p.node_id, p.title, p.status::text AS status, p.rank, p.pinned, \
            pj.name AS project, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = p.node_id) AS comment_count, \
            (SELECT count(*) FROM relationship r JOIN node wn ON wn.id = r.right_id \
              WHERE r.left_id = p.node_id AND r.relationship = 'covers' \
                AND wn.kind = 'workitem') AS covered_count \
     FROM sprint_proposal p \
     JOIN node n ON n.id = p.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

/// The shared tail of both queue reads: status set, project, archived, ordering.
const PROPOSAL_QUEUE_WHERE: &str = " WHERE ($1::text[] IS NULL OR p.status::text = ANY($1)) \
        AND ($2::text IS NULL OR pj.name = $2) \
        AND ($3::bool IS NULL OR n.archived = $3) \
      ORDER BY p.pinned DESC, p.rank ASC, p.node_id ASC";

/// Count what the queue read's filters hid, in one round trip.
///
/// The counts are taken over the `project`-filtered corpus — narrowing to a
/// project is the caller *choosing* a scope, not a default hiding rows from
/// them, so it is not something `omitted` should report.
pub(super) async fn proposal_omitted(
    pool: &PgPool,
    project: Option<&str>,
    archived: ArchivedFilter,
    shown: Option<&[String]>,
) -> Result<ProposalOmitted> {
    let (archived_hidden, done, declined) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           count(*) FILTER (WHERE n.archived AND $2::bool IS NOT NULL AND NOT $2), \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND p.status::text = 'done'), \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND p.status::text = 'declined') \
         FROM sprint_proposal p \
         JOIN node n ON n.id = p.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1)",
    )
    .bind(project)
    .bind(archived)
    .fetch_one(pool)
    .await?;
    // A status the caller asked for isn't omitted, however many rows it has.
    let hidden = |status: &str, count: i64| match shown {
        Some(list) if list.iter().any(|s| s == status) => 0,
        None => 0,
        _ => count,
    };
    Ok(ProposalOmitted {
        done: hidden("done", done),
        declined: hidden("declined", declined),
        archived: archived_hidden,
    })
}

/// `list_proposals` over MCP: the lean projection, live statuses and unarchived
/// rows by default, with `omitted` saying what that hid.
pub async fn list_proposals_lean(
    pool: &PgPool,
    status: Option<&str>,
    project: Option<&str>,
    archived: ArchivedFilter,
) -> Result<ProposalListLean> {
    let shown = proposal_status_predicate(status)?;
    let items = sqlx::query_as::<_, ProposalLeanRow>(&format!(
        "{PROPOSAL_LEAN_SELECT}{PROPOSAL_QUEUE_WHERE}"
    ))
    .bind(shown.as_deref())
    .bind(project)
    .bind(archived)
    .fetch_all(pool)
    .await?;
    Ok(ProposalListLean {
        items,
        omitted: proposal_omitted(pool, project, archived, shown.as_deref()).await?,
    })
}

/// `list_proposals` with `detail: "full"` — the same filters and envelope, but
/// every column including `summary`. The escape hatch that keeps the lean
/// default from making anything unreachable.
pub async fn list_proposals_full(
    pool: &PgPool,
    status: Option<&str>,
    project: Option<&str>,
    archived: ArchivedFilter,
) -> Result<ProposalListFull> {
    let shown = proposal_status_predicate(status)?;
    let items =
        sqlx::query_as::<_, ProposalRow>(&format!("{PROPOSAL_SELECT}{PROPOSAL_QUEUE_WHERE}"))
            .bind(shown.as_deref())
            .bind(project)
            .bind(archived)
            .fetch_all(pool)
            .await?;
    Ok(ProposalListFull {
        items,
        omitted: proposal_omitted(pool, project, archived, shown.as_deref()).await?,
    })
}

/// A covered work item as a proposal's detail read reports it — enough to
/// decide and to render, without a second call per item (§4.3).
#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct CoveredRef {
    pub wi_number: i64,
    pub node_id: i64,
    pub title: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub project: Option<String>,
    pub comment_count: i64,
}

/// A proposal plus what it covers and its discussion (WI #536). This is the
/// authoritative "what is this sprint" read: before it, the Planning page
/// fetched every proposal, every work item, then called `neighbors` once per
/// proposal and joined client-side, and `start-sprint` did the same dance over
/// MCP in three tools.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub proposal: ProposalRow,
    /// Covered work items, ordered by wi_number.
    pub covered: Vec<CoveredRef>,
    pub comments: Vec<Comment>,
    pub comments_truncated: bool,
    /// The proposal's non-`covers` edges, inlined (LB-3). `covers` is excluded
    /// because `covered` already carries it.
    pub related: Vec<RelatedRef>,
    /// True when there are more such edges than were inlined (call `neighbors`).
    pub related_truncated: bool,
}

/// `get_proposal` — the proposal, its covered work items, and capped comments.
/// `None` if no proposal has that node id (the transports turn that into
/// 404 / isError per D-6).
pub async fn get_proposal_detail(pool: &PgPool, node_id: i64) -> Result<Option<ProposalDetail>> {
    let Some(proposal) = get_proposal(pool, node_id).await? else {
        return Ok(None);
    };
    // Reads the `covers` edge in its semantic orientation (proposal -> work
    // item), which sprint 014 made trustworthy.
    let covered = sqlx::query_as::<_, CoveredRef>(
        "SELECT w.wi_number, w.node_id, w.title, w.wi_status, w.wi_tshirt, \
                pj.name AS project, \
                (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count \
         FROM relationship r \
         JOIN workitem w ON w.node_id = r.right_id \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE r.left_id = $1 AND r.relationship = 'covers' \
         ORDER BY w.wi_number",
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
    let comments_truncated = proposal.comment_count > WORKITEM_COMMENT_CAP;
    // Everything except covers — that is already inlined as `covered`.
    let (related, related_truncated) = related_context(pool, node_id, Some("covers")).await?;
    Ok(Some(ProposalDetail {
        proposal,
        covered,
        comments,
        comments_truncated,
        related,
        related_truncated,
    }))
}

pub async fn get_proposal(pool: &PgPool, node_id: i64) -> Result<Option<ProposalRow>> {
    Ok(
        sqlx::query_as::<_, ProposalRow>(&format!("{PROPOSAL_SELECT} WHERE p.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// `update_proposal` / `PATCH /api/proposals/:node_id`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ProposalPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    /// The routing contract, **≤500 characters** (rejected above that). Put the
    /// analysis in `notes` — it is unbounded, and that is what it is for.
    #[serde(default)]
    pub summary: Option<String>,
    /// The analysis: measurements, dependencies, sequencing, what was
    /// considered and rejected. Unbounded; pass `null` to clear it.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::proposal_status")]
    pub status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Option<Decimal>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

/// Partially update a proposal: status transitions (propose -> active ->
/// done/declined), reorder (rank), pin, archive. Same "only bind what's
/// present" shape as `update_card`.
pub async fn update_proposal(
    pool: &PgPool,
    node_id: i64,
    patch: ProposalPatch,
) -> Result<ProposalRow> {
    if let Some(v) = &patch.status {
        validate_status(v, &PROPOSAL_STATUSES, "proposal status")?;
    }
    if let Some(v) = &patch.summary {
        check_proposal_summary(v)?;
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "sprint_proposal", "proposal").await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE sprint_proposal SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.summary {
        sqlx::query("UPDATE sprint_proposal SET summary = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    // Double-option: absent leaves `notes` alone, explicit null clears it.
    if let Some(v) = &patch.notes {
        sqlx::query("UPDATE sprint_proposal SET notes = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v.as_deref())
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.status {
        // Read-before-write (#977): the log records status *changes*, and the
        // only way to know one happened is to have seen what was there.
        let before: String =
            sqlx::query_scalar("SELECT status::text FROM sprint_proposal WHERE node_id = $1")
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "UPDATE sprint_proposal SET status = $2::sprint_proposal_status WHERE node_id = $1",
        )
        .bind(node_id)
        .bind(v)
        .execute(&mut *tx)
        .await?;
        record_transition(&mut *tx, node_id, &before, v).await?;
        // #1424: starting a slice starts the program above it. A `queued`
        // program asserts that none of its slices has begun, and this is the
        // usual way that stops being true — start-sprint marking slice 1
        // `active`. No-op unless the new status is a start and some program
        // over this proposal is still queued.
        promote_queued_programs_over(&mut *tx, node_id).await?;
    }
    if let Some(v) = patch.rank {
        sqlx::query("UPDATE sprint_proposal SET rank = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.pinned {
        sqlx::query("UPDATE sprint_proposal SET pinned = $2 WHERE node_id = $1")
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
    // D-7: `done`/`declined` are Ken's calls on the bundle, so an awaiting
    // marker on this proposal has been answered by the transition itself.
    settle_awaiting(&mut *tx, node_id).await?;
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_proposal(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no proposal with node_id {node_id}")).into())
}
