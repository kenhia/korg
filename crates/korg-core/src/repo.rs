//! korg-core repository layer: typed creation of nodes (work items, cards,
//! reading-list links) and generalized cross-kind relationships.
//!
//! Every entity is a `node`; kind-specific data lives in a detail table; any
//! two nodes can be linked through a single `relationship` edge regardless of
//! kind. This is the API the MCP/CLI/web surfaces (M4/M5) build on.

use anyhow::Result;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};

// --- work items -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewWorkItem {
    pub project_id: Option<i64>,
    pub area_id: Option<i64>,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub sprint: Option<String>,
    pub title: String,
    pub content: String,
    pub details: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkItemRef {
    pub node_id: i64,
    pub wi_number: i64,
}

pub async fn create_work_item(pool: &PgPool, new: NewWorkItem) -> Result<WorkItemRef> {
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('workitem', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    let wi_number: i64 = sqlx::query(
        "INSERT INTO workitem \
         (node_id, area_id, wi_type, wi_status, wi_tshirt, sprint, title, content, details) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING wi_number",
    )
    .bind(node_id)
    .bind(new.area_id)
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
    Ok(WorkItemRef { node_id, wi_number })
}

// --- cards ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewCard {
    pub project_id: Option<i64>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub status: String,
    pub title: String,
    pub description: String,
    pub rank: Decimal,
}

pub async fn create_card(pool: &PgPool, new: NewCard) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('card', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
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
    Ok(node_id)
}

// --- reading-list links ---------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewLink {
    pub project_id: Option<i64>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LinkRow {
    pub node_id: i64,
    pub url: String,
    pub title: Option<String>,
    pub read: bool,
}

pub async fn create_link(pool: &PgPool, new: NewLink) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('link', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
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
    Ok(node_id)
}

pub async fn list_links(pool: &PgPool) -> Result<Vec<LinkRow>> {
    let rows = sqlx::query_as::<_, LinkRow>(
        "SELECT node_id, url, title, read FROM link ORDER BY node_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn mark_link_read(pool: &PgPool, node_id: i64, read: bool) -> Result<()> {
    sqlx::query("UPDATE link SET read = $2 WHERE node_id = $1")
        .bind(node_id)
        .bind(read)
        .execute(pool)
        .await?;
    Ok(())
}

// --- generalized relationships --------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, PartialEq, Eq)]
pub struct Neighbor {
    pub node_id: i64,
    pub kind: String,
    pub label: String,
}

pub async fn relate(pool: &PgPool, left: i64, right: i64, label: &str) -> Result<i64> {
    let id: i64 = sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(left)
    .bind(right)
    .bind(label)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// Undirected neighbors of `node`: the node on the other end of each edge,
/// with that node's kind and the relationship label. Works across kinds.
pub async fn neighbors(pool: &PgPool, node: i64) -> Result<Vec<Neighbor>> {
    let rows = sqlx::query_as::<_, Neighbor>(
        "SELECT n.id AS node_id, n.kind, r.relationship AS label \
         FROM relationship r \
         JOIN node n \
           ON n.id = CASE WHEN r.left_id = $1 THEN r.right_id ELSE r.left_id END \
         WHERE r.left_id = $1 OR r.right_id = $1 \
         ORDER BY n.id",
    )
    .bind(node)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn unrelate(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM relationship WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
