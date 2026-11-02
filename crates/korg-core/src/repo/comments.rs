//! Comments — the one detail table that hangs off any node kind.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;

use super::activity::record_comment_activity;
use super::common::{require_node, require_non_empty};

// --- comments -------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Comment {
    pub id: i64,
    pub node_id: i64,
    pub body: String,
    /// Self-reported, unverified provenance of the writer (#1879), exactly the
    /// `relationship.origin` convention (D-17): the web client sends `"web"`, a
    /// skill sends its own name. `None` means the comment predates provenance
    /// or the writer declined to identify itself — korg is no-auth HTTP on the
    /// fleet, so this is an audit aid and never a control.
    pub origin: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// Comments are node-scoped: any node (work item, card, …) can carry comments.
pub async fn list_comments(pool: &PgPool, node_id: i64) -> Result<Vec<Comment>> {
    let rows = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, origin, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add_comment(
    pool: &PgPool,
    node_id: i64,
    body: &str,
    origin: Option<&str>,
) -> Result<Comment> {
    require_non_empty(body, "comment body")?;
    require_node(pool, node_id).await?;
    // Provenance (#1879) follows D-17: stamped on insert, self-reported, and
    // NULL when the writer sent nothing. No caller-derived guess is
    // substituted — a fabricated origin is worse than an absent one.
    let mut tx = pool.begin().await?;
    let c = sqlx::query_as::<_, Comment>(
        "INSERT INTO comment (node_id, body, origin) VALUES ($1, $2, $3) \
         RETURNING id, node_id, body, origin, created, updated",
    )
    .bind(node_id)
    .bind(body)
    .bind(origin)
    .fetch_one(&mut *tx)
    .await?;
    record_comment_activity(&mut *tx, node_id, "comment_added", c.id).await?;
    tx.commit().await?;
    Ok(c)
}

/// Edit a comment's body (WI #232). The `updated` column advances via the
/// standard trigger; `created` is preserved.
///
/// `origin` (#1879) is re-stamped only when the caller sends one: an editor
/// that identifies itself takes the stamp, because it wrote the body that is
/// now there. An edit that sends nothing leaves the existing origin alone
/// rather than clearing it — the same shape as relate's ON CONFLICT no-op,
/// which preserves the original provenance instead of blanking it.
pub async fn update_comment(
    pool: &PgPool,
    id: i64,
    body: &str,
    origin: Option<&str>,
) -> Result<Comment> {
    require_non_empty(body, "comment body")?;
    let mut tx = pool.begin().await?;
    // Read-before-write for the activity log's no-op guard (WI 2936): an
    // edit that leaves the body and origin as they were logs nothing.
    let before: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT body, origin FROM comment WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let c = sqlx::query_as::<_, Comment>(
        "UPDATE comment SET body = $2, origin = COALESCE($3, origin) WHERE id = $1 \
         RETURNING id, node_id, body, origin, created, updated",
    )
    .bind(id)
    .bind(body)
    .bind(origin)
    .fetch_optional(&mut *tx)
    .await?;
    let c = c.ok_or_else(|| RepoError::NotFound(format!("no comment with id {id}")))?;
    if before != Some((c.body.clone(), c.origin.clone())) {
        record_comment_activity(&mut *tx, c.node_id, "comment_edited", c.id).await?;
    }
    tx.commit().await?;
    Ok(c)
}

/// Delete a comment; `false` means there was no such comment (WI #525).
pub async fn delete_comment(pool: &PgPool, id: i64) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let node_id: Option<i64> =
        sqlx::query_scalar("DELETE FROM comment WHERE id = $1 RETURNING node_id")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(node_id) = node_id {
        record_comment_activity(&mut *tx, node_id, "comment_deleted", id).await?;
    }
    tx.commit().await?;
    Ok(node_id.is_some())
}
