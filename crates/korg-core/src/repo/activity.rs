//! The activity log (WI 2936, migration 0038): what changed in korg, newest
//! first, with each change's proposal and program context.
//!
//! Two tables answer it. `transition` (0026) holds status changes; `activity`
//! holds every other change to a node — created, edited, comments, edges,
//! attachments. [`list_activity`] reads them as one feed, naming the
//! transition rows `status_changed`.
//!
//! **Written here, by korg-core, never by a trigger** (0023/0026), and only for
//! a write that changed something. Creation, comments, edges and attachments
//! are unambiguous at their call sites. `edited` is the hard one, because an
//! update path takes a patch whose fields may all equal what is stored: those
//! paths take a [`fingerprint`] of the node before writing and hand it to
//! [`record_edit`] before commit, which compares it with one taken after and
//! stays silent when they match.
//!
//! The fingerprint leaves out the columns that are not the caller's edit:
//! `updated` (it advances on every write), the awaiting marker (the update
//! paths' own `settle_awaiting` may clear it as a side effect), and the status
//! column of the three kinds `transition` covers — a status-only write is a
//! `status_changed` row there, and logging it here as well would count it
//! twice. A card's or schedule's status has no transition row, so for those
//! kinds it stays in, and a status change reads as `edited`.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, PgPool, Postgres};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::vocab::{node_path, ACTIVITY_ACTIONS, NODE_KINDS};

use super::page::Page;

/// Default and ceiling for one page of the feed.
pub const ACTIVITY_LIMIT_DEFAULT: i64 = 100;
pub const ACTIVITY_LIMIT_MAX: i64 = 500;

// --- writing -----------------------------------------------------------------

/// Append one event with no comment or edge attached: `created`.
pub(super) async fn record_activity<'e, E>(executor: E, node_id: i64, action: &str) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query("INSERT INTO activity (node_id, action) VALUES ($1, $2)")
        .bind(node_id)
        .bind(action)
        .execute(executor)
        .await?;
    Ok(())
}

/// Append a comment event on the comment's node.
pub(super) async fn record_comment_activity<'e, E>(
    executor: E,
    node_id: i64,
    action: &str,
    comment_id: i64,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query("INSERT INTO activity (node_id, action, comment_id) VALUES ($1, $2, $3)")
        .bind(node_id)
        .bind(action)
        .bind(comment_id)
        .execute(executor)
        .await?;
    Ok(())
}

/// Append an edge event on the edge's left node, naming the right one: an
/// edge is logged once, from the side that holds it, not on both ends.
pub(super) async fn record_edge_activity<'e, E>(
    executor: E,
    node_id: i64,
    action: &str,
    other_node_id: i64,
    label: Option<&str>,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO activity (node_id, action, other_node_id, label) VALUES ($1, $2, $3, $4)",
    )
    .bind(node_id)
    .bind(action)
    .bind(other_node_id)
    .bind(label)
    .execute(executor)
    .await?;
    Ok(())
}

/// A digest of everything about a node an edit can change. See the module
/// docs for what is left out and why. `None` only if the node does not exist.
pub(super) async fn fingerprint<'e, E>(executor: E, node_id: i64) -> Result<Option<String>>
where
    E: Executor<'e, Database = Postgres>,
{
    Ok(sqlx::query_scalar::<_, String>(
        "SELECT md5(concat_ws('|', \
            (to_jsonb(n) - 'updated' - 'awaiting_since' - 'awaiting_note')::text, \
            (SELECT (to_jsonb(x) - 'wi_status')::text FROM workitem x        WHERE x.node_id = n.id), \
            (SELECT to_jsonb(x)::text                 FROM card x            WHERE x.node_id = n.id), \
            (SELECT to_jsonb(x)::text                 FROM link x            WHERE x.node_id = n.id), \
            (SELECT (to_jsonb(x) - 'status')::text    FROM sprint_proposal x WHERE x.node_id = n.id), \
            (SELECT (to_jsonb(x) - 'status')::text    FROM program x         WHERE x.node_id = n.id), \
            (SELECT to_jsonb(x)::text                 FROM report x          WHERE x.node_id = n.id), \
            (SELECT to_jsonb(x)::text                 FROM handoff x         WHERE x.node_id = n.id), \
            (SELECT to_jsonb(x)::text                 FROM schedule x        WHERE x.node_id = n.id))) \
           FROM node n WHERE n.id = $1",
    )
    .bind(node_id)
    .fetch_optional(executor)
    .await?)
}

