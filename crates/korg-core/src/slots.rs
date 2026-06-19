//! Calendar timebox slots: an editable weekly template and the materialized
//! per-date slots generated from it. Slots are duration-only timeboxes (no
//! fixed time-of-day) that hold a small goal and can be linked to the work
//! item / card they advance.

use anyhow::Result;
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::Date;

/// Serialize a `Date` as an ISO `YYYY-MM-DD` string (time's default emits an
/// ordinal array, which is awkward for the web client).
mod date_str {
    use serde::Serializer;
    use time::macros::format_description;
    use time::Date;

    pub fn serialize<S: Serializer>(d: &Date, s: S) -> Result<S::Ok, S::Error> {
        let fmt = format_description!("[year]-[month]-[day]");
        let out = d.format(&fmt).map_err(serde::ser::Error::custom)?;
        s.serialize_str(&out)
    }
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq)]
pub struct TemplateSlot {
    pub id: i64,
    pub dow: i16,
    pub position: i32,
    pub duration_minutes: i32,
    pub label: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTemplateSlot {
    pub dow: i16,
    pub position: i32,
    pub duration_minutes: i32,
    pub label: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq)]
pub struct Slot {
    pub node_id: i64,
    #[serde(with = "date_str")]
    pub slot_date: Date,
    pub duration_minutes: i32,
    pub label: Option<String>,
    pub goal: Option<String>,
    pub position: i32,
}

pub async fn list_templates(pool: &PgPool) -> Result<Vec<TemplateSlot>> {
    let rows = sqlx::query_as::<_, TemplateSlot>(
        "SELECT id, dow, position, duration_minutes, label \
         FROM slot_template ORDER BY dow, position",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Replace the entire weekly template. This is how the cadence is edited as
/// free time changes.
pub async fn set_weekly_template(pool: &PgPool, slots: &[NewTemplateSlot]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM slot_template").execute(&mut *tx).await?;
    for s in slots {
        sqlx::query(
            "INSERT INTO slot_template (dow, position, duration_minutes, label) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(s.dow)
        .bind(s.position)
        .bind(s.duration_minutes)
        .bind(&s.label)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Materialize slots for `days` consecutive dates starting at `start`, using
/// the current weekly template. Returns the number of slots created.
pub async fn generate_slots(pool: &PgPool, start: Date, days: i64) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let mut created = 0i64;
    let mut date = start;
    for _ in 0..days {
        let dow = date.weekday().number_days_from_sunday() as i16;
        let templates = sqlx::query_as::<_, TemplateSlot>(
            "SELECT id, dow, position, duration_minutes, label \
             FROM slot_template WHERE dow = $1 ORDER BY position",
        )
        .bind(dow)
        .fetch_all(&mut *tx)
        .await?;

        for t in &templates {
            let node_id: i64 =
                sqlx::query("INSERT INTO node (kind) VALUES ('slot') RETURNING id")
                    .fetch_one(&mut *tx)
                    .await?
                    .get("id");
            sqlx::query(
                "INSERT INTO slot \
                 (node_id, slot_date, duration_minutes, label, template_id, position) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(node_id)
            .bind(date)
            .bind(t.duration_minutes)
            .bind(&t.label)
            .bind(t.id)
            .bind(t.position)
            .execute(&mut *tx)
            .await?;
            created += 1;
        }

        date = date.next_day().expect("date overflow");
    }
    tx.commit().await?;
    Ok(created)
}

pub async fn list_slots(pool: &PgPool, from: Date, to: Date) -> Result<Vec<Slot>> {
    let rows = sqlx::query_as::<_, Slot>(
        "SELECT node_id, slot_date, duration_minutes, label, goal, position \
         FROM slot WHERE slot_date BETWEEN $1 AND $2 \
         ORDER BY slot_date, position",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn set_slot_goal(pool: &PgPool, node_id: i64, goal: Option<&str>) -> Result<()> {
    sqlx::query("UPDATE slot SET goal = $2 WHERE node_id = $1")
        .bind(node_id)
        .bind(goal)
        .execute(pool)
        .await?;
    Ok(())
}
