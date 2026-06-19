//! Typed source-model readers for the kwi and kcard snapshot databases.
//!
//! These read the *original* kwi/kcard schemas (as restored from `pg_dump`
//! snapshots) into Rust structs. They drop nothing and resolve kwi's
//! type/status reference tables to their names, so the importer (S5) and the
//! fidelity harness (S6) work against a faithful in-memory view of each source.

use rust_decimal::Decimal;
use sqlx::PgPool;
use time::OffsetDateTime;

// --- kwi (work items) -----------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KwiProject {
    pub id: i32,
    pub project: String,
    pub gh_repo: Option<String>,
    pub cn_path: String,
    pub description: Option<String>,
    pub created: OffsetDateTime,
    pub updated: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KwiArea {
    pub id: i32,
    pub project_id: i32,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KwiWorkitem {
    pub id: i32,
    pub project_id: i32,
    pub area_id: Option<i32>,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub sprint: Option<String>,
    pub title: String,
    pub content: String,
    pub details: Option<String>,
    pub parent_id: Option<i32>,
    pub created: OffsetDateTime,
    pub updated: OffsetDateTime,
    pub archived: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KwiRelated {
    pub id: i32,
    pub left_id: i32,
    pub right_id: i32,
    pub relationship: String,
}

#[derive(Debug, Clone)]
pub struct KwiData {
    pub projects: Vec<KwiProject>,
    pub areas: Vec<KwiArea>,
    pub workitems: Vec<KwiWorkitem>,
    pub related: Vec<KwiRelated>,
}

pub async fn read_kwi(pool: &PgPool) -> Result<KwiData, sqlx::Error> {
    let projects = sqlx::query_as::<_, KwiProject>(
        "SELECT id, project, gh_repo, cn_path, description, created, updated \
         FROM project ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let areas = sqlx::query_as::<_, KwiArea>(
        "SELECT id, project_id, name, description FROM area ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let workitems = sqlx::query_as::<_, KwiWorkitem>(
        "SELECT w.id, w.project_id, w.area_id, \
                t.name AS wi_type, s.name AS wi_status, \
                w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
                w.parent_id, w.created, w.updated, w.archived \
         FROM workitem w \
         JOIN workitem_type   t ON t.id = w.wi_type_id \
         JOIN workitem_status s ON s.id = w.wi_status_id \
         ORDER BY w.id",
    )
    .fetch_all(pool)
    .await?;

    let related = sqlx::query_as::<_, KwiRelated>(
        "SELECT id, left_id, right_id, relationship FROM related ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    Ok(KwiData {
        projects,
        areas,
        workitems,
        related,
    })
}

// --- kcard (kanban) -------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KcardCard {
    pub id: i64,
    pub title: String,
    pub status: String,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub description: String,
    pub archived: bool,
    pub rank: Decimal,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KcardComment {
    pub id: i64,
    pub card_id: i64,
    pub body: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct KcardData {
    pub cards: Vec<KcardCard>,
    pub comments: Vec<KcardComment>,
}

pub async fn read_kcard(pool: &PgPool) -> Result<KcardData, sqlx::Error> {
    let cards = sqlx::query_as::<_, KcardCard>(
        "SELECT id, title, status::text AS status, project, category, tags, \
                description, archived, rank, created_at, updated_at \
         FROM cards ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let comments = sqlx::query_as::<_, KcardComment>(
        "SELECT id, card_id, body, created_at, updated_at \
         FROM comments ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    Ok(KcardData { cards, comments })
}