/// Record `edited` if the node's fingerprint moved since `before` — the no-op
/// guard. Call it inside the update's own transaction, after its last write,
/// so an event cannot survive a rolled-back update.
pub(super) async fn record_edit(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    node_id: i64,
    before: Option<String>,
) -> Result<()> {
    let after = fingerprint(&mut **tx, node_id).await?;
    if after.is_some() && after != before {
        record_activity(&mut **tx, node_id, "edited").await?;
    }
    Ok(())
}

// --- reading -----------------------------------------------------------------

/// A node an event points at, resolved enough to render a link and a label.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ActivityRef {
    pub node_id: i64,
    pub kind: String,
    /// The user-facing handle when the node is a work item.
    pub wi_number: Option<i64>,
    pub title: String,
    /// korg's own page for the node (GP-16), or `None` for a kind with none.
    pub url: Option<String>,
}

/// One line of the feed.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ActivityEvent {
    /// Unique across both source tables: `a<id>` for `activity`, `t<id>` for
    /// `transition`. The feed's stable row key.
    pub key: String,
    /// One of `ACTIVITY_ACTIONS`.
    pub action: String,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub at: OffsetDateTime,
    /// The node that changed.
    pub target: ActivityRef,
    /// The target's project; `None` for a program, which has none.
    pub project: Option<String>,
    /// The comment, for `comment_*`.
    pub comment_id: Option<i64>,
    /// For `status_changed`.
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    /// The edge's other end (`related`/`unrelated`) or the attachment
    /// (`attached`/`detached`). `None` if it has since been deleted.
    pub other: Option<ActivityRef>,
    /// The relationship label, for `related`/`unrelated`.
    pub label: Option<String>,
    /// The proposal that `covers` the target, resolved now through the edge
    /// rather than stored. `None` when the target is itself a proposal.
    pub proposal: Option<ActivityRef>,
    /// The program that `includes` that proposal, or that includes the target
    /// when the target is a proposal. `None` when the target is itself a
    /// program.
    pub program: Option<ActivityRef>,
}

/// The feed's filters. Every one is optional; together they AND.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActivityFilter {
    pub action: Option<String>,
    pub kind: Option<String>,
    pub project: Option<String>,
    /// Inclusive lower bound, RFC 3339.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub since: Option<OffsetDateTime>,
    /// Exclusive upper bound, RFC 3339.
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub until: Option<OffsetDateTime>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// The title of node alias `{a}`, given its satellite aliases `{a}_w` etc.
fn title_joins(a: &str) -> String {
    format!(
        "LEFT JOIN workitem {a}_w         ON {a}_w.node_id  = {a}.id \
         LEFT JOIN sprint_proposal {a}_sp ON {a}_sp.node_id = {a}.id \
         LEFT JOIN program {a}_g          ON {a}_g.node_id  = {a}.id \
         LEFT JOIN card {a}_cd            ON {a}_cd.node_id = {a}.id \
         LEFT JOIN link {a}_lk            ON {a}_lk.node_id = {a}.id \
         LEFT JOIN report {a}_rp          ON {a}_rp.node_id = {a}.id \
         LEFT JOIN handoff {a}_hd         ON {a}_hd.node_id = {a}.id \
         LEFT JOIN schedule {a}_sc        ON {a}_sc.node_id = {a}.id \
         LEFT JOIN attachment {a}_at      ON {a}_at.node_id = {a}.id "
    )
}

