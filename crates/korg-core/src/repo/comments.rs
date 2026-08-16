//! Comments — the one detail table that hangs off any node kind.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;

use super::common::{require_node, require_non_empty};

// --- comments -------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Comment {
    pub id: i64,
    pub node_id: i64,
    pub body: String,
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
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add_comment(pool: &PgPool, node_id: i64, body: &str) -> Result<Comment> {
    require_non_empty(body, "comment body")?;
    require_node(pool, node_id).await?;
    let c = sqlx::query_as::<_, Comment>(
        "INSERT INTO comment (node_id, body) VALUES ($1, $2) \
         RETURNING id, node_id, body, created, updated",
    )
    .bind(node_id)
    .bind(body)
    .fetch_one(pool)
    .await?;
    Ok(c)
}

/// Edit a comment's body (WI #232). The `updated` column advances via the
/// standard trigger; `created` is preserved.
pub async fn update_comment(pool: &PgPool, id: i64, body: &str) -> Result<Comment> {
    require_non_empty(body, "comment body")?;
    let c = sqlx::query_as::<_, Comment>(
        "UPDATE comment SET body = $2 WHERE id = $1 \
         RETURNING id, node_id, body, created, updated",
    )
    .bind(id)
    .bind(body)
    .fetch_optional(pool)
    .await?;
    c.ok_or_else(|| RepoError::NotFound(format!("no comment with id {id}")).into())
}

/// Delete a comment; `false` means there was no such comment (WI #525).
pub async fn delete_comment(pool: &PgPool, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM comment WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
