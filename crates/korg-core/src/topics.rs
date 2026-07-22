//! Reusable planning topics backed by first-class nodes.

use anyhow::Result;
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;

use crate::error::RepoError;
use crate::repo::{ArchivedFilter, Page, PageQuery};

#[derive(Debug, Clone)]
pub struct NewTopic {
    pub project_id: Option<i64>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TopicPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub category: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq)]
pub struct Topic {
    pub node_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub project_id: Option<i64>,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this topic (WI #535) — the two-level read contract
    /// generalized past work items: any commentable row says so.
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated: OffsetDateTime,
}

fn validate_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RepoError::invalid("topic name is required").into());
    }
    Ok(name)
}

fn topic_not_found(node_id: i64) -> anyhow::Error {
    RepoError::NotFound(format!("no topic with node_id {node_id}")).into()
}

pub async fn create_topic(pool: &PgPool, new: NewTopic) -> Result<Topic> {
    let name = validate_name(&new.name)?;
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('topic', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
    .bind(new.category)
    .bind(new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");
    sqlx::query("INSERT INTO topic (node_id, name, description) VALUES ($1, $2, $3)")
        .bind(node_id)
        .bind(name)
        .bind(new.description)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    get_topic(pool, node_id)
        .await?
        .ok_or_else(|| topic_not_found(node_id))
}

const SELECT_TOPIC: &str =
    "SELECT t.node_id, t.name, t.description, n.project_id, p.name AS project, \
            n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment c WHERE c.node_id = t.node_id) AS comment_count, \
            n.created, n.updated \
     FROM topic t JOIN node n ON n.id = t.node_id \
     LEFT JOIN project p ON p.id = n.project_id";

pub async fn get_topic(pool: &PgPool, node_id: i64) -> Result<Option<Topic>> {
    Ok(
        sqlx::query_as::<_, Topic>(&format!("{SELECT_TOPIC} WHERE t.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

#[derive(Debug, Clone)]
pub struct TopicQuery {
    /// Free-text match against name and description.
    pub q: Option<String>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for TopicQuery {
    fn default() -> Self {
        Self {
            q: None,
            archived: crate::repo::archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Topics, enveloped and bounded (WI #534). `list_topics` and the old
/// `search_topics` collapse into one call: `q` is just another filter.
pub async fn list_topics(pool: &PgPool, query: TopicQuery) -> Result<Page<Topic>> {
    let (limit, offset) = query.page.resolve_public();
    let q = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty());
    const WHERE: &str = "WHERE ($1::bool IS NULL OR n.archived = $1) \
           AND ($2::text IS NULL \
                OR t.name ILIKE '%' || $2 || '%' \
                OR coalesce(t.description, '') ILIKE '%' || $2 || '%')";
    let items = sqlx::query_as::<_, Topic>(&format!(
        "{SELECT_TOPIC} {WHERE} ORDER BY lower(t.name), t.node_id LIMIT $3 OFFSET $4"
    ))
    .bind(query.archived)
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM topic t JOIN node n ON n.id = t.node_id {WHERE}"
    ))
    .bind(query.archived)
    .bind(q)
    .fetch_one(pool)
    .await?;
    Ok(Page::from_parts(items, total, limit, offset))
}

pub async fn update_topic(pool: &PgPool, node_id: i64, patch: TopicPatch) -> Result<Topic> {
    let name = match patch.name.as_deref() {
        Some(value) => Some(validate_name(value)?.to_owned()),
        None => None,
    };
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE topic SET \
             name = CASE WHEN $2 THEN $3 ELSE name END, \
             description = CASE WHEN $4 THEN $5 ELSE description END \
         WHERE node_id = $1",
    )
    .bind(node_id)
    .bind(name.is_some())
    .bind(name)
    .bind(patch.description.is_some())
    .bind(patch.description.flatten())
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Err(topic_not_found(node_id));
    }
    sqlx::query(
        "UPDATE node SET \
             category = CASE WHEN $2 THEN $3 ELSE category END, \
             tags = CASE WHEN $4 THEN $5 ELSE tags END \
         WHERE id = $1 AND kind = 'topic'",
    )
    .bind(node_id)
    .bind(patch.category.is_some())
    .bind(patch.category.flatten())
    .bind(patch.tags.is_some())
    .bind(patch.tags.unwrap_or_default())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    get_topic(pool, node_id)
        .await?
        .ok_or_else(|| topic_not_found(node_id))
}

pub async fn archive_topic(pool: &PgPool, node_id: i64, archived: bool) -> Result<Topic> {
    let result = sqlx::query("UPDATE node SET archived = $2 WHERE id = $1 AND kind = 'topic'")
        .bind(node_id)
        .bind(archived)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(topic_not_found(node_id));
    }
    get_topic(pool, node_id)
        .await?
        .ok_or_else(|| topic_not_found(node_id))
}