fn title_expr(a: &str) -> String {
    format!(
        "COALESCE({a}_w.title, {a}_sp.title, {a}_g.title, {a}_cd.title, {a}_lk.title, \
                  {a}_lk.url, {a}_rp.summary, {a}_hd.title, {a}_sc.title, {a}_at.filename, \
                  {a}.kind || ' #' || {a}.id)"
    )
}

/// One row as the feed query returns it, before the refs are assembled.
#[derive(sqlx::FromRow)]
struct RawEvent {
    key: String,
    action: String,
    at: OffsetDateTime,
    n_id: i64,
    n_kind: String,
    n_wi: Option<i64>,
    n_title: String,
    project: Option<String>,
    comment_id: Option<i64>,
    from_status: Option<String>,
    to_status: Option<String>,
    label: Option<String>,
    o_id: Option<i64>,
    o_kind: Option<String>,
    o_wi: Option<i64>,
    o_title: Option<String>,
    p_id: Option<i64>,
    p_title: Option<String>,
    g_id: Option<i64>,
    g_title: Option<String>,
}

/// `GET /api/activity` — the feed, newest first, as the collection envelope.
pub async fn list_activity(pool: &PgPool, filter: ActivityFilter) -> Result<Page<ActivityEvent>> {
    if let Some(a) = &filter.action {
        if !ACTIVITY_ACTIONS.contains(&a.as_str()) {
            return Err(RepoError::invalid(format!(
                "unknown action {a:?}; expected one of {}",
                ACTIVITY_ACTIONS.join(", ")
            ))
            .into());
        }
    }
    if let Some(k) = &filter.kind {
        if !NODE_KINDS.contains(&k.as_str()) {
            return Err(RepoError::invalid(format!(
                "unknown kind {k:?}; expected one of {}",
                NODE_KINDS.join(", ")
            ))
            .into());
        }
    }
    let limit = filter.limit.unwrap_or(ACTIVITY_LIMIT_DEFAULT);
    if !(1..=ACTIVITY_LIMIT_MAX).contains(&limit) {
        return Err(RepoError::invalid(format!(
            "limit must be between 1 and {ACTIVITY_LIMIT_MAX}"
        ))
        .into());
    }
    let offset = filter.offset.unwrap_or(0);
    if offset < 0 {
        return Err(RepoError::invalid("offset must not be negative").into());
    }

    // Both tables as one event stream. An update that edits and moves the
    // status writes both in one transaction, so they share `now()`; `src` puts
    // the transition on top of that instant — the status is the headline — and
    // a refresh never reshuffles them.
    let events = "SELECT 'a' || a.id AS key, a.at, 0 AS src, a.id AS seq, a.node_id, \
                         a.action, a.comment_id, a.other_node_id, a.label, \
                         NULL::text AS from_status, NULL::text AS to_status \
                    FROM activity a \
                  UNION ALL \
                  SELECT 't' || t.id, t.at, 1, t.id, t.node_id, 'status_changed', \
                         NULL, NULL, NULL, t.from_status, t.to_status \
                    FROM transition t";
    let filters = "WHERE ($1::text IS NULL OR ev.action = $1) \
                     AND ($2::text IS NULL OR n.kind = $2) \
                     AND ($3::text IS NULL OR pj.name = $3) \
                     AND ($4::timestamptz IS NULL OR ev.at >= $4) \
                     AND ($5::timestamptz IS NULL OR ev.at <  $5)";

    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM ({events}) ev \
           JOIN node n ON n.id = ev.node_id \
           LEFT JOIN project pj ON pj.id = n.project_id \
         {filters}"
    ))
    .bind(&filter.action)
    .bind(&filter.kind)
    .bind(&filter.project)
    .bind(filter.since)
    .bind(filter.until)
    .fetch_one(pool)
    .await?;

    // Context, resolved now (0038: never copied onto the row). The covering
    // proposal is the newest `covers` edge into the target; the program is the
    // newest `includes` edge into that proposal — or into the target, when the
    // target is itself a proposal.
    let sql = format!(
        "SELECT ev.key, ev.action, ev.at, \
                n.id AS n_id, n.kind AS n_kind, n_w.wi_number AS n_wi, {n_title} AS n_title, \
                pj.name AS project, \
                ev.comment_id, ev.from_status, ev.to_status, ev.label, \
                o.id AS o_id, o.kind AS o_kind, o_w.wi_number AS o_wi, \
                CASE WHEN o.id IS NULL THEN NULL ELSE {o_title} END AS o_title, \
                cov.left_id AS p_id, cov_sp.title AS p_title, \
                inc.left_id AS g_id, inc_g.title AS g_title \
           FROM ({events}) ev \
           JOIN node n ON n.id = ev.node_id \
           {n_joins} \
           LEFT JOIN project pj ON pj.id = n.project_id \
           LEFT JOIN node o ON o.id = ev.other_node_id \
           {o_joins} \
           LEFT JOIN LATERAL ( \
               SELECT r.left_id FROM relationship r \
                 JOIN node pn ON pn.id = r.left_id AND pn.kind = 'sprint_proposal' \
                WHERE r.relationship = 'covers' AND r.right_id = n.id \
                ORDER BY r.id DESC LIMIT 1) cov ON true \
           LEFT JOIN sprint_proposal cov_sp ON cov_sp.node_id = cov.left_id \
           LEFT JOIN LATERAL ( \
               SELECT r.left_id FROM relationship r \
                 JOIN node gn ON gn.id = r.left_id AND gn.kind = 'program' \
                WHERE r.relationship = 'includes' \
                  AND r.right_id = CASE WHEN n.kind = 'sprint_proposal' THEN n.id \
                                        ELSE cov.left_id END \
                ORDER BY r.id DESC LIMIT 1) inc ON true \
           LEFT JOIN program inc_g ON inc_g.node_id = inc.left_id \
         {filters} \
          ORDER BY ev.at DESC, ev.src DESC, ev.seq DESC \
          LIMIT $6 OFFSET $7",
        n_title = title_expr("n"),
        o_title = title_expr("o"),
        n_joins = title_joins("n"),
        o_joins = title_joins("o"),
    );

    let rows: Vec<RawEvent> = sqlx::query_as(&sql)
        .bind(&filter.action)
        .bind(&filter.kind)
        .bind(&filter.project)
        .bind(filter.since)
        .bind(filter.until)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;

    let node_ref = |id: i64, kind: String, wi_number: Option<i64>, title: String| ActivityRef {
        url: node_path(&kind, id),
        node_id: id,
        kind,
        wi_number,
        title,
    };

    let items = rows
        .into_iter()
        .map(|r| {
            let other = match (r.o_id, r.o_kind, r.o_title) {
                (Some(id), Some(kind), Some(title)) => Some(node_ref(id, kind, r.o_wi, title)),
                _ => None,
            };
            let proposal = match (r.p_id, r.p_title) {
                (Some(id), Some(title)) => {
                    Some(node_ref(id, "sprint_proposal".to_owned(), None, title))
                }
                _ => None,
            };
            let program = match (r.g_id, r.g_title) {
                (Some(id), Some(title)) => Some(node_ref(id, "program".to_owned(), None, title)),
                _ => None,
            };
            ActivityEvent {
                key: r.key,
                action: r.action,
                at: r.at,
                target: node_ref(r.n_id, r.n_kind, r.n_wi, r.n_title),
                project: r.project,
                comment_id: r.comment_id,
                from_status: r.from_status,
                to_status: r.to_status,
                label: r.label,
                other,
                proposal,
                program,
            }
        })
        .collect();

    Ok(Page {
        items,
        total,
        limit,
        offset,
    })
}
