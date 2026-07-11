//! Reusable planning topics backed by first-class nodes.

use anyhow::{bail, Result};
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;

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
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated: OffsetDateTime,
}

fn validate_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        bail!("topic name is required");
    }
    Ok(name)
}

pub async fn create_topic(pool: &PgPool, new: NewTopic) -> Result<i64> {
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
    Ok(node_id)
}

const SELECT_TOPIC: &str =
    "SELECT t.node_id, t.name, t.description, n.project_id, p.name AS project, \
            n.category, n.tags, n.archived, n.created, n.updated \
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

pub async fn list_topics(pool: &PgPool) -> Result<Vec<Topic>> {
    Ok(sqlx::query_as::<_, Topic>(&format!(
        "{SELECT_TOPIC} WHERE NOT n.archived ORDER BY lower(t.name), t.node_id"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn search_topics(pool: &PgPool, query: &str) -> Result<Vec<Topic>> {
    Ok(sqlx::query_as::<_, Topic>(&format!(
        "{SELECT_TOPIC} WHERE NOT n.archived \
         AND (t.name ILIKE '%' || $1 || '%' OR coalesce(t.description, '') ILIKE '%' || $1 || '%') \
         ORDER BY lower(t.name), t.node_id"
    ))
    .bind(query.trim())
    .fetch_all(pool)
    .await?)
}

pub async fn update_topic(pool: &PgPool, node_id: i64, patch: TopicPatch) -> Result<()> {
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
        bail!("topic {node_id} not found");
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
    Ok(())
}

pub async fn archive_topic(pool: &PgPool, node_id: i64, archived: bool) -> Result<()> {
    let result = sqlx::query("UPDATE node SET archived = $2 WHERE id = $1 AND kind = 'topic'")
        .bind(node_id)
        .bind(archived)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        bail!("topic {node_id} not found");
    }
    Ok(())
}
