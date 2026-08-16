//! Handoffs (durable cross-agent/session context)
//!
//! A handoff is a node like any other (the report/proposal pattern): a detail
//! table for its own fields, attached to the work it describes through the
//! generalized `relationship` table (label `has_handoff`, subject -> handoff).
//! The read path is inherited whole from LB-3: a `has_handoff` edge surfaces in
//! `get_work_item`/`get_proposal`'s `related` block automatically, so there are
//! no handoff-specific projection fields on those reads (sprint 025).

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::schema;

use super::common::{node_kind, require_kind, touch_node};
use super::relationships::{related_context, RelatedRef};
use super::selectors::resolve_project;

/// `create_handoff` / `POST /api/handoffs`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewHandoff {
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
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    #[schemars(schema_with = "schema::non_empty")]
    pub summary: String,
    /// The full handoff document, Markdown.
    pub body: String,
    /// Nodes this handoff describes — its owners. Each becomes a `has_handoff`
    /// edge (owner -> handoff). Rejected if any id does not resolve (the whole
    /// create rolls back — a handoff must not silently lose an owner).
    #[serde(default)]
    pub related_node_ids: Vec<i64>,
    /// Opt in to a handoff with no owners. Off by default so a forgotten link
    /// step cannot silently orphan a handoff (plan Write contract).
    #[serde(default)]
    pub allow_standalone: bool,
}

/// The created handoff plus the owner node ids actually linked (deduped). Since
/// create rejects any id that does not resolve, this echoes the request minus
/// duplicates — the honest confirmation of what was attached.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct HandoffCreated {
    #[serde(flatten)]
    #[ts(flatten)]
    pub handoff: HandoffRow,
    pub related_node_ids: Vec<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct HandoffRow {
    pub node_id: i64,
    pub title: String,
    pub summary: String,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this handoff (nodes are comment-generic, 0007).
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const HANDOFF_SELECT: &str = "SELECT h.node_id, h.title, h.summary, \
            pj.name AS project, n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = h.node_id) AS comment_count, \
            n.created, n.updated \
     FROM handoff h \
     JOIN node n ON n.id = h.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

async fn get_handoff_row(pool: &PgPool, node_id: i64) -> Result<Option<HandoffRow>> {
    Ok(
        sqlx::query_as::<_, HandoffRow>(&format!("{HANDOFF_SELECT} WHERE h.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// A handoff with its full Markdown body and the nodes it is attached to,
/// inlined the LB-3 way (both directions, nothing excluded). This is the
/// authoritative "read this handoff" call.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct HandoffFull {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: HandoffRow,
    pub body: String,
    /// The nodes this handoff is attached to (has_handoff both ways, plus any
    /// other edges), inlined up to [`RELATED_CONTEXT_CAP`] (LB-3).
    pub related: Vec<RelatedRef>,
    pub related_truncated: bool,
}

/// Create a handoff, its detail row, and one `has_handoff` edge per owner in a
/// single transaction. Mirrors `create_proposal`'s node+detail+edges insert;
/// the owner-existence check happens before the transaction, matching how
/// `create_proposal` resolves its work items. Unlike `upsert_report`, which
/// silently drops findings that don't resolve, this *rejects* an unknown owner:
/// a handoff that loses an owner unnoticed is the exact invisible-context
/// failure the handoff feature exists to prevent.
pub async fn create_handoff(pool: &PgPool, new: NewHandoff) -> Result<HandoffCreated> {
    if new.title.trim().is_empty() {
        return Err(RepoError::InvalidInput("handoff title must not be empty".into()).into());
    }
    if new.summary.trim().is_empty() {
        return Err(RepoError::InvalidInput("handoff summary must not be empty".into()).into());
    }
    if new.related_node_ids.is_empty() && !new.allow_standalone {
        return Err(RepoError::InvalidInput(
            "a handoff needs at least one related node (its owner); \
             pass allow_standalone to create one with none"
                .into(),
        )
        .into());
    }
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;

    // Reject any owner that doesn't resolve, and dedup. node_kind turns a missing
    // id into a clean `not_found` naming it, rather than the raw FK violation the
    // edge insert would otherwise surface as `internal` (WI #524). The FK on
    // relationship.left_id is still the backstop if one is deleted mid-flight.
    let mut owners: Vec<i64> = Vec::with_capacity(new.related_node_ids.len());
    for &id in &new.related_node_ids {
        node_kind(pool, id).await?;
        if !owners.contains(&id) {
            owners.push(id);
        }
    }

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('handoff', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query("INSERT INTO handoff (node_id, title, summary, body) VALUES ($1, $2, $3, $4)")
        .bind(node_id)
        .bind(&new.title)
        .bind(&new.summary)
        .bind(&new.body)
        .execute(&mut *tx)
        .await?;

    // Owner -> handoff (subject on the left, per the registry). Provenance
    // stamped; ON CONFLICT preserves the original created/origin, as every other
    // edge writer does.
    for &owner in &owners {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'has_handoff', now(), 'create_handoff') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(owner)
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let handoff = get_handoff_row(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no handoff with node_id {node_id}")))?;
    Ok(HandoffCreated {
        handoff,
        related_node_ids: owners,
    })
}

/// `get_handoff` — the handoff, its Markdown body, and the nodes it is attached
/// to. `None` if no handoff has that node id (transports turn that into 404 /
/// isError per D-6).
pub async fn get_handoff(pool: &PgPool, node_id: i64) -> Result<Option<HandoffFull>> {
    let Some(row) = get_handoff_row(pool, node_id).await? else {
        return Ok(None);
    };
    let body: String = sqlx::query_scalar("SELECT body FROM handoff WHERE node_id = $1")
        .bind(node_id)
        .fetch_one(pool)
        .await?;
    // Nothing excluded: a handoff wants to show every owner it is attached to.
    let (related, related_truncated) = related_context(pool, node_id, None).await?;
    Ok(Some(HandoffFull {
        row,
        body,
        related,
        related_truncated,
    }))
}

/// `update_handoff` / `PATCH /api/handoffs/:node_id`. Partial: only passed
/// fields change. Relationship changes go through `relate`/`unrelate`, not here
/// (plan Write contract). Same "only bind what's present" shape as
/// `update_proposal`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct HandoffPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub archived: Option<bool>,
}

pub async fn update_handoff(
    pool: &PgPool,
    node_id: i64,
    patch: HandoffPatch,
) -> Result<HandoffRow> {
    if patch.title.as_ref().is_some_and(|v| v.trim().is_empty()) {
        return Err(RepoError::InvalidInput("handoff title must not be empty".into()).into());
    }
    if patch.summary.as_ref().is_some_and(|v| v.trim().is_empty()) {
        return Err(RepoError::InvalidInput("handoff summary must not be empty".into()).into());
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "handoff", "handoff").await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE handoff SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.summary {
        sqlx::query("UPDATE handoff SET summary = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.body {
        sqlx::query("UPDATE handoff SET body = $2 WHERE node_id = $1")
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
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_handoff_row(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no handoff with node_id {node_id}")).into())
}
