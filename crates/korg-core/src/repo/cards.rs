//! Cards — create, read, and the move-plus-rank update that does both in one call.

use anyhow::Result;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::vocab::CARD_STATUSES;

use super::common::{require_kind, touch_node, validate_status};
use super::page::{archived_default, ArchivedFilter, Page, PageQuery};
use super::selectors::{resolve_project, resolve_project_patch};

// --- cards ----------------------------------------------------------------

/// `create_card` / `POST /api/cards`. `rank` arrives as a JSON number and is
/// kept as a `Decimal` so fractional insertion never loses precision.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewCard {
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
    #[serde(default = "ops::default_backlog")]
    #[schemars(schema_with = "schema::card_status")]
    pub status: String,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Decimal,
}

pub async fn create_card(pool: &PgPool, new: NewCard) -> Result<CardRow> {
    validate_status(&new.status, &CARD_STATUSES, "card status")?;
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('card', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query(
        "INSERT INTO card (node_id, status, title, description, rank) \
         VALUES ($1, $2::card_status, $3, $4, $5)",
    )
    .bind(node_id)
    .bind(&new.status)
    .bind(&new.title)
    .bind(&new.description)
    .bind(new.rank)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    get_card(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no card with node_id {node_id}")).into())
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct CardRow {
    pub node_id: i64,
    pub status: String,
    pub title: String,
    pub description: String,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this card (WI #535).
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const CARD_SELECT: &str =
    "SELECT c.node_id, c.status::text AS status, c.title, c.description, c.rank, \
            pj.name AS project, n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = c.node_id) AS comment_count, \
            n.created, n.updated \
     FROM card c \
     JOIN node n ON n.id = c.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

#[derive(Debug, Clone)]
pub struct CardQuery {
    pub status: Option<String>,
    pub project: Option<String>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for CardQuery {
    fn default() -> Self {
        Self {
            status: None,
            project: None,
            archived: archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Cards, enveloped and bounded (WI #534). Ordering gains a `node_id`
/// tie-breaker (F-19) so equal-rank cards don't shuffle between calls.
pub async fn list_cards(pool: &PgPool, query: CardQuery) -> Result<Page<CardRow>> {
    if let Some(status) = &query.status {
        validate_status(status, &CARD_STATUSES, "card status")?;
    }
    let (limit, offset) = query.page.resolve();
    const WHERE: &str = "WHERE ($1::text IS NULL OR c.status::text = $1) \
           AND ($2::text IS NULL OR pj.name = $2) \
           AND ($3::bool IS NULL OR n.archived = $3)";
    let items = sqlx::query_as::<_, CardRow>(&format!(
        "{CARD_SELECT} {WHERE} ORDER BY c.status, c.rank ASC, c.node_id ASC LIMIT $4 OFFSET $5"
    ))
    .bind(query.status.as_deref())
    .bind(query.project.as_deref())
    .bind(query.archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM card c JOIN node n ON n.id = c.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id {WHERE}"
    ))
    .bind(query.status.as_deref())
    .bind(query.project.as_deref())
    .bind(query.archived)
    .fetch_one(pool)
    .await?;
    Ok(Page::new(items, total, limit, offset))
}

pub async fn get_card(pool: &PgPool, node_id: i64) -> Result<Option<CardRow>> {
    Ok(
        sqlx::query_as::<_, CardRow>(&format!("{CARD_SELECT} WHERE c.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

// --- cards (update: move + rank in one) -----------------------------------

/// `update_card` / `PATCH /api/cards/:node_id`. Projects are addressed by
/// `project_id` on both transports (WI #537).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CardPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::card_status")]
    pub status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Option<Decimal>,
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project_id: Option<Option<i64>>,
    /// Project name — the alternative to `project_id`; null unassigns. Never pass both.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub category: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

pub async fn update_card(pool: &PgPool, node_id: i64, patch: CardPatch) -> Result<CardRow> {
    if let Some(status) = &patch.status {
        validate_status(status, &CARD_STATUSES, "card status")?;
    }
    let project_id = resolve_project_patch(pool, patch.project_id, patch.project).await?;
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "card", "card").await?;
    if let Some(status) = &patch.status {
        sqlx::query("UPDATE card SET status = $2::card_status WHERE node_id = $1")
            .bind(node_id)
            .bind(status)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(rank) = patch.rank {
        sqlx::query("UPDATE card SET rank = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(rank)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(title) = &patch.title {
        sqlx::query("UPDATE card SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(title)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = &patch.description {
        sqlx::query("UPDATE card SET description = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(description)
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
    if let Some(project_id) = project_id {
        sqlx::query("UPDATE node SET project_id = $2 WHERE id = $1")
            .bind(node_id)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(category) = &patch.category {
        sqlx::query("UPDATE node SET category = $2 WHERE id = $1")
            .bind(node_id)
            .bind(category)
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
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_card(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no card with node_id {node_id}")).into())
}
