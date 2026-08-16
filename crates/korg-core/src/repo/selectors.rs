//! Name-or-id selectors (WI #575)
//!
//! Every write that targets a project used to take a bare `project_id`, so an
//! agent that didn't already hold the id had to guess — and a wrong guess was a
//! *silent wrong write*, not an error. A work item filed with `project_id: 1`
//! landed in an archived project and reported success.
//!
//! Operations now accept either the id or the name, resolved here, in core, so
//! both transports get identical behaviour. Three rules, and the reasons matter:
//!
//! 1. **Never both.** Passing `project_id` and `project` together is
//!    `invalid_input`, not a precedence rule. A precedence rule silently
//!    discards one of two things the caller explicitly asked for, which is the
//!    very failure this change exists to remove.
//! 2. **Resolve, never create.** An unknown name is an error. WI #537 removed
//!    project-name acceptance from `update_card` precisely because it *created*
//!    the project as a side effect of a card edit; that stays removed. Creating
//!    a project is `create_project`'s job and nothing else's.
//! 3. **Say what to do next.** An unresolvable name names `list_projects` as
//!    the remedy — the same principle as `vocab::validate`, where the error
//!    doubles as the documentation needed to retry.

use anyhow::Result;
use sqlx::{Executor, PgPool, Postgres};

use crate::error::RepoError;
use crate::relationships;
use crate::vocab::PROJECT_STATUS_ACTIVE;

/// Both halves of a selector were supplied. Which one did the caller mean?
/// korg refuses to guess.
fn selector_conflict(id_field: &str, name_field: &str) -> anyhow::Error {
    RepoError::InvalidInput(format!("pass either {id_field} or {name_field}, not both")).into()
}

/// A name that didn't resolve, with the remedy attached.
///
/// The only suggestion offered is a case-insensitive exact match, which is the
/// realistic near-miss (`KORG` for `korg`). Deliberately no fuzzy matching: a
/// confidently wrong "did you mean…" would invite exactly the misfile this
/// whole change is about, and `list_projects` is one call away.
async fn unknown_project(pool: &PgPool, name: &str) -> anyhow::Error {
    let suggestion: Option<String> =
        sqlx::query_scalar("SELECT name FROM project WHERE lower(name) = lower($1)")
            .bind(name)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    let hint = match suggestion {
        Some(actual) => format!(" — did you mean '{actual}'?"),
        None => " — call list_projects (GET /api/projects) for the available names".into(),
    };
    RepoError::InvalidInput(format!("no project named '{name}'{hint}")).into()
}

/// An unregistered relationship label (D-11). The error names the whole
/// registry and, when there is an obvious near-miss, suggests it — the
/// sprint-017 principle that the error doubles as the retry instructions.
///
/// A "did you mean" is safe here where it is not for open project names: the
/// registry is a closed, five-entry set, so every suggestion is a real label.
/// The near-miss is case-insensitive exact, then separator-insensitive exact,
/// then a prefix overlap (`related` -> `related-to`); anything further is left
/// to the named vocabulary rather than guessed at.
///
/// The separator pass is WI #890. The registry spells one label `depends_on`
/// and another `related-to`, so which separator a label uses is unguessable
/// from the outside — and `depends-on` returned the same bare registry listing
/// as a wild miss like `blocks`, making the description's promise of a
/// near-miss false for the one confusion the vocabulary invites by
/// construction. Normalizing `-` and `_` to the same character is not fuzzy
/// matching: it can only ever match a label that differs in nothing else.
pub(super) fn unknown_label(label: &str) -> anyhow::Error {
    /// `-` and `_` are interchangeable noise for matching purposes only.
    fn separator_blind(s: &str) -> String {
        s.to_ascii_lowercase().replace('-', "_")
    }
    let registered: Vec<&str> = relationships::REGISTRY.iter().map(|s| s.label).collect();
    let lower = label.to_ascii_lowercase();
    let blind = separator_blind(label);
    let suggestion = registered
        .iter()
        .find(|l| l.eq_ignore_ascii_case(label))
        .or_else(|| registered.iter().find(|l| separator_blind(l) == blind))
        .or_else(|| {
            registered
                .iter()
                .find(|l| l.starts_with(lower.as_str()) || lower.starts_with(**l))
        })
        .copied();
    let hint = match suggestion {
        Some(s) => format!("; did you mean '{s}'?"),
        None => String::new(),
    };
    RepoError::InvalidInput(format!(
        "unknown label '{label}'; registered labels are {}{hint}",
        registered.join(", ")
    ))
    .into()
}

/// A project that exists but cannot take work (WI #884).
///
/// The MCP instructions call the active roster "the only valid targets for new
/// work", and `unknown_project` promises a name that doesn't resolve fails
/// "rather than mis-filing". A known-but-archived name resolving silently broke
/// both promises in the one direction they exist to prevent: the work lands,
/// somewhere nobody is looking. Naming the status is the point — an agent that
/// picked the name off a stale list needs to know it is *retired*, not
/// misspelled, because the remedy is a different project rather than a
/// different spelling.
fn archived_project_target(name: &str, status: &str) -> anyhow::Error {
    RepoError::InvalidInput(format!(
        "project '{name}' is {status} and cannot take new work — call list_projects \
         (GET /api/projects) for the active projects, whose work this probably belongs to"
    ))
    .into()
}
/// Look a project up by name, refusing one that can't take work.
async fn project_id_for_name(pool: &PgPool, name: &str) -> Result<i64> {
    let found: Option<(i64, String)> =
        sqlx::query_as("SELECT id, status FROM project WHERE name = $1")
            .bind(name)
            .fetch_optional(pool)
            .await?;
    match found {
        Some((id, status)) if status == PROJECT_STATUS_ACTIVE => Ok(id),
        Some((_, status)) => Err(archived_project_target(name, &status)),
        None => Err(unknown_project(pool, name).await),
    }
}

