//! Reading-list links.

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Transaction};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::schema;
use crate::vocab::LINK_DISPOSITIONS;

use super::common::{require_kind, require_node, require_non_empty, touch_node, validate_status};
use super::page::{archived_default, ArchivedFilter, Page, PageQuery};
use super::selectors::resolve_project;

// --- reading-list links ---------------------------------------------------

/// `create_link` / `POST /api/links`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewLink {
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
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct LinkRow {
    pub node_id: i64,
    pub url: String,
    pub title: Option<String>,
    pub read: bool,
    pub disposition: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Capture recency (WI #888). Links are nodes, so `node` has carried both
    /// timestamps since 0001 — the row simply never selected them, leaving the
    /// one kind whose whole point is *when did I capture this* unable to say.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

pub async fn create_link(pool: &PgPool, new: NewLink) -> Result<LinkRow> {
    require_non_empty(&new.url, "link url")?;
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('link', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query("INSERT INTO link (node_id, url, title) VALUES ($1, $2, $3)")
        .bind(node_id)
        .bind(&new.url)
        .bind(&new.title)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    get_link(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no link with node_id {node_id}")).into())
}

const LINK_SELECT: &str =
    "SELECT l.node_id, l.url, l.title, l.read, l.disposition::text AS disposition, \
            n.category, n.tags, n.archived, n.created, n.updated \
     FROM link l JOIN node n ON n.id = l.node_id";

#[derive(Debug, Clone)]
pub struct LinkQuery {
    pub disposition: Option<String>,
    pub read: Option<bool>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for LinkQuery {
    fn default() -> Self {
        Self {
            disposition: None,
            read: None,
            archived: archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Reading-list links, enveloped and bounded (WI #534). Without this the read
/// returned the entire capture history forever.
pub async fn list_links(pool: &PgPool, query: LinkQuery) -> Result<Page<LinkRow>> {
    if let Some(d) = &query.disposition {
        validate_status(d, &LINK_DISPOSITIONS, "link disposition")?;
    }
    let (limit, offset) = query.page.resolve();
    const WHERE: &str = "WHERE ($1::text IS NULL OR l.disposition::text = $1) \
           AND ($2::bool IS NULL OR l.read = $2) \
           AND ($3::bool IS NULL OR n.archived = $3)";
    let items = sqlx::query_as::<_, LinkRow>(&format!(
        "{LINK_SELECT} {WHERE} ORDER BY l.node_id LIMIT $4 OFFSET $5"
    ))
    .bind(query.disposition.as_deref())
    .bind(query.read)
    .bind(query.archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM link l JOIN node n ON n.id = l.node_id {WHERE}"
    ))
    .bind(query.disposition.as_deref())
    .bind(query.read)
    .bind(query.archived)
    .fetch_one(pool)
    .await?;
    Ok(Page::new(items, total, limit, offset))
}

pub async fn get_link(pool: &PgPool, node_id: i64) -> Result<Option<LinkRow>> {
    Ok(
        sqlx::query_as::<_, LinkRow>(&format!("{LINK_SELECT} WHERE l.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// Everything `update_link` can change in one transaction. `None` leaves a
/// field alone.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct LinkPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::disposition")]
    pub disposition: Option<String>,
    #[serde(default)]
    pub read: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
    /// The lifecycle end for a link that was real (WI #888). `list_links` has
    /// documented an archived-excluded default since #534 with no write able
    /// to reach it; this is that write. For a capture that was never real —
    /// a probe, a mistyped URL — `delete_link` is the honest disposal.
    #[serde(default)]
    pub archived: Option<bool>,
}

/// One transactional link update (WI #538). The REST handler used to make up
/// to three independent repo calls, so a mid-sequence failure left a partial
/// write and an error that didn't say which parts landed. Validation happens
/// before the transaction opens, so an invalid disposition changes nothing.
pub async fn update_link(pool: &PgPool, node_id: i64, patch: LinkPatch) -> Result<LinkRow> {
    if let Some(d) = &patch.disposition {
        validate_status(d, &LINK_DISPOSITIONS, "link disposition")?;
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "link", "link").await?;
    if let Some(d) = &patch.disposition {
        sqlx::query("UPDATE link SET disposition = $2::link_disposition WHERE node_id = $1")
            .bind(node_id)
            .bind(d)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(read) = patch.read {
        sqlx::query("UPDATE link SET read = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(read)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tags) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(tags)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(archived) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(archived)
            .execute(&mut *tx)
            .await?;
    }
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    reread_link(pool, node_id).await
}

/// Hard-delete a link (WI #888) — the disposal for a capture that was never
/// real. `Ok(false)` means there was no such link, matching `delete_comment`.
///
/// It **refuses** rather than cascading. `relationship` and `comment` both
/// cascade from `node`, so an unguarded delete would take a link's edges and
/// its whole thread with it. Anything referenced is by definition real;
/// archive it instead.
pub async fn delete_link(pool: &PgPool, node_id: i64) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await?;
    match kind.as_deref() {
        None => return Ok(false),
        Some("link") => {}
        Some(other) => {
            return Err(
                RepoError::invalid(format!("node {node_id} is a {other}, not a link")).into(),
            );
        }
    }
    refuse_if_referenced(&mut tx, node_id).await?;
    sqlx::query("DELETE FROM node WHERE id = $1")
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// The refuse-if-referenced guard shared by korg's node-level hard deletes.
/// A row something else points at is a row with a history, and history is what
/// `archived` is for — so the caller has to resolve the reference explicitly
/// rather than let the schema's cascade decide silently.
async fn refuse_if_referenced(tx: &mut Transaction<'_, Postgres>, node_id: i64) -> Result<()> {
    let edges: i64 =
        sqlx::query_scalar("SELECT count(*) FROM relationship WHERE left_id = $1 OR right_id = $1")
            .bind(node_id)
            .fetch_one(&mut **tx)
            .await?;
    let comments: i64 = sqlx::query_scalar("SELECT count(*) FROM comment WHERE node_id = $1")
        .bind(node_id)
        .fetch_one(&mut **tx)
        .await?;
    if edges + comments > 0 {
        let mut refs = Vec::new();
        if edges > 0 {
            refs.push(format!("{edges} relationship(s)"));
        }
        if comments > 0 {
            refs.push(format!("{comments} comment(s)"));
        }
        return Err(RepoError::Conflict(format!(
            "node {node_id} is referenced by {} — delete is for rows that were \
             never real; archive it instead, or remove the references first",
            refs.join(", ")
        ))
        .into());
    }
    Ok(())
}

pub async fn set_link_disposition(
    pool: &PgPool,
    node_id: i64,
    disposition: &str,
) -> Result<LinkRow> {
    validate_status(disposition, &LINK_DISPOSITIONS, "link disposition")?;
    require_kind(pool, node_id, "link", "link").await?;
    sqlx::query("UPDATE link SET disposition = $2::link_disposition WHERE node_id = $1")
        .bind(node_id)
        .bind(disposition)
        .execute(pool)
        .await?;
    touch_node(pool, node_id).await?;
    reread_link(pool, node_id).await
}

/// Update the cross-cutting tags on any node.
pub async fn set_node_tags(pool: &PgPool, node_id: i64, tags: &[String]) -> Result<()> {
    require_node(pool, node_id).await?;
    sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
        .bind(node_id)
        .bind(tags)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_link_read(pool: &PgPool, node_id: i64, read: bool) -> Result<LinkRow> {
    require_kind(pool, node_id, "link", "link").await?;
    sqlx::query("UPDATE link SET read = $2 WHERE node_id = $1")
        .bind(node_id)
        .bind(read)
        .execute(pool)
        .await?;
    touch_node(pool, node_id).await?;
    reread_link(pool, node_id).await
}

async fn reread_link(pool: &PgPool, node_id: i64) -> Result<LinkRow> {
    get_link(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no link with node_id {node_id}")).into())
}
