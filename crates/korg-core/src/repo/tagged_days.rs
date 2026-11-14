//! Tagged days — Ken's days off and special days, for kpidash's calendar card
//! (WI 4664, migration 0039).
//!
//! A table, not a node kind; see the migration for why. This module stores and
//! lists the tags and nothing else: holidays are computed by rule and the feed
//! record is assembled by the publisher (`calfeed/`), which reads
//! `GET /api/tagged-days`.

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};

use crate::error::RepoError;
use crate::ops::schema;
use crate::vocab::TAGGED_DAY_KINDS;

use super::common::{report_date_fmt, validate_status};

/// The feed contract's bounds (kdashdata CD-30), mirrored by 0039's CHECKs so
/// a stored row can never produce an entry the panel's reader would skip.
pub const TAGGED_DAY_LABEL_MAX: usize = 31;
pub const TAGGED_DAY_SHORT_MAX: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow, Serialize)]
pub struct TaggedDayRow {
    pub id: i64,
    /// `YYYY-MM-DD`, America/Los_Angeles. For a `yearly` row, the first year it
    /// applies in; its month and day repeat every year after.
    #[serde(with = "report_date_fmt")]
    pub date: Date,
    pub yearly: bool,
    /// One of [`TAGGED_DAY_KINDS`].
    pub kind: String,
    /// A special day's name; `None` on a day off.
    pub label: Option<String>,
    /// A special day's tight-space label; `None` when none was given.
    pub short: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
}

/// `add_tagged_day`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewTaggedDay {
    /// The date, `YYYY-MM-DD`, in Ken's local calendar (America/Los_Angeles).
    /// For a `yearly` tag, any year's date works; it repeats from that year on.
    #[serde(with = "report_date_fmt")]
    #[schemars(schema_with = "schema::report_date")]
    pub date: Date,
    /// `off` (a personal day off) or `special` (a named day such as a birthday).
    #[schemars(schema_with = "schema::tagged_day_kind")]
    pub kind: String,
    /// Required for `special`, refused for `off`: the day's name, at most 31
    /// characters ("Sam's birthday").
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty_opt")]
    pub label: Option<String>,
    /// Optional for `special`, refused for `off`: a label for tight spaces, at
    /// most 12 characters ("Sam").
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty_opt")]
    pub short: Option<String>,
    /// Repeat on this month and day every year (birthdays, anniversaries).
    #[serde(default)]
    pub yearly: bool,
}

/// `remove_tagged_day` — by `id`, or by `date` (optionally narrowed by `kind`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RemoveTaggedDay {
    /// One row, as `list_tagged_days` returned it.
    #[serde(default)]
    pub id: Option<i64>,
    /// Every tag on this date, `YYYY-MM-DD` — including a yearly tag whose
    /// month and day it is. Pass `kind` to remove only the day off, or only
    /// the special day.
    #[serde(default, with = "report_date_fmt::option")]
    #[schemars(schema_with = "schema::optional_date")]
    pub date: Option<Date>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tagged_day_kind_filter")]
    pub kind: Option<String>,
}

const COLUMNS: &str = "id, day AS date, yearly, kind, label, short, created";

/// Every tag, by date.
pub async fn list_tagged_days(pool: &PgPool) -> Result<Vec<TaggedDayRow>> {
    Ok(sqlx::query_as::<_, TaggedDayRow>(&format!(
        "SELECT {COLUMNS} FROM tagged_day ORDER BY day, kind, id"
    ))
    .fetch_all(pool)
    .await?)
}

/// Add one tag. The same tag twice is `conflict`, naming the row that exists.
pub async fn add_tagged_day(pool: &PgPool, new: NewTaggedDay) -> Result<TaggedDayRow> {
    validate_status(&new.kind, &TAGGED_DAY_KINDS, "kind")?;
    let label = trimmed(new.label);
    let short = trimmed(new.short);
    if new.kind == "off" {
        if label.is_some() || short.is_some() {
            return Err(RepoError::invalid(
                "a day off carries no `label` or `short`; tag the date a second time with \
                 kind `special` to name it",
            )
            .into());
        }
    } else {
        let Some(l) = &label else {
            return Err(RepoError::invalid("a `special` day needs a `label`").into());
        };
        bounded("label", l, TAGGED_DAY_LABEL_MAX)?;
    }
    if let Some(s) = &short {
        bounded("short", s, TAGGED_DAY_SHORT_MAX)?;
    }

    let inserted = sqlx::query_as::<_, TaggedDayRow>(&format!(
        "INSERT INTO tagged_day (day, yearly, kind, label, short) VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT DO NOTHING RETURNING {COLUMNS}"
    ))
    .bind(new.date)
    .bind(new.yearly)
    .bind(&new.kind)
    .bind(&label)
    .bind(&short)
    .fetch_optional(pool)
    .await?;
    match inserted {
        Some(row) => Ok(row),
        None => {
            let id: i64 = sqlx::query_scalar(
                "SELECT id FROM tagged_day WHERE kind = $1 AND day = $2 AND yearly = $3 \
                 AND COALESCE(label, '') = COALESCE($4, '')",
            )
            .bind(&new.kind)
            .bind(new.date)
            .bind(new.yearly)
            .bind(&label)
            .fetch_one(pool)
            .await?;
            Err(RepoError::Conflict(format!("that tag already exists as id {id}")).into())
        }
    }
}

/// Remove by id, or every tag on a date (narrowed by `kind`). Returns what was
/// removed; removing nothing is `not_found`, so a mistyped date is not a
/// silent success.
pub async fn remove_tagged_day(pool: &PgPool, sel: RemoveTaggedDay) -> Result<Vec<TaggedDayRow>> {
    if let Some(k) = &sel.kind {
        validate_status(k, &TAGGED_DAY_KINDS, "kind")?;
    }
    let removed = match (sel.id, sel.date) {
        (Some(id), None) => {
            if sel.kind.is_some() {
                return Err(RepoError::invalid("`kind` narrows a `date`, not an `id`").into());
            }
            sqlx::query_as::<_, TaggedDayRow>(&format!(
                "DELETE FROM tagged_day WHERE id = $1 RETURNING {COLUMNS}"
            ))
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        (None, Some(date)) => {
            sqlx::query_as::<_, TaggedDayRow>(&format!(
                "DELETE FROM tagged_day \
                 WHERE ($2::text IS NULL OR kind = $2) \
                   AND (day = $1 OR (yearly \
                        AND EXTRACT(MONTH FROM day) = EXTRACT(MONTH FROM $1::date) \
                        AND EXTRACT(DAY FROM day) = EXTRACT(DAY FROM $1::date) \
                        AND day <= $1)) \
                 RETURNING {COLUMNS}"
            ))
            .bind(date)
            .bind(&sel.kind)
            .fetch_all(pool)
            .await?
        }
        _ => {
            return Err(RepoError::invalid("pass exactly one of `id` or `date`").into());
        }
    };
    if removed.is_empty() {
        let what = match (sel.id, sel.date) {
            (Some(id), _) => format!("no tagged day with id {id}"),
            (_, Some(d)) => format!(
                "no {}tag on {d}",
                sel.kind.map(|k| format!("`{k}` ")).unwrap_or_default()
            ),
            _ => unreachable!(),
        };
        return Err(RepoError::not_found(what).into());
    }
    Ok(removed)
}

fn trimmed(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn bounded(field: &str, value: &str, max: usize) -> Result<()> {
    let n = value.chars().count();
    if n > max {
        return Err(RepoError::invalid(format!(
            "`{field}` is {n} characters; the calendar feed allows at most {max}"
        ))
        .into());
    }
    Ok(())
}