/// Confirm a project id exists and can take work. Without the existence half a
/// typo'd id reached the FK and came back as a raw Postgres error in a 500 —
/// the same shape WI #524 fixed for `relate`'s endpoints; the status half is
/// #884, so that `project_id` cannot route around the name path's refusal.
async fn require_project(pool: &PgPool, id: i64) -> Result<()> {
    let found: Option<(String, String)> =
        sqlx::query_as("SELECT name, status FROM project WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    match found {
        Some((_, status)) if status == PROJECT_STATUS_ACTIVE => Ok(()),
        Some((name, status)) => Err(archived_project_target(&name, &status)),
        None => Err(RepoError::InvalidInput(format!(
            "no project with id {id} — call list_projects (GET /api/projects) for the available projects"
        ))
        .into()),
    }
}

/// Resolve a create-time project selector: id, name, or neither.
///
/// Every create that takes a project funnels through here — work items, cards,
/// links, proposals, handoffs — which is why #884's status check lives
/// in the two resolvers rather than in six call sites. `update_project` is
/// deliberately *not* one of them: setting `status` is how a project comes back.
pub(crate) async fn resolve_project(
    pool: &PgPool,
    id: Option<i64>,
    name: Option<&str>,
) -> Result<Option<i64>> {
    match (id, name) {
        (Some(_), Some(_)) => Err(selector_conflict("project_id", "project")),
        (Some(id), None) => {
            require_project(pool, id).await?;
            Ok(Some(id))
        }
        (None, Some(name)) => Ok(Some(project_id_for_name(pool, name).await?)),
        (None, None) => Ok(None),
    }
}

/// Resolve a patch-time project selector, where the outer `Option` is
/// "mentioned at all" and the inner one is "unassign".
///
/// Shares [`resolve_project`]'s archived refusal: moving an item *into* a
/// retired project is the same mis-file as creating it there. Moving one *out*
/// stays legal — that is the remedy, not the offence — because the check reads
/// the target, never the current project.
pub(crate) async fn resolve_project_patch(
    pool: &PgPool,
    id: Option<Option<i64>>,
    name: Option<Option<String>>,
) -> Result<Option<Option<i64>>> {
    match (id, name) {
        (Some(_), Some(_)) => Err(selector_conflict("project_id", "project")),
        (Some(id), None) => match id {
            Some(id) => {
                require_project(pool, id).await?;
                Ok(Some(Some(id)))
            }
            None => Ok(Some(None)),
        },
        (None, Some(name)) => match name {
            Some(name) => Ok(Some(Some(project_id_for_name(pool, &name).await?))),
            None => Ok(Some(None)),
        },
        (None, None) => Ok(None),
    }
}

/// A project's name from its id. The inverse of [`project_id_for_name`], for
/// the paths that resolved an id but have to *quote* the project — sprint 043's
/// cross-project refusal names both projects, and an id would name neither.
pub(super) async fn project_name_for_id(pool: &PgPool, id: i64) -> Result<String> {
    sqlx::query_scalar::<_, String>("SELECT name FROM project WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no project with id {id}")).into())
}

/// Look an area up by name within its project. Areas are unique per
/// `(project_id, name)`, so a name is only meaningful once the project is
/// known — which is why an area name without a project is a specific error
/// rather than a lookup that mysteriously finds nothing.
async fn area_id_for_name<'e, E>(executor: E, project_id: Option<i64>, name: &str) -> Result<i64>
where
    E: Executor<'e, Database = Postgres>,
{
    let Some(project_id) = project_id else {
        return Err(RepoError::InvalidInput(format!(
            "cannot resolve area '{name}' without a project — pass project or project_id too"
        ))
        .into());
    };
    let id: Option<i64> =
        sqlx::query_scalar("SELECT id FROM area WHERE project_id = $1 AND name = $2")
            .bind(project_id)
            .bind(name)
            .fetch_optional(executor)
            .await?;
    id.ok_or_else(|| {
        RepoError::InvalidInput(format!(
            "no area named '{name}' in that project — call list_areas for the available names"
        ))
        .into()
    })
}

/// Resolve a create-time area selector against an already-resolved project.
pub(super) async fn resolve_area<'e, E>(
    executor: E,
    project_id: Option<i64>,
    id: Option<i64>,
    name: Option<&str>,
) -> Result<Option<i64>>
where
    E: Executor<'e, Database = Postgres>,
{
    match (id, name) {
        (Some(_), Some(_)) => Err(selector_conflict("area_id", "area")),
        (Some(id), None) => Ok(Some(id)),
        (None, Some(name)) => Ok(Some(area_id_for_name(executor, project_id, name).await?)),
        (None, None) => Ok(None),
    }
}

/// Resolve a patch-time area selector against the project the work item will
/// have *after* the update.
pub(super) async fn resolve_area_patch<'e, E>(
    executor: E,
    project_id: Option<i64>,
    id: Option<Option<i64>>,
    name: Option<Option<String>>,
) -> Result<Option<Option<i64>>>
where
    E: Executor<'e, Database = Postgres>,
{
    match (id, name) {
        (Some(_), Some(_)) => Err(selector_conflict("area_id", "area")),
        (Some(id), None) => Ok(Some(id)),
        (None, Some(name)) => match name {
            Some(name) => Ok(Some(Some(
                area_id_for_name(executor, project_id, &name).await?,
            ))),
            None => Ok(Some(None)),
        },
        (None, None) => Ok(None),
    }
}
