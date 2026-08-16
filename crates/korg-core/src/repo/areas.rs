//! Areas — the optional second axis under a project, unique per `(project, name)`.

use anyhow::Result;
use serde::Serialize;
use sqlx::{PgPool, Row};
use ts_rs::TS;

use crate::error::RepoError;

use super::common::require_non_empty;

// --- areas ----------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct AreaRow {
    pub id: i64,
    pub name: String,
    /// What the area is for. `create_area` has accepted and idempotently
    /// updated this since 0001, but no read returned it (WI #889), so the
    /// contract it documents was unverifiable from the surface offering it.
    pub description: Option<String>,
}

pub async fn list_areas(pool: &PgPool, project: &str) -> Result<Vec<AreaRow>> {
    let rows = sqlx::query_as::<_, AreaRow>(
        "SELECT a.id, a.name, a.description FROM area a \
         JOIN project p ON p.id = a.project_id \
         WHERE p.name = $1 ORDER BY a.name",
    )
    .bind(project)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Rename and/or re-describe an area (WI #889). An area is a label, not a
/// record with a history — `workitem.area_id` keeps pointing at the same row,
/// so a rename is safe and needs no fixup.
pub async fn update_area(
    pool: &PgPool,
    project: &str,
    name: &str,
    new_name: Option<&str>,
    description: Option<Option<&str>>,
) -> Result<AreaRow> {
    if let Some(n) = new_name {
        require_non_empty(n, "area name")?;
    }
    let id = area_row_id(pool, project, name).await?;
    if let Some(n) = new_name {
        let clash: Option<i64> = sqlx::query_scalar(
            "SELECT a.id FROM area a JOIN project p ON p.id = a.project_id \
             WHERE p.name = $1 AND a.name = $2 AND a.id <> $3",
        )
        .bind(project)
        .bind(n)
        .bind(id)
        .fetch_optional(pool)
        .await?;
        if clash.is_some() {
            return Err(RepoError::Conflict(format!(
                "project '{project}' already has an area named '{n}'"
            ))
            .into());
        }
        sqlx::query("UPDATE area SET name = $2 WHERE id = $1")
            .bind(id)
            .bind(n)
            .execute(pool)
            .await?;
    }
    if let Some(d) = description {
        sqlx::query("UPDATE area SET description = $2 WHERE id = $1")
            .bind(id)
            .bind(d)
            .execute(pool)
            .await?;
    }
    let row = sqlx::query_as::<_, AreaRow>("SELECT id, name, description FROM area WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(row)
}

/// Delete an area (WI #889) — refusing while work items are still filed under
/// it. Areas carry no history of their own, so delete rather than archive is
/// their whole lifecycle end; the refusal is what stops the schema's
/// `ON DELETE SET NULL` from silently unfiling every item instead.
pub async fn delete_area(pool: &PgPool, project: &str, name: &str) -> Result<bool> {
    let id = match area_row_id(pool, project, name).await {
        Ok(id) => id,
        Err(e) if matches!(e.downcast_ref::<RepoError>(), Some(RepoError::NotFound(_))) => {
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let filed: i64 = sqlx::query_scalar("SELECT count(*) FROM workitem WHERE area_id = $1")
        .bind(id)
        .fetch_one(pool)
        .await?;
    if filed > 0 {
        return Err(RepoError::Conflict(format!(
            "area '{name}' still has {filed} work item(s) filed under it — move \
             them off it first; deleting would silently unfile them"
        ))
        .into());
    }
    sqlx::query("DELETE FROM area WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(true)
}

async fn area_row_id(pool: &PgPool, project: &str, name: &str) -> Result<i64> {
    sqlx::query_scalar(
        "SELECT a.id FROM area a JOIN project p ON p.id = a.project_id \
         WHERE p.name = $1 AND a.name = $2",
    )
    .bind(project)
    .bind(name)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| RepoError::NotFound(format!("no area '{name}' in project '{project}'")).into())
}

/// Create (or return existing) an area under a project by name.
pub async fn create_area(
    pool: &PgPool,
    project: &str,
    name: &str,
    description: Option<&str>,
) -> Result<i64> {
    let pid: i64 = sqlx::query_scalar("SELECT id FROM project WHERE name = $1")
        .bind(project)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no project named '{project}'")))?;
    let id: i64 = sqlx::query(
        "INSERT INTO area (project_id, name, description) VALUES ($1, $2, $3) \
         ON CONFLICT (project_id, name) DO UPDATE SET description = EXCLUDED.description \
         RETURNING id",
    )
    .bind(pid)
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}
