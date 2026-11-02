//! Machines — the per-host home directory a `~/` `src_path` expands against
//! (WI 2919, migration 0037).
//!
//! Two columns and no more on purpose; see the migration for what was left out
//! and why. `project.machines` stays a `TEXT[]` and does not reference this
//! table, so a host can be named before anyone describes it.

use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::RepoError;
use crate::ops;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow, Serialize)]
pub struct MachineRow {
    /// A `project.machines` entry, spelled the same way.
    pub name: String,
    /// What `~` expands to on this host. `None` means the host does not expand
    /// `~` at all (Windows hosts, whose clones use 0036's drive-root form).
    pub home: Option<String>,
}

/// `PUT /api/machines/:name` — the whole row. `home` is required in the body
/// and may be `null`, because "this host does not expand `~`" is a statement
/// the caller has to make, not a default it can fall into by omission.
#[derive(Debug, Clone, Deserialize)]
pub struct MachinePut {
    #[serde(default, deserialize_with = "ops::double_option")]
    pub home: Option<Option<String>>,
}

/// Every described host, by name.
pub async fn list_machines(pool: &PgPool) -> Result<Vec<MachineRow>> {
    Ok(
        sqlx::query_as::<_, MachineRow>("SELECT name, home FROM machine ORDER BY name")
            .fetch_all(pool)
            .await?,
    )
}

/// The lookup the connector wants: host name → home (`None` = no expansion).
/// A host absent from the map has no row, which is a different answer.
pub async fn machine_homes(pool: &PgPool) -> Result<HashMap<String, Option<String>>> {
    Ok(list_machines(pool)
        .await?
        .into_iter()
        .map(|m| (m.name, m.home))
        .collect())
}

/// Create or replace one host's row.
///
/// Validated here as well as by 0037's CHECKs so a bad value is
/// `invalid_input` naming the rule, not a 500 carrying Postgres's text.
pub async fn set_machine(pool: &PgPool, name: &str, put: MachinePut) -> Result<MachineRow> {
    let Some(home) = put.home else {
        return Err(RepoError::invalid(
            "`home` is required: an absolute path, or null for a host that does not expand `~`",
        )
        .into());
    };
    validate_name(name)?;
    if let Some(h) = &home {
        validate_home(h)?;
    }
    Ok(sqlx::query_as::<_, MachineRow>(
        "INSERT INTO machine (name, home) VALUES ($1, $2) \
         ON CONFLICT (name) DO UPDATE SET home = EXCLUDED.home \
         RETURNING name, home",
    )
    .bind(name)
    .bind(home)
    .fetch_one(pool)
    .await?)
}

fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(RepoError::invalid(format!(
            "machine name {name:?} must be a lowercase host name, as `project.machines` spells it"
        ))
        .into())
    }
}

fn validate_home(home: &str) -> Result<()> {
    let ok = home.len() > 1
        && home.starts_with('/')
        && !home.ends_with('/')
        && !home.chars().any(char::is_whitespace);
    if ok {
        Ok(())
    } else {
        Err(RepoError::invalid(format!(
            "home {home:?} must be an absolute path with no trailing slash or whitespace, e.g. /home/ken"
        ))
        .into())
    }
}
