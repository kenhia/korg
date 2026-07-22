//! Source-linked daily planning with server-enforced lifecycle boundaries.

use serde::Serialize;
use sqlx::{PgPool, Postgres, Row, Transaction};
use thiserror::Error;
use time::{Date, OffsetDateTime};
use ts_rs::TS;

#[derive(Debug, Clone, Copy)]
pub struct LifecycleContext {
    pub today: Date,
    pub now: OffsetDateTime,
}

#[derive(Debug, Error)]
pub enum PlanningError {
    #[error("source node {0} not found")]
    SourceNotFound(i64),
    #[error("source kind '{kind}' is not plannable for node {node_id}")]
    WrongSource { node_id: i64, kind: String },
    #[error("daily plan item {0} not found")]
    ItemNotFound(i64),
    #[error("past daily plan structure is frozen")]
    FrozenPast,
    #[error("target date must be today or in the future")]
    TargetPast,
    #[error("invalid date range: {0}")]
    InvalidRange(&'static str),
    #[error("reorder must contain every item for the day exactly once")]
    InvalidReorder,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub type Result<T> = std::result::Result<T, PlanningError>;

mod date_str {
    use serde::Serializer;
    use time::macros::format_description;
    use time::Date;

    pub fn serialize<S: Serializer>(date: &Date, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(
            &date
                .format(&format_description!("[year]-[month]-[day]"))
                .map_err(serde::ser::Error::custom)?,
        )
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct DailyPlanItem {
    pub node_id: i64,
    #[serde(with = "date_str")]
    #[ts(type = "string")]
    pub plan_date: Date,
    pub position: i32,
    pub display: String,
    pub source_node_id: i64,
    pub source_kind: String,
    pub source_title: String,
    #[serde(with = "time::serde::rfc3339::option")]
    #[ts(type = "string | null")]
    pub completed_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct History {
    #[serde(with = "date_str")]
    #[ts(type = "string")]
    pub from: Date,
    #[serde(with = "date_str")]
    #[ts(type = "string")]
    pub to: Date,
    pub total: usize,
    pub completed: usize,
    pub completion_rate: f64,
    pub items: Vec<DailyPlanItem>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct MoveOutcome {
    pub node_id: i64,
    pub copied: bool,
}

const SELECT_ITEMS: &str =
    "SELECT d.node_id, d.plan_date, d.position, d.display, d.source_node_id, \
            s.kind AS source_kind, \
            CASE s.kind WHEN 'workitem' THEN w.title WHEN 'card' THEN c.title \
                        WHEN 'topic' THEN t.name END AS source_title, \
            d.completed_at, d.created_at \
     FROM daily_plan_item d JOIN node s ON s.id = d.source_node_id \
     LEFT JOIN workitem w ON w.node_id = s.id \
     LEFT JOIN card c ON c.node_id = s.id \
     LEFT JOIN topic t ON t.node_id = s.id";

async fn resolve_source(
    tx: &mut Transaction<'_, Postgres>,
    source_node_id: i64,
) -> Result<(String, String)> {
    let row = sqlx::query(
        "SELECT n.kind, CASE n.kind \
             WHEN 'workitem' THEN w.title WHEN 'card' THEN c.title WHEN 'topic' THEN t.name END AS title \
         FROM node n LEFT JOIN workitem w ON w.node_id = n.id \
         LEFT JOIN card c ON c.node_id = n.id LEFT JOIN topic t ON t.node_id = n.id \
         WHERE n.id = $1",
    )
    .bind(source_node_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PlanningError::SourceNotFound(source_node_id))?;
    let kind: String = row.get("kind");
    if !matches!(kind.as_str(), "workitem" | "card" | "topic") {
        return Err(PlanningError::WrongSource {
            node_id: source_node_id,
            kind,
        });
    }
    Ok((kind, row.get("title")))
}

pub async fn create_item(
    pool: &PgPool,
    source_node_id: i64,
    plan_date: Date,
    context: &LifecycleContext,
) -> Result<DailyPlanItem> {
    if plan_date < context.today {
        return Err(PlanningError::TargetPast);
    }
    let mut tx = pool.begin().await?;
    let (_, display) = resolve_source(&mut tx, source_node_id).await?;
    let position: i32 = sqlx::query_scalar(
        "SELECT coalesce(max(position) + 1, 0) FROM daily_plan_item WHERE plan_date = $1",
    )
    .bind(plan_date)
    .fetch_one(&mut *tx)
    .await?;
    let node_id: i64 =
        sqlx::query_scalar("INSERT INTO node (kind) VALUES ('daily_plan_item') RETURNING id")
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query(
        "INSERT INTO daily_plan_item (node_id, plan_date, position, display, source_node_id, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(node_id)
    .bind(plan_date)
    .bind(position)
    .bind(display)
    .bind(source_node_id)
    .bind(context.now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    reread(pool, node_id).await
}

/// One planned item by node id — the read every mutation acknowledges with
/// (WI #525).
pub async fn get_item(pool: &PgPool, node_id: i64) -> Result<Option<DailyPlanItem>> {
    Ok(
        sqlx::query_as::<_, DailyPlanItem>(&format!("{SELECT_ITEMS} WHERE d.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

async fn reread(pool: &PgPool, node_id: i64) -> Result<DailyPlanItem> {
    get_item(pool, node_id)
        .await?
        .ok_or(PlanningError::ItemNotFound(node_id))
}

pub async fn list_items(pool: &PgPool, from: Date, to: Date) -> Result<Vec<DailyPlanItem>> {
    if from > to {
        return Err(PlanningError::InvalidRange("from must not be after to"));
    }
    Ok(sqlx::query_as::<_, DailyPlanItem>(&format!(
        "{SELECT_ITEMS} WHERE d.plan_date BETWEEN $1 AND $2 ORDER BY d.plan_date, d.position"
    ))
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?)
}

pub async fn set_completion(
    pool: &PgPool,
    node_id: i64,
    completed: bool,
    context: &LifecycleContext,
) -> Result<DailyPlanItem> {
    let result = sqlx::query(
        "UPDATE daily_plan_item SET completed_at = CASE WHEN $2 THEN $3 ELSE NULL END WHERE node_id = $1",
    )
    .bind(node_id)
    .bind(completed)
    .bind(context.now)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(PlanningError::ItemNotFound(node_id));
    }
    reread(pool, node_id).await
}

async fn item_location(
    tx: &mut Transaction<'_, Postgres>,
    node_id: i64,
) -> Result<(Date, i32, String, i64)> {
    let row = sqlx::query(
        "SELECT plan_date, position, display, source_node_id FROM daily_plan_item \
         WHERE node_id = $1 FOR UPDATE",
    )
    .bind(node_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PlanningError::ItemNotFound(node_id))?;
    Ok((
        row.get("plan_date"),
        row.get("position"),
        row.get("display"),
        row.get("source_node_id"),
    ))
}

pub async fn reorder_day(
    pool: &PgPool,
    plan_date: Date,
    ordered_node_ids: &[i64],
    context: &LifecycleContext,
) -> Result<Vec<DailyPlanItem>> {
    if plan_date < context.today {
        return Err(PlanningError::FrozenPast);
    }
    let mut tx = pool.begin().await?;
    let actual: Vec<i64> = sqlx::query_scalar(
        "SELECT node_id FROM daily_plan_item WHERE plan_date = $1 ORDER BY position FOR UPDATE",
    )
    .bind(plan_date)
    .fetch_all(&mut *tx)
    .await?;
    let mut expected = actual.clone();
    let mut supplied = ordered_node_ids.to_vec();
    expected.sort_unstable();
    supplied.sort_unstable();
    supplied.dedup();
    if expected != supplied || actual.len() != ordered_node_ids.len() {
        return Err(PlanningError::InvalidReorder);
    }
    for (position, node_id) in ordered_node_ids.iter().enumerate() {
        sqlx::query("UPDATE daily_plan_item SET position = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(position as i32)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    list_items(pool, plan_date, plan_date).await
}

pub async fn delete_item(pool: &PgPool, node_id: i64, context: &LifecycleContext) -> Result<()> {
    let mut tx = pool.begin().await?;
    let (plan_date, position, _, _) = item_location(&mut tx, node_id).await?;
    if plan_date < context.today {
        return Err(PlanningError::FrozenPast);
    }
    sqlx::query("DELETE FROM node WHERE id = $1 AND kind = 'daily_plan_item'")
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE daily_plan_item SET position = position - 1 WHERE plan_date = $1 AND position > $2",
    )
    .bind(plan_date)
    .bind(position)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn insert_position(
    tx: &mut Transaction<'_, Postgres>,
    target_date: Date,
    requested: i32,
) -> Result<i32> {
    let target_ids: Vec<i64> =
        sqlx::query_scalar("SELECT node_id FROM daily_plan_item WHERE plan_date = $1 FOR UPDATE")
            .bind(target_date)
            .fetch_all(&mut **tx)
            .await?;
    let position = requested.clamp(0, target_ids.len() as i32);
    sqlx::query(
        "UPDATE daily_plan_item SET position = position + 1 \
         WHERE plan_date = $1 AND position >= $2",
    )
    .bind(target_date)
    .bind(position)
    .execute(&mut **tx)
    .await?;
    Ok(position)
}

pub async fn move_item(
    pool: &PgPool,
    node_id: i64,
    target_date: Date,
    target_position: i32,
    context: &LifecycleContext,
) -> Result<MoveOutcome> {
    if target_date < context.today {
        return Err(PlanningError::TargetPast);
    }
    let mut tx = pool.begin().await?;
    let (source_date, source_position, display, source_node_id) =
        item_location(&mut tx, node_id).await?;

    if source_date < context.today {
        let position = insert_position(&mut tx, target_date, target_position).await?;
        let copied_id: i64 =
            sqlx::query_scalar("INSERT INTO node (kind) VALUES ('daily_plan_item') RETURNING id")
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "INSERT INTO daily_plan_item \
             (node_id, plan_date, position, display, source_node_id, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(copied_id)
        .bind(target_date)
        .bind(position)
        .bind(display)
        .bind(source_node_id)
        .bind(context.now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(MoveOutcome {
            node_id: copied_id,
            copied: true,
        });
    }

    if source_date == target_date {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT node_id FROM daily_plan_item WHERE plan_date = $1 AND node_id <> $2 ORDER BY position",
        )
        .bind(source_date)
        .bind(node_id)
        .fetch_all(&mut *tx)
        .await?;
        let position = target_position.clamp(0, ids.len() as i32) as usize;
        let mut ordered = ids;
        ordered.insert(position, node_id);
        for (position, id) in ordered.iter().enumerate() {
            sqlx::query("UPDATE daily_plan_item SET position = $2 WHERE node_id = $1")
                .bind(id)
                .bind(position as i32)
                .execute(&mut *tx)
                .await?;
        }
    } else {
        sqlx::query(
            "UPDATE daily_plan_item SET position = position - 1 \
             WHERE plan_date = $1 AND position > $2",
        )
        .bind(source_date)
        .bind(source_position)
        .execute(&mut *tx)
        .await?;
        let position = insert_position(&mut tx, target_date, target_position).await?;
        sqlx::query("UPDATE daily_plan_item SET plan_date = $2, position = $3 WHERE node_id = $1")
            .bind(node_id)
            .bind(target_date)
            .bind(position)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(MoveOutcome {
        node_id,
        copied: false,
    })
}

pub async fn history(
    pool: &PgPool,
    from: Date,
    to: Date,
    source_node_id: Option<i64>,
    context: &LifecycleContext,
) -> Result<History> {
    if from > to {
        return Err(PlanningError::InvalidRange("from must not be after to"));
    }
    if to >= context.today {
        return Err(PlanningError::InvalidRange(
            "history end must be before today",
        ));
    }
    let items = sqlx::query_as::<_, DailyPlanItem>(&format!(
        "{SELECT_ITEMS} WHERE d.plan_date BETWEEN $1 AND $2 \
         AND ($3::bigint IS NULL OR d.source_node_id = $3) ORDER BY d.plan_date, d.position"
    ))
    .bind(from)
    .bind(to)
    .bind(source_node_id)
    .fetch_all(pool)
    .await?;
    let total = items.len();
    let completed = items
        .iter()
        .filter(|item| item.completed_at.is_some())
        .count();
    let completion_rate = if total == 0 {
        0.0
    } else {
        completed as f64 / total as f64
    };
    Ok(History {
        from,
        to,
        total,
        completed,
        completion_rate,
        items,
    })
}
