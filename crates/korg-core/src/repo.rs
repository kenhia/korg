//! korg-core repository layer: typed creation of nodes (work items, cards,
//! reading-list links) and generalized cross-kind relationships.
//!
//! Every entity is a `node`; kind-specific data lives in a detail table; any
//! two nodes can be linked through a single `relationship` edge regardless of
//! kind. This is the API the MCP/CLI/web surfaces (M4/M5) build on.

use anyhow::Result;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, PgPool, Postgres, Row, Transaction};
use time::OffsetDateTime;
use ts_rs::TS;

pub use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::relationships;
use crate::vocab::{
    self, CARD_STATUSES, LINK_DISPOSITIONS, PROGRAM_LIVE_STATUSES, PROGRAM_STATUSES,
    PROJECT_STATUS_ACTIVE, PROPOSAL_LIVE_STATUSES, PROPOSAL_STATUSES, REPORT_STATUSES,
    WI_LIVE_STATUSES,
};
pub use crate::vocab::{PROJECT_CATEGORIES, PROJECT_STATUSES, WI_STATUSES};

fn validate_status(value: &str, allowed: &[&str], what: &str) -> Result<()> {
    Ok(vocab::validate(value, allowed, what)?)
}

/// Reject a blank value before the database does (WI #551).
///
/// `comment.body` and `link.url` carry `CHECK (btrim(...) <> '')` constraints
/// from 0001/0002. They worked — nothing blank was ever stored — but the
/// failure arrived as an `sqlx::Error`, which classifies as `internal`, so a
/// caller who sent an empty string was told korg had a problem and shown
/// `error returned from database: new row for relation "comment" violates
/// check constraint "comment_body_nonempty"`. Since sprint 019 the web client
/// renders `internal` as an apology and a retry suggestion, which is precisely
/// the wrong advice for input that will never be accepted.
///
/// The CHECK constraints stay: this is the polite front door, not a
/// replacement for the guarantee.
fn require_non_empty(value: &str, what: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(RepoError::invalid(format!("{what} must not be empty")).into());
    }
    Ok(())
}

/// Every mutation starts here (WI #525): the target must exist *and* be the
/// kind the operation is about. Without the kind half, `update_card` against a
/// work item's node id silently archived the work item and reported success —
/// exactly the slip an agent makes now that `wi_number == node_id`.
async fn require_kind<'e, E>(executor: E, node_id: i64, kind: &str, what: &str) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    let found: Option<String> = sqlx::query_scalar("SELECT kind FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(executor)
        .await?;
    match found.as_deref() {
        Some(k) if k == kind => Ok(()),
        _ => Err(RepoError::NotFound(format!("no {what} with node_id {node_id}")).into()),
    }
}

/// Existence check for operations that legitimately span kinds (comments,
/// relationships, tags).
async fn require_node<'e, E>(executor: E, node_id: i64) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(executor)
        .await?;
    exists
        .map(|_| ())
        .ok_or_else(|| RepoError::NotFound(format!("no node with id {node_id}")).into())
}

/// The work item that carries the program layer — the sanctioned way to bundle
/// work across projects. Quoted by [`cross_project_covers`] so a refused edge
/// teaches the workflow instead of only blocking it (proposal korg:971).
const PROGRAM_LAYER_WI: i64 = 968;

/// The refusal for a `covers` edge whose two ends are in different projects
/// (sprint 043, #967).
///
/// Named projects on both sides, the offending work item by its `wi_number`
/// (the handle the caller passed, not a node id), and the program layer as the
/// alternative. Shared by [`relate`] and [`create_proposal`], which insert
/// `covers` by two different paths and must refuse identically.
fn cross_project_covers(
    proposal_project: &str,
    wi_number: i64,
    wi_project: &str,
    wi_title: &str,
) -> anyhow::Error {
    RepoError::InvalidInput(format!(
        "a sprint proposal covers work in one project only: this proposal is in \
         '{proposal_project}' but #{wi_number} ({wi_title}) is in '{wi_project}'. \
         Either move #{wi_number} into '{proposal_project}', or propose it separately \
         in '{wi_project}' — cross-project work is bundled by a program, the layer \
         above proposals (korg #{PROGRAM_LAYER_WI}), never by widening one proposal."
    ))
    .into()
}

/// The project name of a node, and whether the node exists at all.
///
/// `Ok(None)` means the node has no project — unfiled, which is not the same as
/// filed elsewhere and is deliberately not a `covers` violation.
async fn node_project<'e, E>(executor: E, node_id: i64) -> Result<Option<String>>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: Option<Option<String>> = sqlx::query_scalar(
        "SELECT p.name FROM node n LEFT JOIN project p ON p.id = n.project_id WHERE n.id = $1",
    )
    .bind(node_id)
    .fetch_optional(executor)
    .await?;
    row.ok_or_else(|| RepoError::NotFound(format!("no node with id {node_id}")).into())
}

/// A work item's user-facing handle — `wi_number` and title — for an error
/// message. Only called once the caller knows the node *is* a work item.
async fn wi_handle<'e, E>(executor: E, node_id: i64) -> Result<(i64, String)>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as::<_, (i64, String)>("SELECT wi_number, title FROM workitem WHERE node_id = $1")
        .bind(node_id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item with node_id {node_id}")).into())
}

/// The kind of a node, or `not_found` — existence and kind in one fetch, which
/// keeps `relate`'s endpoint checks a `not_found` on a typo'd id rather than a
/// raw FK violation surfaced as `internal` (WI #524).
async fn node_kind(pool: &PgPool, node_id: i64) -> Result<String> {
    sqlx::query_scalar::<_, String>("SELECT kind FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no node with id {node_id}")).into())
}

// --- name-or-id selectors (WI #575) -----------------------------------------
//
// Every write that targets a project used to take a bare `project_id`, so an
// agent that didn't already hold the id had to guess — and a wrong guess was a
// *silent wrong write*, not an error. A work item filed with `project_id: 1`
// landed in an archived project and reported success.
//
// Operations now accept either the id or the name, resolved here, in core, so
// both transports get identical behaviour. Three rules, and the reasons matter:
//
// 1. **Never both.** Passing `project_id` and `project` together is
//    `invalid_input`, not a precedence rule. A precedence rule silently
//    discards one of two things the caller explicitly asked for, which is the
//    very failure this change exists to remove.
// 2. **Resolve, never create.** An unknown name is an error. WI #537 removed
//    project-name acceptance from `update_card` precisely because it *created*
//    the project as a side effect of a card edit; that stays removed. Creating
//    a project is `create_project`'s job and nothing else's.
// 3. **Say what to do next.** An unresolvable name names `list_projects` as
//    the remedy — the same principle as `vocab::validate`, where the error
//    doubles as the documentation needed to retry.

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
fn unknown_label(label: &str) -> anyhow::Error {
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

/// Advance a node's `updated` after a write that landed in its satellite table
/// (WI #885).
///
/// `updated` is a `node` column and 0001's `touch_updated` trigger fires on
/// writes to `node` and `comment` only — but an ordinary field edit of a work
/// item, card, proposal or link writes `workitem`/`card`/`sprint_proposal`/
/// `link`. So `updated` advanced on exactly the fields that happen to live on
/// `node` (`archived`, `tags`, `category`, the project move) and sat frozen at
/// creation time for title, status, content, rank and the rest: an agent
/// sorting by recency saw creation order, and "recently touched" missed
/// everything actively being worked.
///
/// A trigger on each satellite table would be the more robust shape, and is
/// deliberately not what this is. The importer's second pass rewrites
/// `workitem.parent_node_id` after inserting every row, which such a trigger
/// would stamp with `now()` — and because the `node` trigger sets
/// `NEW.updated := now()` unconditionally on *any* update, nothing could then
/// restore the source timestamp `fidelity.rs` asserts. Cheap invariant, real
/// cost; the call sites are few and they are all here.
async fn touch_node<'e, E>(executor: E, node_id: i64) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query("UPDATE node SET updated = now() WHERE id = $1")
        .bind(node_id)
        .execute(executor)
        .await?;
    Ok(())
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
/// links, topics, proposals, handoffs — which is why #884's status check lives
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
async fn project_name_for_id(pool: &PgPool, id: i64) -> Result<String> {
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
async fn resolve_area<'e, E>(
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
async fn resolve_area_patch<'e, E>(
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

// --- collection reads: the envelope every list returns ----------------------

/// The shape every collection read returns (WI #534, D-3). `total` is the full
/// filtered count *before* `limit`/`offset`, so a caller can page without
/// guessing and can tell a complete answer from a clipped one.
///
/// That holds on **every** page, including one whose `offset` overshoots the
/// last row: `items` is empty there and `total` still reports the corpus, so
/// `remaining = total - offset` and "trust the last page's total" both stay
/// sound (WI #883). Count in a statement of your own, never with a
/// `count(*) OVER()` riding on the paged rows — that one returns zero exactly
/// when the page is empty.
///
/// Unbounded list reads were the review's context bomb: `list_work_items`
/// returned every row with full content, which is why `survey_work_items` had
/// to exist at all. #861 made the lean projection *the* MCP list read, leaving
/// the survey a deprecated alias of it, and #871 deleted that alias once its
/// last caller moved. This envelope carries `omitted` alongside on the reads
/// that also narrow by default.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

impl<T> Page<T> {
    /// Assemble a page from an already-executed query.
    pub fn from_parts(items: Vec<T>, total: i64, limit: i64, offset: i64) -> Self {
        Self::new(items, total, limit, offset)
    }

    fn new(items: Vec<T>, total: i64, limit: i64, offset: i64) -> Self {
        Self {
            items,
            total,
            limit,
            offset,
        }
    }
}

/// Default page size for collection reads. Generous enough that one project's
/// work items stay a single call (D-10), finite enough to bound the payload.
pub const LIST_LIMIT_DEFAULT: i64 = 200;
/// Hard ceiling a caller may request.
pub const LIST_LIMIT_MAX: i64 = 500;

/// Pagination knobs shared by every collection read. Defaults are applied in
/// [`PageQuery::resolve`], not here, so `None` means "use the documented
/// default" rather than "no limit".
#[derive(Debug, Clone, Copy, Default)]
pub struct PageQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl PageQuery {
    /// Clamped (limit, offset), for callers outside this module.
    pub fn resolve_public(&self) -> (i64, i64) {
        self.resolve()
    }

    /// Clamped (limit, offset) — callers can't escape the ceiling or go negative.
    fn resolve(&self) -> (i64, i64) {
        (
            self.limit
                .unwrap_or(LIST_LIMIT_DEFAULT)
                .clamp(1, LIST_LIMIT_MAX),
            self.offset.unwrap_or(0).max(0),
        )
    }
}

/// `archived` filter shared by every collection read: `Some(false)` hides
/// archived rows, `Some(true)` shows only them, `None` means both.
///
/// The default is `Some(false)` (D-3) and it is declared **once**, here, so
/// core and both transports cannot drift apart on it. Ask for `None`
/// explicitly to see everything.
pub type ArchivedFilter = Option<bool>;

/// The archived default every collection read starts from.
pub fn archived_default() -> ArchivedFilter {
    Some(false)
}

// --- work items -----------------------------------------------------------

/// `create_work_item` / `POST /api/work-items`. Both transports deserialize
/// this exact type, and the MCP input schema is derived from it (WI #539/#540).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewWorkItem {
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both. Resolved by exact name, and an unknown name returns `not_found`
    /// rather than mis-filing, so pass a name you are confident in directly.
    /// Call `list_projects` only when the name is genuinely unknown or
    /// ambiguous — the roster in this server's instructions already names
    /// every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub area_id: Option<i64>,
    /// Area name — the alternative to `area_id`, resolved within the item's project.
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default = "ops::default_task")]
    #[schemars(schema_with = "schema::wi_type")]
    pub wi_type: String,
    #[serde(default = "ops::default_open")]
    #[schemars(schema_with = "schema::wi_status")]
    pub wi_status: String,
    #[serde(default = "ops::default_unknown")]
    #[schemars(schema_with = "schema::wi_tshirt")]
    pub wi_tshirt: String,
    #[serde(default)]
    pub sprint: Option<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
}

/// Create a work item and return the row a read would return (WI #525) — a
/// superset of the old `{node_id, wi_number}` acknowledgement.
pub async fn create_work_item(pool: &PgPool, new: NewWorkItem) -> Result<WorkItemRow> {
    validate_status(&new.wi_status, &WI_STATUSES, "wi_status")?;
    validate_status(&new.wi_type, &vocab::WI_TYPES, "wi_type")?;
    validate_status(&new.wi_tshirt, &vocab::WI_TSHIRTS, "wi_tshirt")?;
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let area_id = resolve_area(pool, project_id, new.area_id, new.area.as_deref()).await?;
    // An area belongs to exactly one project; `update_work_item` has always
    // enforced that, `create_work_item` did not (WI #526). Resolving by name
    // satisfies this by construction; an explicit `area_id` still has to be
    // checked.
    if let Some(area_id) = area_id {
        let area_pid: Option<i64> = sqlx::query_scalar("SELECT project_id FROM area WHERE id = $1")
            .bind(area_id)
            .fetch_optional(pool)
            .await?;
        match area_pid {
            None => {
                return Err(RepoError::InvalidInput(format!(
                    "no area with id {area_id} — call list_areas for the available areas"
                ))
                .into());
            }
            Some(pid) if Some(pid) != project_id => {
                return Err(RepoError::InvalidInput(format!(
                    "area {area_id} does not belong to the work item's project"
                ))
                .into());
            }
            Some(_) => {}
        }
    }
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('workitem', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    // Since 0009_identity, wi_number IS the node id — one number everywhere.
    let wi_number: i64 = sqlx::query(
        "INSERT INTO workitem \
         (node_id, wi_number, area_id, wi_type, wi_status, wi_tshirt, sprint, title, content, details) \
         VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING wi_number",
    )
    .bind(node_id)
    .bind(area_id)
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
    get_work_item(pool, wi_number)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_number}")).into())
}

// --- cards ----------------------------------------------------------------

/// `create_card` / `POST /api/cards`. `rank` arrives as a JSON number and is
/// kept as a `Decimal` so fractional insertion never loses precision.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewCard {
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both. Resolved by exact name, and an unknown name returns `not_found`
    /// rather than mis-filing, so pass a name you are confident in directly.
    /// Call `list_projects` only when the name is genuinely unknown or
    /// ambiguous — the roster in this server's instructions already names
    /// every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[serde(default = "ops::default_backlog")]
    #[schemars(schema_with = "schema::card_status")]
    pub status: String,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Decimal,
}

pub async fn create_card(pool: &PgPool, new: NewCard) -> Result<CardRow> {
    validate_status(&new.status, &CARD_STATUSES, "card status")?;
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('card', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
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
    get_card(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no card with node_id {node_id}")).into())
}

// --- reading-list links ---------------------------------------------------

/// `create_link` / `POST /api/links`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewLink {
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both. Resolved by exact name, and an unknown name returns `not_found`
    /// rather than mis-filing, so pass a name you are confident in directly.
    /// Call `list_projects` only when the name is genuinely unknown or
    /// ambiguous — the roster in this server's instructions already names
    /// every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct LinkRow {
    pub node_id: i64,
    pub url: String,
    pub title: Option<String>,
    pub read: bool,
    pub disposition: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Capture recency (WI #888). Links are nodes, so `node` has carried both
    /// timestamps since 0001 — the row simply never selected them, leaving the
    /// one kind whose whole point is *when did I capture this* unable to say.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

pub async fn create_link(pool: &PgPool, new: NewLink) -> Result<LinkRow> {
    require_non_empty(&new.url, "link url")?;
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('link', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
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
    get_link(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no link with node_id {node_id}")).into())
}

const LINK_SELECT: &str =
    "SELECT l.node_id, l.url, l.title, l.read, l.disposition::text AS disposition, \
            n.category, n.tags, n.archived, n.created, n.updated \
     FROM link l JOIN node n ON n.id = l.node_id";

#[derive(Debug, Clone)]
pub struct LinkQuery {
    pub disposition: Option<String>,
    pub read: Option<bool>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for LinkQuery {
    fn default() -> Self {
        Self {
            disposition: None,
            read: None,
            archived: archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Reading-list links, enveloped and bounded (WI #534). Without this the read
/// returned the entire capture history forever.
pub async fn list_links(pool: &PgPool, query: LinkQuery) -> Result<Page<LinkRow>> {
    if let Some(d) = &query.disposition {
        validate_status(d, &LINK_DISPOSITIONS, "link disposition")?;
    }
    let (limit, offset) = query.page.resolve();
    const WHERE: &str = "WHERE ($1::text IS NULL OR l.disposition::text = $1) \
           AND ($2::bool IS NULL OR l.read = $2) \
           AND ($3::bool IS NULL OR n.archived = $3)";
    let items = sqlx::query_as::<_, LinkRow>(&format!(
        "{LINK_SELECT} {WHERE} ORDER BY l.node_id LIMIT $4 OFFSET $5"
    ))
    .bind(query.disposition.as_deref())
    .bind(query.read)
    .bind(query.archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM link l JOIN node n ON n.id = l.node_id {WHERE}"
    ))
    .bind(query.disposition.as_deref())
    .bind(query.read)
    .bind(query.archived)
    .fetch_one(pool)
    .await?;
    Ok(Page::new(items, total, limit, offset))
}

pub async fn get_link(pool: &PgPool, node_id: i64) -> Result<Option<LinkRow>> {
    Ok(
        sqlx::query_as::<_, LinkRow>(&format!("{LINK_SELECT} WHERE l.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// Everything `update_link` can change in one transaction. `None` leaves a
/// field alone.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct LinkPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::disposition")]
    pub disposition: Option<String>,
    #[serde(default)]
    pub read: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
    /// The lifecycle end for a link that was real (WI #888). `list_links` has
    /// documented an archived-excluded default since #534 with no write able
    /// to reach it; this is that write. For a capture that was never real —
    /// a probe, a mistyped URL — `delete_link` is the honest disposal.
    #[serde(default)]
    pub archived: Option<bool>,
}

/// One transactional link update (WI #538). The REST handler used to make up
/// to three independent repo calls, so a mid-sequence failure left a partial
/// write and an error that didn't say which parts landed. Validation happens
/// before the transaction opens, so an invalid disposition changes nothing.
pub async fn update_link(pool: &PgPool, node_id: i64, patch: LinkPatch) -> Result<LinkRow> {
    if let Some(d) = &patch.disposition {
        validate_status(d, &LINK_DISPOSITIONS, "link disposition")?;
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "link", "link").await?;
    if let Some(d) = &patch.disposition {
        sqlx::query("UPDATE link SET disposition = $2::link_disposition WHERE node_id = $1")
            .bind(node_id)
            .bind(d)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(read) = patch.read {
        sqlx::query("UPDATE link SET read = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(read)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tags) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(tags)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(archived) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(archived)
            .execute(&mut *tx)
            .await?;
    }
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    reread_link(pool, node_id).await
}

/// Hard-delete a link (WI #888) — the disposal for a capture that was never
/// real. `Ok(false)` means there was no such link, matching `delete_comment`.
///
/// It **refuses** rather than cascading. `relationship` and `comment` both
/// cascade from `node`, so an unguarded delete would take a link's edges and
/// its whole thread with it, and `daily_plan_item.source_node_id` would fail
/// with raw Postgres text instead of a `conflict` an agent can branch on.
/// Anything referenced is by definition real; archive it instead.
pub async fn delete_link(pool: &PgPool, node_id: i64) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(&mut *tx)
        .await?;
    match kind.as_deref() {
        None => return Ok(false),
        Some("link") => {}
        Some(other) => {
            return Err(
                RepoError::invalid(format!("node {node_id} is a {other}, not a link")).into(),
            );
        }
    }
    refuse_if_referenced(&mut tx, node_id).await?;
    sqlx::query("DELETE FROM node WHERE id = $1")
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

/// The refuse-if-referenced guard shared by korg's node-level hard deletes.
/// A row something else points at is a row with a history, and history is what
/// `archived` is for — so the caller has to resolve the reference explicitly
/// rather than let the schema's cascade decide silently.
async fn refuse_if_referenced(tx: &mut Transaction<'_, Postgres>, node_id: i64) -> Result<()> {
    let edges: i64 =
        sqlx::query_scalar("SELECT count(*) FROM relationship WHERE left_id = $1 OR right_id = $1")
            .bind(node_id)
            .fetch_one(&mut **tx)
            .await?;
    let comments: i64 = sqlx::query_scalar("SELECT count(*) FROM comment WHERE node_id = $1")
        .bind(node_id)
        .fetch_one(&mut **tx)
        .await?;
    let plan_items: i64 =
        sqlx::query_scalar("SELECT count(*) FROM daily_plan_item WHERE source_node_id = $1")
            .bind(node_id)
            .fetch_one(&mut **tx)
            .await?;
    if edges + comments + plan_items > 0 {
        let mut refs = Vec::new();
        if edges > 0 {
            refs.push(format!("{edges} relationship(s)"));
        }
        if comments > 0 {
            refs.push(format!("{comments} comment(s)"));
        }
        if plan_items > 0 {
            refs.push(format!("{plan_items} daily-plan item(s)"));
        }
        return Err(RepoError::Conflict(format!(
            "node {node_id} is referenced by {} — delete is for rows that were \
             never real; archive it instead, or remove the references first",
            refs.join(", ")
        ))
        .into());
    }
    Ok(())
}

pub async fn set_link_disposition(
    pool: &PgPool,
    node_id: i64,
    disposition: &str,
) -> Result<LinkRow> {
    validate_status(disposition, &LINK_DISPOSITIONS, "link disposition")?;
    require_kind(pool, node_id, "link", "link").await?;
    sqlx::query("UPDATE link SET disposition = $2::link_disposition WHERE node_id = $1")
        .bind(node_id)
        .bind(disposition)
        .execute(pool)
        .await?;
    touch_node(pool, node_id).await?;
    reread_link(pool, node_id).await
}

/// Update the cross-cutting tags on any node.
pub async fn set_node_tags(pool: &PgPool, node_id: i64, tags: &[String]) -> Result<()> {
    require_node(pool, node_id).await?;
    sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
        .bind(node_id)
        .bind(tags)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_link_read(pool: &PgPool, node_id: i64, read: bool) -> Result<LinkRow> {
    require_kind(pool, node_id, "link", "link").await?;
    sqlx::query("UPDATE link SET read = $2 WHERE node_id = $1")
        .bind(node_id)
        .bind(read)
        .execute(pool)
        .await?;
    touch_node(pool, node_id).await?;
    reread_link(pool, node_id).await
}

async fn reread_link(pool: &PgPool, node_id: i64) -> Result<LinkRow> {
    get_link(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no link with node_id {node_id}")).into())
}

// --- generalized relationships --------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Neighbor {
    pub rel_id: i64,
    pub node_id: i64,
    pub kind: String,
    pub label: String,
    /// "out" = the queried node is the edge's left (label reads queried → this
    /// neighbor, e.g. queried `depends_on` neighbor); "in" = the reverse.
    #[ts(type = "\"out\" | \"in\"")]
    pub direction: String,
    /// Whether `direction` carries meaning for this label (WI #530). False for
    /// registry-undirected labels like `related-to`, where the orientation is
    /// an artifact of how the edge happened to be written and readers must
    /// treat the edge as symmetric.
    #[sqlx(default)]
    pub directed: bool,
}

/// Filters and bound for a `neighbors` read (WI #533). Defaults: no filters,
/// [`NEIGHBOR_LIMIT_DEFAULT`].
#[derive(Debug, Clone, Default)]
pub struct NeighborQuery {
    pub label: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

/// Neighbors plus the bound that produced them. `total` is the full match
/// count before the limit, so `truncated` is exact rather than inferred.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct NeighborPage {
    pub items: Vec<Neighbor>,
    pub total: i64,
    pub limit: i64,
    pub truncated: bool,
}

/// Default cap on a `neighbors` read. Generous next to real fan-out (the
/// biggest production node has ~10 edges) but finite.
pub const NEIGHBOR_LIMIT_DEFAULT: i64 = 100;
/// Hard ceiling a caller may request.
pub const NEIGHBOR_LIMIT_MAX: i64 = 500;

pub async fn relate(
    pool: &PgPool,
    left: i64,
    right: i64,
    label: &str,
    origin: Option<&str>,
    rank: Option<Decimal>,
) -> Result<i64> {
    // A node related to itself is meaningless under every registry label and
    // actively harmful under depends_on — it would block itself forever
    // (WI #532). Backed by relationship_no_self_edge since 0014.
    if left == right {
        return Err(RepoError::InvalidInput(format!("cannot relate node {left} to itself")).into());
    }
    // Closed vocabulary (D-11): the label must be one korg declares. After
    // LB-1 the corpus already conforms, so this needs no grandfather clause —
    // enforced in core, the single write path both transports share, never a
    // DB trigger (which would re-create the drift class B4 killed).
    let spec = relationships::spec(label).ok_or_else(|| unknown_label(label))?;

    // Endpoints are checked up front (and their kinds fetched) so a typo'd node
    // id is a 404, not a raw FK violation surfaced as a 500 (WI #524).
    let left_kind = node_kind(pool, left).await?;
    let right_kind = node_kind(pool, right).await?;

    // Endpoint kinds (D-12): a kind-constrained label (covers, finding)
    // validates both ends. covers/finding written by create_proposal /
    // upsert_report are correct by construction and never reach this path.
    if let Some(expected) = spec.left_kind {
        if left_kind != expected {
            return Err(RepoError::InvalidInput(format!(
                "label '{label}' requires a {expected} on the left, but node {left} is a {left_kind}"
            ))
            .into());
        }
    }
    if let Some(expected) = spec.right_kind {
        if right_kind != expected {
            return Err(RepoError::InvalidInput(format!(
                "label '{label}' requires a {expected} on the right, but node {right} is a {right_kind}"
            ))
            .into());
        }
    }

    // Single-project labels (sprint 043, #967): `covers` may not join two
    // projects. Same reasoning as the kind check above — enforced in core, the
    // one write path both transports share, not in a DB trigger. A node with no
    // project is unfiled rather than filed elsewhere, so it passes; the corpus
    // holds no unfiled work items (measured 2026-08-05) and `create_work_item`
    // still allows one.
    if spec.same_project {
        let (left_project, right_project) = (
            node_project(pool, left).await?,
            node_project(pool, right).await?,
        );
        if let (Some(lp), Some(rp)) = (&left_project, &right_project) {
            if lp != rp {
                let (wi_number, title) = wi_handle(pool, right).await?;
                return Err(cross_project_covers(lp, wi_number, rp, &title));
            }
        }
    }

    // L-10: a registry-undirected label (related-to) whose reverse edge already
    // exists dedups to it instead of storing a mirror. Directed labels keep
    // both orientations — A depends_on B and B depends_on A is a cycle, not a
    // duplicate — so this only fires for the undirected case.
    if !spec.directed {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM relationship WHERE left_id = $1 AND right_id = $2 AND relationship = $3",
        )
        .bind(right)
        .bind(left)
        .bind(label)
        .fetch_optional(pool)
        .await?;
        if let Some(id) = existing {
            return Ok(id);
        }
    }

    // Provenance (D-17): stamp created + self-reported origin on insert. The
    // ON CONFLICT clause preserves the original created/origin (what LB-1's
    // migration comment reserved) — it used to be a pure no-op.
    //
    // Sprint 044 (D-9) gives it one job: `rank` is COALESCEd, so re-relating an
    // existing edge **with** a rank reorders it in place, and re-relating
    // **without** one is still a no-op. Without this there is no reorder path
    // short of unrelate + relate, which would churn a slice's provenance just to
    // move it up a position.
    let id: i64 = sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship, created, origin, rank) \
         VALUES ($1, $2, $3, now(), $4, $5) \
         ON CONFLICT (left_id, right_id, relationship) \
         DO UPDATE SET rank = COALESCE(EXCLUDED.rank, relationship.rank) \
         RETURNING id",
    )
    .bind(left)
    .bind(right)
    .bind(label)
    .bind(origin)
    .bind(rank)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// Neighbors of `node`: the node on the other end of each edge (direction
/// tells you which end the queried node is), with that node's kind and the
/// relationship label. Works across kinds.
///
/// Ordering is `node_id` then `rel_id`, so two edges to the same neighbor have
/// a stable relative order (F-19). `label`/`kind` filter server-side — the
/// Planning page and several skills used to pull every edge and filter in the
/// client.
pub async fn neighbors(pool: &PgPool, node: i64, query: NeighborQuery) -> Result<NeighborPage> {
    let limit = query
        .limit
        .unwrap_or(NEIGHBOR_LIMIT_DEFAULT)
        .clamp(1, NEIGHBOR_LIMIT_MAX);
    let sql = "SELECT r.id AS rel_id, n.id AS node_id, n.kind, r.relationship AS label, \
                      CASE WHEN r.left_id = $1 THEN 'out' ELSE 'in' END AS direction, \
                      count(*) OVER() AS total \
               FROM relationship r \
               JOIN node n \
                 ON n.id = CASE WHEN r.left_id = $1 THEN r.right_id ELSE r.left_id END \
               WHERE (r.left_id = $1 OR r.right_id = $1) \
                 AND ($2::text IS NULL OR r.relationship = $2) \
                 AND ($3::text IS NULL OR n.kind = $3) \
               ORDER BY n.id, r.id \
               LIMIT $4";

    #[derive(sqlx::FromRow)]
    struct Row {
        rel_id: i64,
        node_id: i64,
        kind: String,
        label: String,
        direction: String,
        total: i64,
    }

    let rows = sqlx::query_as::<_, Row>(sql)
        .bind(node)
        .bind(query.label.as_deref())
        .bind(query.kind.as_deref())
        .bind(limit)
        .fetch_all(pool)
        .await?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
    let items: Vec<Neighbor> = rows
        .into_iter()
        .map(|r| Neighbor {
            directed: relationships::direction_is_meaningful(&r.label),
            rel_id: r.rel_id,
            node_id: r.node_id,
            kind: r.kind,
            label: r.label,
            direction: r.direction,
        })
        .collect();
    let truncated = total > items.len() as i64;
    Ok(NeighborPage {
        items,
        total,
        limit,
        truncated,
    })
}

/// A neighbor as a focused read inlines it (LB-3, D-20): a compact edge ref
/// carrying enough to render and decide — the neighbor's `title` and
/// `wi_number` — without a second round-trip. The generalization of `covered` /
/// inlined `comments` from one label / comments to every edge.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct RelatedRef {
    pub rel_id: i64,
    pub node_id: i64,
    /// Present when the neighbor is a work item — its user-facing handle.
    pub wi_number: Option<i64>,
    pub kind: String,
    /// The neighbor's title/summary/name, resolved across kinds.
    pub title: String,
    pub label: String,
    #[ts(type = "\"out\" | \"in\"")]
    pub direction: String,
    /// Whether `direction` carries meaning (registry-undirected labels: false).
    pub directed: bool,
}

/// Max edges inlined into a focused read before `related_truncated` trips
/// (LB-3). Production's densest node has 9 edges; 25 inlines every current node
/// in full and bounds the payload, truncating only the handoff-attached future.
/// Past the cap the caller falls back to `neighbors` for the complete set.
pub const RELATED_CONTEXT_CAP: i64 = 25;

/// The inlined related-context block for a focused read (LB-3): up to
/// [`RELATED_CONTEXT_CAP`] of `node`'s edges, ordered by `(label, node_id)` so
/// structural labels (`covers`, `depends_on`, `finding`) survive truncation
/// ahead of `related-to`, plus whether more were dropped. `exclude_label` omits
/// a label already inlined elsewhere — `get_proposal` passes `covers`, which it
/// carries as `covered`. Titles resolve in one query (no N+1); `directed` comes
/// from the registry, exactly as `neighbors` computes it.
pub async fn related_context(
    pool: &PgPool,
    node: i64,
    exclude_label: Option<&str>,
) -> Result<(Vec<RelatedRef>, bool)> {
    #[derive(sqlx::FromRow)]
    struct Row {
        rel_id: i64,
        node_id: i64,
        wi_number: Option<i64>,
        kind: String,
        title: String,
        label: String,
        direction: String,
        total: i64,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT r.id AS rel_id, \
                other.id AS node_id, \
                w.wi_number AS wi_number, \
                other.kind AS kind, \
                COALESCE(w.title, sp.title, pg.title, cd.title, lk.title, lk.url, tp.name, \
                         rp.summary, hd.title, other.kind || ' #' || other.id) AS title, \
                r.relationship AS label, \
                CASE WHEN r.left_id = $1 THEN 'out' ELSE 'in' END AS direction, \
                count(*) OVER() AS total \
         FROM relationship r \
         JOIN node other \
           ON other.id = CASE WHEN r.left_id = $1 THEN r.right_id ELSE r.left_id END \
         LEFT JOIN workitem w         ON w.node_id  = other.id \
         LEFT JOIN sprint_proposal sp ON sp.node_id = other.id \
         LEFT JOIN program pg         ON pg.node_id = other.id \
         LEFT JOIN card cd            ON cd.node_id = other.id \
         LEFT JOIN link lk            ON lk.node_id = other.id \
         LEFT JOIN topic tp           ON tp.node_id = other.id \
         LEFT JOIN report rp          ON rp.node_id = other.id \
         LEFT JOIN handoff hd         ON hd.node_id = other.id \
         WHERE (r.left_id = $1 OR r.right_id = $1) \
           AND ($2::text IS NULL OR r.relationship <> $2) \
         ORDER BY r.relationship, other.id \
         LIMIT $3",
    )
    .bind(node)
    .bind(exclude_label)
    .bind(RELATED_CONTEXT_CAP)
    .fetch_all(pool)
    .await?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
    let related: Vec<RelatedRef> = rows
        .into_iter()
        .map(|r| RelatedRef {
            directed: relationships::direction_is_meaningful(&r.label),
            rel_id: r.rel_id,
            node_id: r.node_id,
            wi_number: r.wi_number,
            kind: r.kind,
            title: r.title,
            label: r.label,
            direction: r.direction,
        })
        .collect();
    let truncated = total > related.len() as i64;
    Ok((related, truncated))
}

/// All (left, right) edges with the given label where BOTH endpoints belong
/// to the named project. Feeds the Plan view: with label `depends_on`, left
/// depends on right.
pub async fn project_edges(pool: &PgPool, project: &str, label: &str) -> Result<Vec<(i64, i64)>> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT r.left_id, r.right_id \
         FROM relationship r \
         JOIN node nl ON nl.id = r.left_id \
         JOIN node nr ON nr.id = r.right_id \
         JOIN project p ON p.id = nl.project_id AND p.id = nr.project_id \
         WHERE p.name = $1 AND r.relationship = $2 \
         ORDER BY r.id",
    )
    .bind(project)
    .bind(label)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Delete an edge; `false` means there was nothing with that id (WI #525 —
/// deletes report what they did instead of always claiming success).
pub async fn unrelate(pool: &PgPool, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM relationship WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

// --- cross-kind node preview (WI #260) -------------------------------------

/// A label/value metadata row in a node preview (e.g. "Area" → "ui").
#[derive(Debug, Clone, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct NodeField {
    pub label: String,
    pub value: String,
}

/// A uniform, kind-agnostic preview of any node, used by the "find by ID"
/// search + preview panel: enough to identify and read an item without knowing
/// its kind up front. `wi_number` is `Some` only for work items (where it
/// equals the node id) — the UI navigates to those rather than previewing.
/// `body`/`details` are markdown; `badges` are short status chips; `fields`
/// are label/value metadata rows.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct NodePreview {
    pub node_id: i64,
    pub kind: String,
    pub wi_number: Option<i64>,
    pub title: String,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    pub badges: Vec<String>,
    pub fields: Vec<NodeField>,
    pub body: Option<String>,
    pub body_label: Option<String>,
    pub details: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

fn field(label: &str, value: impl Into<String>) -> NodeField {
    NodeField {
        label: label.into(),
        value: value.into(),
    }
}

/// Resolve any node id to a uniform preview, dispatching on its kind. Returns
/// `None` if no node has that id. Dates are read as `YYYY-MM-DD` text so the
/// payload needs no client-side date parsing.
pub async fn get_node_preview(pool: &PgPool, id: i64) -> Result<Option<NodePreview>> {
    let base = sqlx::query(
        "SELECT n.kind, pj.name AS project, n.tags, n.archived, n.created, n.updated \
         FROM node n LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE n.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let Some(base) = base else { return Ok(None) };

    let kind: String = base.get("kind");
    let mut p = NodePreview {
        node_id: id,
        kind: kind.clone(),
        wi_number: None,
        title: format!("{kind} #{id}"),
        project: base.get("project"),
        tags: base.get("tags"),
        archived: base.get("archived"),
        badges: Vec::new(),
        fields: Vec::new(),
        body: None,
        body_label: None,
        details: None,
        created: base.get("created"),
        updated: base.get("updated"),
    };

    match kind.as_str() {
        "workitem" => {
            if let Some(r) = sqlx::query(
                "SELECT w.wi_number, w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, \
                        a.name AS area, w.title, w.content, w.details \
                 FROM workitem w LEFT JOIN area a ON a.id = w.area_id \
                 WHERE w.node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.wi_number = Some(r.get("wi_number"));
                p.title = r.get("title");
                p.badges = vec![r.get("wi_type"), r.get("wi_status"), r.get("wi_tshirt")];
                if let Some(area) = r.get::<Option<String>, _>("area") {
                    p.fields.push(field("Area", area));
                }
                if let Some(sprint) = r.get::<Option<String>, _>("sprint") {
                    p.fields.push(field("Sprint", sprint));
                }
                p.body = Some(r.get("content"));
                p.body_label = Some("Content".into());
                p.details = r.get("details");
            }
        }
        "card" => {
            if let Some(r) = sqlx::query(
                "SELECT status::text AS status, title, description FROM card WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("title");
                p.badges = vec![r.get("status")];
                let desc: String = r.get("description");
                if !desc.trim().is_empty() {
                    p.body = Some(desc);
                    p.body_label = Some("Description".into());
                }
            }
        }
        "link" => {
            if let Some(r) = sqlx::query(
                "SELECT url, title, read, disposition::text AS disposition FROM link WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                let url: String = r.get("url");
                p.title = r.get::<Option<String>, _>("title").unwrap_or_else(|| url.clone());
                p.badges = vec![
                    r.get("disposition"),
                    if r.get::<bool, _>("read") { "read".into() } else { "unread".into() },
                ];
                p.fields.push(field("URL", url));
            }
        }
        "report" => {
            if let Some(r) = sqlx::query(
                "SELECT source, to_char(report_date, 'YYYY-MM-DD') AS report_date, status, \
                        summary, body, model, escalated \
                 FROM report WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                let source: String = r.get("source");
                let date: String = r.get("report_date");
                p.title = format!("{source} — {date}");
                p.badges = vec![r.get("status")];
                if r.get::<bool, _>("escalated") {
                    p.badges.push("escalated".into());
                }
                if let Some(model) = r.get::<Option<String>, _>("model") {
                    p.fields.push(field("Model", model));
                }
                p.fields.push(field("Summary", r.get::<String, _>("summary")));
                p.body = Some(r.get("body"));
                p.body_label = Some("Report".into());
            }
        }
        "sprint_proposal" => {
            if let Some(r) = sqlx::query(
                "SELECT title, summary, notes, status::text AS status, pinned \
                 FROM sprint_proposal WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("title");
                p.badges = vec![r.get("status")];
                if r.get::<bool, _>("pinned") {
                    p.badges.push("pinned".into());
                }
                // Since #860 the summary is a 500-char contract and the analysis
                // lives in `notes`, so the body is `notes` where there is one —
                // otherwise this preview would show the shortest form korg has
                // and nothing else. The summary stays visible as a field.
                match r.get::<Option<String>, _>("notes") {
                    Some(notes) => {
                        p.fields.push(field("Summary", r.get::<String, _>("summary")));
                        p.body = Some(notes);
                        p.body_label = Some("Notes".into());
                    }
                    None => {
                        p.body = Some(r.get("summary"));
                        p.body_label = Some("Summary".into());
                    }
                }
            }
        }
        "topic" => {
            if let Some(r) = sqlx::query(
            "SELECT name, description FROM topic WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("name");
                if let Some(description) = r.get::<Option<String>, _>("description") {
                    p.body = Some(description);
                    p.body_label = Some("Description".into());
                }
            }
        }
        "daily_plan_item" => {
            if let Some(r) = sqlx::query(
                "SELECT to_char(plan_date, 'YYYY-MM-DD') AS plan_date, display, \
                        source_node_id, completed_at IS NOT NULL AS completed \
                 FROM daily_plan_item WHERE node_id = $1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await?
            {
                p.title = r.get("display");
                p.fields.push(field("Date", r.get::<String, _>("plan_date")));
                p.fields.push(field("Source", format!("#{}", r.get::<i64, _>("source_node_id"))));
                if r.get::<bool, _>("completed") {
                    p.badges.push("complete".into());
                }
            }
        }
        "handoff" => {
            // Sprint 026: the handoff "viewer" is this generic slide-over. The
            // owning read (get_work_item/get_proposal) surfaces the has_handoff
            // ref; clicking it opens this preview. A missing detail row leaves
            // the default `handoff #<id>` title rather than a blank node.
            if let Some(r) =
                sqlx::query("SELECT title, summary, body FROM handoff WHERE node_id = $1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await?
            {
                p.title = r.get("title");
                p.fields.push(field("Summary", r.get::<String, _>("summary")));
                p.body = Some(r.get("body"));
                p.body_label = Some("Handoff".into());
            }
        }
        _ => {}
    }

    Ok(Some(p))
}

// --- read views -----------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemRow {
    pub wi_number: i64,
    pub node_id: i64,
    pub project: Option<String>,
    pub area: Option<String>,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub sprint: Option<String>,
    pub title: String,
    pub content: String,
    pub details: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub parent: Option<i64>,
    pub archived: bool,
    /// Number of comments on this work item (WI #392) — the hint that tells an
    /// agent "this row has discussion; fetch it".
    pub comment_count: i64,
    /// True when a `has_handoff` edge leaves this item (WI #813) — durable
    /// context another session left, waiting to be read.
    pub has_handoff: bool,
    /// The live proposal covering this item, or `None` when nothing does
    /// (WI #824). See `membership_select!` for why "live" and why one id.
    pub proposal_node_id: Option<i64>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// The two membership markers (sprint 042), spliced into both row tiers — the
/// full REST row and the lean MCP summary. Written once because the whole
/// point of bundling #813/#824/#823 was to pay the contract cost *once*; two
/// copies of this SQL is how the tiers drift on what "spoken for" means.
///
/// Both read the `relationship` table, and the two labels point **opposite
/// ways** — the trap #813 flagged and both drafts still got wrong:
///
/// - `covers` is proposal -> work item, so the item is the edge's **right**
///   end. Drafts wrote `left_node_id`/`label`; the columns are
///   `left_id`/`right_id`/`relationship`.
/// - `has_handoff` is node -> handoff, so the item is the **left** end.
///
/// **Why only live proposals.** The question the rows are answering is "is
/// this already spoken for?" — the one that cost 17 `get_proposal` calls in
/// the 2026-07-31 backlog review. A `declined` proposal speaks for nothing,
/// and a `done` one has already had its say; either would paint an open item
/// in the "claimed" colour and answer that question wrong. `min` picks a
/// single id when more than one live proposal covers an item — rare, and a
/// stable pick beats an arbitrary one.
///
/// **Why joins and not the correlated subqueries the WIs sketched.** Both WIs
/// proposed `EXISTS (SELECT 1 FROM relationship …)` per row, and #813 asked
/// for a measurement rather than an assumption. The measurement
/// (`tests/sprint042_measure.rs`, 1000 items / 100 proposals / 60 handoffs)
/// said the assumption was wrong: correlated subqueries cost **+324%** on the
/// REST read and **+448%** on the lean one, because a 500-row page does 500
/// index lookups plus 500 joins to `sprint_proposal`.
///
/// These pre-aggregate instead. Both edge sets are *small* — coverage is
/// hundreds of rows, handoffs dozens — so Postgres scans each once behind
/// `relationship_label_idx` and hash-joins the result, turning per-row work
/// into per-*query* work. Same answers, and the cost drops to noise. Grouping
/// in the subquery (rather than `DISTINCT` on the join) keeps the join
/// one-to-one, so no row can be duplicated by a second covering edge.
macro_rules! membership_columns {
    () => {
        "(ho.left_id IS NOT NULL) AS has_handoff, cov.proposal_node_id"
    };
}

/// The `FROM` half of [`membership_columns!`] — kept adjacent because the two
/// are meaningless apart, and a select list referencing a join that is not
/// there fails at runtime rather than compile time.
macro_rules! membership_joins {
    () => {
        "LEFT JOIN (SELECT r.right_id AS node_id, min(r.left_id) AS proposal_node_id \
                      FROM relationship r \
                      JOIN sprint_proposal sp ON sp.node_id = r.left_id \
                     WHERE r.relationship = 'covers' \
                       AND sp.status::text IN ('proposed', 'active') \
                     GROUP BY r.right_id) cov ON cov.node_id = w.node_id \
         LEFT JOIN (SELECT DISTINCT r.left_id FROM relationship r \
                     WHERE r.relationship = 'has_handoff') ho ON ho.left_id = w.node_id"
    };
}

const WORKITEM_SELECT: &str = concat!(
    "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, \
        (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, ",
    membership_columns!(),
    ", n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id ",
    membership_joins!()
);

/// Filters for a work-item listing. `project` is a name, matching the other
/// project-keyed surfaces.
#[derive(Debug, Clone)]
pub struct WorkItemQuery {
    pub project: Option<String>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for WorkItemQuery {
    fn default() -> Self {
        Self {
            project: None,
            archived: archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Full work-item rows for one project (or all), enveloped and bounded
/// (WI #534). **REST only since #861** — `GET /api/work-items`, which the Work
/// Items page walks to completion (035 D-1) and then filters, searches and
/// derives tag chips from in memory, all of which need `content` and `details`.
/// The MCP `list_work_items` is [`list_work_items_lean`]; the full tier there is
/// `get_work_item`, one row at a time.
pub async fn list_work_items(pool: &PgPool, query: WorkItemQuery) -> Result<Page<WorkItemRow>> {
    let (limit, offset) = query.page.resolve();
    let sql = format!(
        "{WORKITEM_SELECT} \
         WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::bool IS NULL OR n.archived = $2) \
         ORDER BY w.wi_number \
         LIMIT $3 OFFSET $4"
    );
    let items = sqlx::query_as::<_, WorkItemRow>(&sql)
        .bind(query.project.as_deref())
        .bind(query.archived)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await?;
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workitem w JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::bool IS NULL OR n.archived = $2)",
    )
    .bind(query.project.as_deref())
    .bind(query.archived)
    .fetch_one(pool)
    .await?;
    Ok(Page::new(items, total, limit, offset))
}

pub async fn get_work_item(pool: &PgPool, wi_number: i64) -> Result<Option<WorkItemRow>> {
    let sql = format!("{WORKITEM_SELECT} WHERE w.wi_number = $1");
    Ok(sqlx::query_as::<_, WorkItemRow>(&sql)
        .bind(wi_number)
        .fetch_optional(pool)
        .await?)
}

/// Max comments inlined into a single-item detail fetch (WI #392). A
/// pathological thread past this is truncated with `comments_truncated`, and
/// the caller fetches the whole thread from `list_comments` — which is
/// unpaginated, so it is a re-read rather than a tail (WI #890).
pub const WORKITEM_COMMENT_CAP: i64 = 10;

/// A work item plus its comments, capped (WI #392). The single-item detail
/// fetch commits to the full state of one item — and comments frequently hold
/// the payload (resolution rationale, decisions), so agents that only call
/// `get_work_item` should see them without a second round-trip. `item.comment_count`
/// is the true total; `comments` holds at most `WORKITEM_COMMENT_CAP` of them.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub item: WorkItemRow,
    pub comments: Vec<Comment>,
    /// True when there are more comments than were inlined (call `list_comments`).
    pub comments_truncated: bool,
    /// The item's edges, inlined (LB-3): covers-IN reveals which proposal covers
    /// it, plus depends_on / related-to / finding. Capped and label-ordered.
    pub related: Vec<RelatedRef>,
    /// True when there are more edges than were inlined (call `neighbors`).
    pub related_truncated: bool,
}

/// `get_work_item` plus inlined, capped comments (WI #392). `None` if the
/// work item doesn't exist.
pub async fn get_work_item_detail(pool: &PgPool, wi_number: i64) -> Result<Option<WorkItemDetail>> {
    let Some(item) = get_work_item(pool, wi_number).await? else {
        return Ok(None);
    };
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created LIMIT $2",
    )
    .bind(item.node_id)
    .bind(WORKITEM_COMMENT_CAP)
    .fetch_all(pool)
    .await?;
    let comments_truncated = item.comment_count > WORKITEM_COMMENT_CAP;
    // All of the item's edges — no label is inlined elsewhere on a work item.
    let (related, related_truncated) = related_context(pool, item.node_id, None).await?;
    Ok(Some(WorkItemDetail {
        item,
        comments,
        comments_truncated,
        related,
        related_truncated,
    }))
}

// --- the lean work-item list (WI #861) --------------------------------------
//
// `list_work_items` above is the REST/web read and stays as it is: the Work
// Items page holds its whole collection and filters, searches and derives tag
// chips in memory (035 D-1), which needs `content` and `details`. It is a
// browser, not a token budget — the same split `list_proposals` has had since
// #852 and `list_projects` since #828.
//
// The MCP read is the one that measured ~890k chars of content+details across
// the corpus, with 78% of the rows `closed` while `update_work_item`'s own
// schema claimed closed items were "hidden by default". Everything below is
// that read: survey's projection, terminal statuses excluded by default, and an
// `omitted` envelope so both narrowings are visible.

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemSummary {
    pub wi_number: i64,
    pub node_id: i64,
    pub project: Option<String>,
    pub title: String,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    /// Comment count (WI #392) — signals which rows carry discussion worth fetching.
    pub comment_count: i64,
    /// True when this item has a non-empty `details` section (WI #813). The
    /// projection carries no bodies by design, so the Review page had to show
    /// no 📝 rather than a false negative; a boolean is the honest fix and
    /// costs no join.
    pub has_details: bool,
    /// True when a `has_handoff` edge leaves this item (WI #813) — "which of
    /// these already has durable context waiting", which is precisely what the
    /// survey could not answer without N follow-up `neighbors` calls.
    pub has_handoff: bool,
    /// The live proposal covering this item (WI #824) — "which of these is
    /// already spoken for", the other question that cost N follow-up calls.
    pub proposal_node_id: Option<i64>,
}

/// What the lean list's defaults hid (WI #851, extended by #861), computed as a
/// **cascade** so no row is counted twice: `archived` is what the archived
/// filter excluded, and `closed` is counted only over the rows that *passed* it.
/// An archived closed item therefore lands in `archived` and nowhere else.
///
/// A field is 0 when the caller asked to see that class — `wi_status: "all"`
/// (or `"closed"`) zeroes `closed`, `archived: null`/`true` zeroes `archived`.
/// Naming each field for what it counts keeps it honest under every setting,
/// rather than meaning "unarchived" half the time.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemOmitted {
    pub closed: i64,
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemListLean {
    pub items: Vec<WorkItemSummary>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    /// The rows the defaults hid. `total` is the count *after* filtering, so
    /// without this a sweep used to decide "is this project drained?" cannot
    /// tell a narrowed answer from a complete one — the same silent-truncation
    /// failure `list_projects`' `omitted` exists to treat (WI #828).
    pub omitted: WorkItemOmitted,
}

/// Resolve the `wi_status` argument into the statuses a lean list returns.
///
/// Absent → the non-terminal ones (`open` + `resolved` + `done`), which is what
/// makes `update_work_item`'s "closed … hidden by default" true for the first
/// time. `Some("all")` → no filter. Anything else is validated against
/// `WI_STATUSES` and returned alone. Identical in shape to
/// `proposal_status_predicate` (#852) and `project_status_predicate` (#828).
fn wi_status_predicate(status: Option<&str>) -> Result<Option<Vec<String>>> {
    match status {
        None => Ok(Some(
            WI_LIVE_STATUSES.iter().map(|s| s.to_string()).collect(),
        )),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &WI_STATUSES, "work item status")?;
            Ok(Some(vec![s.to_string()]))
        }
    }
}

/// A slim, paginated projection of work items (no content/details), excluding
/// terminal and archived rows by default and saying what that hid.
///
/// This is `list_work_items` over MCP and `GET /api/work-items/survey` over
/// REST. `total` is the full filtered count (before LIMIT/OFFSET), so callers
/// can page.
pub async fn list_work_items_lean(
    pool: &PgPool,
    project: Option<&str>,
    wi_status: Option<&str>,
    archived: ArchivedFilter,
    limit: i64,
    offset: i64,
) -> Result<WorkItemListLean> {
    let shown = wi_status_predicate(wi_status)?;
    #[derive(sqlx::FromRow)]
    struct Row {
        wi_number: i64,
        node_id: i64,
        project: Option<String>,
        title: String,
        wi_type: String,
        wi_status: String,
        wi_tshirt: String,
        comment_count: i64,
        has_details: bool,
        has_handoff: bool,
        proposal_node_id: Option<i64>,
    }
    let rows = sqlx::query_as::<_, Row>(concat!(
        "SELECT w.wi_number, w.node_id, pj.name AS project, w.title, \
                w.wi_type, w.wi_status, w.wi_tshirt, \
                (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
                (w.details IS NOT NULL AND w.details <> '') AS has_details, ",
        membership_columns!(),
        " FROM workitem w \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id ",
        membership_joins!(),
        " WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::text[] IS NULL OR w.wi_status = ANY($2)) \
           AND ($3::bool IS NULL OR n.archived = $3) \
         ORDER BY w.wi_number \
         LIMIT $4 OFFSET $5",
    ))
    .bind(project)
    .bind(shown.as_deref())
    .bind(archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let items = rows
        .into_iter()
        .map(|r| WorkItemSummary {
            wi_number: r.wi_number,
            node_id: r.node_id,
            project: r.project,
            title: r.title,
            wi_type: r.wi_type,
            wi_status: r.wi_status,
            wi_tshirt: r.wi_tshirt,
            comment_count: r.comment_count,
            has_details: r.has_details,
            has_handoff: r.has_handoff,
            proposal_node_id: r.proposal_node_id,
        })
        .collect();

    let (total, omitted) = wi_counts(pool, project, archived, shown.as_deref()).await?;
    Ok(WorkItemListLean {
        items,
        total,
        limit,
        offset,
        omitted,
    })
}

/// The lean list's corpus size and what its defaults hid, in one round trip.
///
/// `total` is deliberately *not* a `count(*) OVER()` on the page query. That
/// window count rides on the returned rows, so a page whose `offset` overshoots
/// the last row returns no rows, no count, and `total: 0` — the envelope
/// claiming an empty corpus exactly when a pager runs off the end (WI #883).
/// Counting in this statement, whose result set is one row whatever the paging
/// did, is what makes `total` mean the same thing on every page.
///
/// The `omitted` counts are taken over the `project`-filtered corpus —
/// narrowing to a project is the caller *choosing* a scope, not a default
/// hiding rows from them, so it is not something `omitted` should report. Same
/// reasoning as `proposal_omitted`. `total` narrows further than that: it is
/// the count after *every* filter, which is what a pager needs.
async fn wi_counts(
    pool: &PgPool,
    project: Option<&str>,
    archived: ArchivedFilter,
    shown: Option<&[String]>,
) -> Result<(i64, WorkItemOmitted)> {
    let (total, archived_hidden, closed) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND ($3::text[] IS NULL OR w.wi_status = ANY($3))), \
           count(*) FILTER (WHERE n.archived AND $2::bool IS NOT NULL AND NOT $2), \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND w.wi_status = 'closed') \
         FROM workitem w \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1)",
    )
    .bind(project)
    .bind(archived)
    .bind(shown)
    .fetch_one(pool)
    .await?;
    // A status the caller asked for isn't omitted, however many rows it has.
    let closed_hidden = match shown {
        Some(list) if list.iter().any(|s| s == "closed") => 0,
        None => 0,
        _ => closed,
    };
    Ok((
        total,
        WorkItemOmitted {
            closed: closed_hidden,
            archived: archived_hidden,
        },
    ))
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct CardRow {
    pub node_id: i64,
    pub status: String,
    pub title: String,
    pub description: String,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this card (WI #535).
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const CARD_SELECT: &str =
    "SELECT c.node_id, c.status::text AS status, c.title, c.description, c.rank, \
            pj.name AS project, n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = c.node_id) AS comment_count, \
            n.created, n.updated \
     FROM card c \
     JOIN node n ON n.id = c.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

#[derive(Debug, Clone)]
pub struct CardQuery {
    pub status: Option<String>,
    pub project: Option<String>,
    pub archived: ArchivedFilter,
    pub page: PageQuery,
}

impl Default for CardQuery {
    fn default() -> Self {
        Self {
            status: None,
            project: None,
            archived: archived_default(),
            page: PageQuery::default(),
        }
    }
}

/// Cards, enveloped and bounded (WI #534). Ordering gains a `node_id`
/// tie-breaker (F-19) so equal-rank cards don't shuffle between calls.
pub async fn list_cards(pool: &PgPool, query: CardQuery) -> Result<Page<CardRow>> {
    if let Some(status) = &query.status {
        validate_status(status, &CARD_STATUSES, "card status")?;
    }
    let (limit, offset) = query.page.resolve();
    const WHERE: &str = "WHERE ($1::text IS NULL OR c.status::text = $1) \
           AND ($2::text IS NULL OR pj.name = $2) \
           AND ($3::bool IS NULL OR n.archived = $3)";
    let items = sqlx::query_as::<_, CardRow>(&format!(
        "{CARD_SELECT} {WHERE} ORDER BY c.status, c.rank ASC, c.node_id ASC LIMIT $4 OFFSET $5"
    ))
    .bind(query.status.as_deref())
    .bind(query.project.as_deref())
    .bind(query.archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let total: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM card c JOIN node n ON n.id = c.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id {WHERE}"
    ))
    .bind(query.status.as_deref())
    .bind(query.project.as_deref())
    .bind(query.archived)
    .fetch_one(pool)
    .await?;
    Ok(Page::new(items, total, limit, offset))
}

pub async fn get_card(pool: &PgPool, node_id: i64) -> Result<Option<CardRow>> {
    Ok(
        sqlx::query_as::<_, CardRow>(&format!("{CARD_SELECT} WHERE c.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProjectRow {
    pub id: i64,
    pub name: String,
    pub gh_repo: Option<String>,
    /// Where the working copy lives on this project's *development* machine —
    /// the `machines` entry, never `deploy_to` (WI #675). Canonical form is
    /// `~/`-relative, no trailing slash, no whitespace or parentheses; the
    /// `project_src_path_canonical` constraint (migration 0019) enforces it.
    pub src_path: Option<String>,
    /// The routing contract: one line, ≤160 chars, saying what work belongs
    /// here and — where a sibling plausibly claims the same work — what does
    /// not. Capped by `project_description_routing_line` (0020); the long form
    /// lives in `notes`.
    pub description: Option<String>,
    /// Long-form operational context (WI #828): deploy topology, build
    /// commands, house conventions. Unbounded, and deliberately absent from the
    /// lean `list_projects` and the MCP instructions roster — it exists so that
    /// capping `description` did not have to destroy prose worth keeping.
    pub notes: Option<String>,
    /// Lifecycle status — see PROJECT_STATUSES.
    pub status: String,
    /// Machines this project's working copy lives on (kai/kubs0/cleo…).
    pub machines: Vec<String>,
    /// Machines this project deploys to (e.g. korg → kubsdb).
    pub deploy_to: Vec<String>,
    pub category: Option<String>,
    /// Both columns have existed since 0001 and migration 0013 has advanced
    /// `updated` on every write since #529 — this row just never selected
    /// them, which made projects the last kind whose recency was unreadable
    /// (WI #905). They are `ProjectRow`-only on purpose: the lean
    /// `list_projects` row answers *does this belong here?*, and a timestamp
    /// does not.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// Canonicalize a `src_path` into the form migration 0019's
/// `project_src_path_canonical` constraint enforces: `~`-relative, no trailing
/// slash. The rules and their order mirror that migration's UPDATEs exactly —
/// absolute-home first, so the missing-prefix rule cannot mistake `/home/ken/…`
/// for a relative path (WI #675).
///
/// Deliberately mechanical only. It does **not** strip whitespace or
/// parentheses, because a value carrying prose — korg's own `kcard` row held
/// `~/.archive-src/kcard (kai; archived …, was ~/src/tools/kcard)` — poses a
/// question this function cannot answer: which of the two paths is meant, and
/// where does the history belong? Truncating to the first token would produce a
/// plausible, unverified path, which is worse than a loud failure. Such a value
/// is left to violate the constraint so a human or the project-metadata pass
/// resolves it.
pub fn canonical_src_path(raw: &str) -> String {
    let mut p = raw.trim().to_string();
    if let Some(rest) = p.strip_prefix("/home/") {
        // `/home/<user>/x` -> `~/x`; a bare `/home/<user>` has no tail to keep.
        if let Some((_, tail)) = rest.split_once('/') {
            p = format!("~/{tail}");
        }
    }
    while p.len() > 2 && p.ends_with('/') {
        p.pop();
    }
    if !p.is_empty() && !p.starts_with('~') && !p.starts_with('/') {
        p = format!("~/{p}");
    }
    p
}

/// Reject a `src_path` that migration 0019's `project_src_path_canonical`
/// constraint would reject, before it reaches the constraint (WI #887).
///
/// The CHECK works — nothing bad was ever stored — but it surfaced as
/// `{code: "internal", message: "… violates check constraint
/// \"project_src_path_canonical\""}`: a user-correctable input error at the
/// tier reserved for korg's own faults, documented only by leaked Postgres
/// text. The rules are knowable app-side, so the caller gets the same shape the
/// description cap already gets right — `invalid_input`, naming the rule that
/// broke and what the field wants instead.
///
/// Deliberately validates rather than canonicalizes. [`canonical_src_path`]
/// could mechanically fix a trailing slash or an absolute home path, but the
/// failure worth catching is prose in a path field (korg's own probe was
/// `~/src/tools/korg (dev copy)`), and silently rewriting *some* inputs while
/// rejecting others is a worse contract than rejecting all of them with the
/// form spelled out. The constraint stays as the backstop.
fn check_src_path(value: &str) -> Result<()> {
    // The remedy, quoted identically whichever rule broke: one form to learn.
    const FORM: &str = "`src_path` is the working copy on the project's development machine and \
                        must be a path and nothing else — `~/`-relative, no trailing slash, no \
                        whitespace or parentheses, e.g. `~/src/tools/korg`. Host notes and \
                        history belong in `notes`.";
    let fault = if !value.starts_with("~/") {
        Some(if value.starts_with('/') {
            "it is absolute; write it relative to home"
        } else {
            "it does not start with `~/`"
        })
    } else if value.chars().any(|c| c.is_whitespace()) {
        Some("it contains whitespace")
    } else if value.contains('(') || value.contains(')') {
        Some("it contains parentheses")
    } else if value.ends_with('/') {
        Some("it has a trailing slash")
    } else {
        None
    };
    match fault {
        Some(why) => Err(RepoError::InvalidInput(format!(
            "src_path '{value}' is not canonical: {why}. {FORM}"
        ))
        .into()),
        None => Ok(()),
    }
}

/// Everything but `name` is editable (WI #246). `None` = leave unchanged;
/// inner `None` on the nullable fields clears them.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ProjectPatch {
    #[serde(default, deserialize_with = "ops::double_option")]
    pub gh_repo: Option<Option<String>>,
    /// Path to the working copy on the project's DEVELOPMENT machine (its
    /// `machines` entry), not the deploy target. Canonical form: `~/`-relative,
    /// no trailing slash, no whitespace, no parentheses — a path and nothing
    /// else, e.g. `~/src/tools/korg`. Enforced by a CHECK constraint, so a
    /// value carrying prose or history is rejected rather than stored.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub src_path: Option<Option<String>>,
    /// The routing contract. One line, **≤160 characters** (rejected above
    /// that). First clause says what work belongs here, task-shaped —
    /// "harness conventions and sprint layout", not "a repository containing…".
    /// Where a sibling project plausibly claims the same work, add the
    /// boundary: "Not X — that's `‹project›`." Written for an agent with zero
    /// prior context deciding where a work item goes. No paths, repos, hosts or
    /// build commands — those belong in the structured fields and `notes`.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub description: Option<Option<String>>,
    /// Long-form operational context — deploy topology, build commands, house
    /// conventions. Unbounded, and never rendered into a routing view. Use it
    /// for anything that would otherwise bloat `description` past its cap.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::project_status")]
    pub status: Option<String>,
    #[serde(default)]
    pub machines: Option<Vec<String>>,
    #[serde(default)]
    pub deploy_to: Option<Vec<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    #[schemars(schema_with = "schema::project_category")]
    pub category: Option<Option<String>>,
}

/// The routing contract's only mechanically-enforceable clause (WI #828). The
/// rest of it — task-shaped first clause, sibling boundary — is prose, checked
/// by the project-metadata drift check rather than by a constraint.
pub const PROJECT_DESCRIPTION_MAX: usize = 160;

pub async fn update_project(pool: &PgPool, id: i64, patch: &ProjectPatch) -> Result<ProjectRow> {
    if let Some(v) = &patch.status {
        validate_status(v, &PROJECT_STATUSES, "project status")?;
    }
    // Checked here as well as by the CHECK constraint so the caller gets a
    // `invalid_input` naming the field and the overage, rather than a raw
    // constraint violation surfacing as `internal`. Counted in characters, not
    // bytes — the constraint uses char_length, and a description is prose.
    if let Some(Some(v)) = &patch.description {
        let n = v.chars().count();
        if n > PROJECT_DESCRIPTION_MAX {
            return Err(RepoError::InvalidInput(format!(
                "project description is {n} characters; the routing contract caps it at \
                 {PROJECT_DESCRIPTION_MAX}. Put the long form in `notes` — it is unbounded, \
                 and that is what it exists for."
            ))
            .into());
        }
    }
    // Same reasoning one field over (WI #887): the CHECK constraint would catch
    // this, but as an `internal` carrying raw Postgres text. Clearing it stays
    // legal — NULL satisfies the constraint and means "not recorded".
    if let Some(Some(v)) = &patch.src_path {
        check_src_path(v)?;
    }
    // Setting a category validates against the closed vocabulary (WI #678);
    // clearing it (inner None) stays legal, since `create_project` takes only a
    // name and every project therefore starts uncategorised.
    if let Some(Some(v)) = &patch.category {
        validate_status(v, &PROJECT_CATEGORIES, "project category")?;
    }
    let mut tx = pool.begin().await?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM project WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        return Err(RepoError::NotFound(format!("no project with id {id}")).into());
    }
    if let Some(v) = &patch.gh_repo {
        sqlx::query("UPDATE project SET gh_repo = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.src_path {
        sqlx::query("UPDATE project SET src_path = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.description {
        sqlx::query("UPDATE project SET description = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.notes {
        sqlx::query("UPDATE project SET notes = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.status {
        sqlx::query("UPDATE project SET status = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.machines {
        sqlx::query("UPDATE project SET machines = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.deploy_to {
        sqlx::query("UPDATE project SET deploy_to = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.category {
        sqlx::query("UPDATE project SET category = $2 WHERE id = $1")
            .bind(id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    get_project(pool, id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no project with id {id}")).into())
}

/// Name-keyed wrapper (the REST/MCP surfaces key projects by name; the
/// name itself is immutable — see WI #246).
pub async fn update_project_by_name(
    pool: &PgPool,
    name: &str,
    patch: &ProjectPatch,
) -> Result<ProjectRow> {
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM project WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    match id {
        Some(id) => update_project(pool, id, patch).await,
        None => Err(RepoError::NotFound(format!("no project named '{name}'")).into()),
    }
}

const PROJECT_SELECT: &str = "SELECT id, name, gh_repo, src_path, description, notes, status, \
                              machines, deploy_to, category, created, updated \
                              FROM project";

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectRow>> {
    let rows = sqlx::query_as::<_, ProjectRow>(&format!("{PROJECT_SELECT} ORDER BY name"))
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get_project(pool: &PgPool, id: i64) -> Result<Option<ProjectRow>> {
    Ok(
        sqlx::query_as::<_, ProjectRow>(&format!("{PROJECT_SELECT} WHERE id = $1"))
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

// --- projects: the tiered read surface (WI #828) ---------------------------
//
// Three consumers ask three different questions, and one no-arg 9-column tool
// answered none of them well: the always-on roster wants names, a routing agent
// wants "does this belong here?", and a maintenance pass wants everything.
// `list_projects` now answers the middle question by default and the third on
// request; `get_project` answers it for one project.

/// A project as the lean `list_projects` reports it: the fields that answer
/// *does this belong here?*, and nothing that answers *where does it live*.
///
/// `id`, `gh_repo`, `src_path`, `machines`, `deploy_to` and `category` are
/// omitted deliberately. Dropping six of ten columns across the corpus is the
/// dominant saving — far larger than the row filter — and every field left is
/// routing signal.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProjectLeanRow {
    pub name: String,
    pub description: Option<String>,
    /// Omitted entirely when `active`. In the default view every row is active,
    /// so printing it would be pure noise; under `status:"all"` it is the only
    /// thing distinguishing a live project from a dead one, which is exactly
    /// when an agent must not confuse them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// What the status filter hid. Returned on every list so a lean view can never
/// masquerade as the whole corpus — an agent that finds no match must be able
/// to see there is an escape hatch rather than conclude "no such project
/// exists". Silent truncation is the precise failure this surface exists to
/// treat, so this is a deliberate deviation from the bare-array convention.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProjectOmitted {
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProjectListLean {
    pub items: Vec<ProjectLeanRow>,
    pub omitted: ProjectOmitted,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProjectListFull {
    pub items: Vec<ProjectRow>,
    pub omitted: ProjectOmitted,
}

/// One project plus the areas under it (WI #828).
///
/// The field review specified "full row + inline comments, the `get_work_item`
/// pattern". Comments are not available here and the difference is structural,
/// not an omission: `comment.card_node_id` references `node(id)`, and a project
/// is not a node — `node.project_id` points *at* projects. Inlining comments
/// would mean making `project` a node kind, which is a different and much
/// larger change than this WI.
///
/// Areas are the faithful adaptation. The intent of that decision was that a
/// focused read commits to the full state of one project so the caller does not
/// need a second round-trip; for a project the second call an agent actually
/// makes is `list_areas`, so that is what is inlined.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProjectDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub project: ProjectRow,
    pub areas: Vec<AreaRow>,
}

/// Resolve the `status` argument into a SQL predicate value.
///
/// Absent → `active`. The principle from the field review, applied to the
/// corpus as it now stands: *the default must include every row that could be a
/// correct routing answer.* An archived project is never one — its work belongs
/// to whatever superseded it — and those rows are precisely the confident-wrong
/// -route trap (`ansible-k`, "Homelab configuration", archived, sitting beside
/// an active `k-homelab`).
///
/// `Some("all")` → no filter. Anything else is validated against
/// `PROJECT_STATUSES`.
fn project_status_predicate(status: Option<&str>) -> Result<Option<String>> {
    match status {
        None => Ok(Some("active".to_string())),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &PROJECT_STATUSES, "project status")?;
            Ok(Some(s.to_string()))
        }
    }
}

async fn count_hidden(pool: &PgPool, keep: Option<&str>) -> Result<i64> {
    let Some(keep) = keep else { return Ok(0) };
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM project WHERE status <> $1")
            .bind(keep)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn list_projects_lean(pool: &PgPool, status: Option<&str>) -> Result<ProjectListLean> {
    let keep = project_status_predicate(status)?;
    let rows: Vec<(String, Option<String>, String)> = match &keep {
        Some(s) => {
            sqlx::query_as(
                "SELECT name, description, status FROM project WHERE status = $1 ORDER BY name",
            )
            .bind(s)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as("SELECT name, description, status FROM project ORDER BY name")
                .fetch_all(pool)
                .await?
        }
    };
    let items = rows
        .into_iter()
        .map(|(name, description, status)| ProjectLeanRow {
            name,
            description,
            status: (status != "active").then_some(status),
        })
        .collect();
    Ok(ProjectListLean {
        items,
        omitted: ProjectOmitted {
            archived: count_hidden(pool, keep.as_deref()).await?,
        },
    })
}

pub async fn list_projects_full(pool: &PgPool, status: Option<&str>) -> Result<ProjectListFull> {
    let keep = project_status_predicate(status)?;
    let items = match &keep {
        Some(s) => {
            sqlx::query_as::<_, ProjectRow>(&format!(
                "{PROJECT_SELECT} WHERE status = $1 ORDER BY name"
            ))
            .bind(s)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, ProjectRow>(&format!("{PROJECT_SELECT} ORDER BY name"))
                .fetch_all(pool)
                .await?
        }
    };
    Ok(ProjectListFull {
        items,
        omitted: ProjectOmitted {
            archived: count_hidden(pool, keep.as_deref()).await?,
        },
    })
}

/// Every active project's name, alphabetically — the roster rendered into the
/// MCP `instructions` block at `initialize` (WI #674).
///
/// **Names only, and all of them.** The field review (#757, decision 4) ruled
/// that the roster carries no descriptions: anything in `instructions` is paid
/// by 100% of sessions, while a description only earns its tokens in a routing
/// session, and deciding *which* names are opaque enough to need one is a
/// judgement with no storage home — i.e. a new drift surface.
///
/// Two things in #674's original design fall out once descriptions are gone,
/// and both are dropped deliberately rather than overlooked:
///
///   * **The top-N ranking by trailing work-item count.** Ranking existed to
///     decide which projects were worth spending description tokens on. With
///     names costing ~4 tokens each and 27 active projects fitting the budget
///     whole, there is nothing to ration — and a complete roster cannot cause
///     the misroute that an omitted project can. Alphabetical is also stable
///     across sessions with no window to tune.
///   * **The `category = 'Fun'` exclusion.** Its stated reason was that "a fun
///     project churning for a week shouldn't evict a daily driver" — scarcity
///     of ranked slots. No scarcity, no eviction, and `hv-simulator`, `kapollo`
///     and `mortars` are real projects that receive real work items. Hiding
///     them from the roster would produce exactly the guess this replaces.
///
/// Archived projects are excluded: they are never a correct routing answer.
pub async fn active_project_names(pool: &PgPool) -> Result<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT name FROM project WHERE status = 'active' ORDER BY name")
            .fetch_all(pool)
            .await?,
    )
}

/// Name-keyed focused read. Names are immutable (WI #246), so keying on the
/// name is stable — the same reasoning `update_project_by_name` rests on.
pub async fn get_project_detail(pool: &PgPool, name: &str) -> Result<Option<ProjectDetail>> {
    let Some(project) =
        sqlx::query_as::<_, ProjectRow>(&format!("{PROJECT_SELECT} WHERE name = $1"))
            .bind(name)
            .fetch_optional(pool)
            .await?
    else {
        return Ok(None);
    };
    let areas = list_areas(pool, name).await?;
    Ok(Some(ProjectDetail { project, areas }))
}

// --- projects (write) -----------------------------------------------------

pub async fn create_project(pool: &PgPool, name: &str) -> Result<i64> {
    // Idempotent: return the existing id if the project already exists.
    let id: i64 = sqlx::query(
        "INSERT INTO project (name) VALUES ($1) \
         ON CONFLICT (name) DO UPDATE SET name = project.name RETURNING id",
    )
    .bind(name)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// The project most recently touched via its work items (by node.updated),
/// used as the default landing project for the work-items view.
pub async fn recent_project(pool: &PgPool) -> Result<Option<String>> {
    let row = sqlx::query(
        "SELECT p.name FROM project p \
         JOIN node n ON n.project_id = p.id AND n.kind = 'workitem' \
         GROUP BY p.name ORDER BY max(n.updated) DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.get::<String, _>("name")))
}

pub async fn list_work_items_by_project(pool: &PgPool, project: &str) -> Result<Vec<WorkItemRow>> {
    let sql = format!("{WORKITEM_SELECT} WHERE pj.name = $1 ORDER BY w.wi_number");
    Ok(sqlx::query_as::<_, WorkItemRow>(&sql)
        .bind(project)
        .fetch_all(pool)
        .await?)
}

// --- cards (update: move + rank in one) -----------------------------------

/// `update_card` / `PATCH /api/cards/:node_id`. Projects are addressed by
/// `project_id` on both transports (WI #537).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct CardPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::card_status")]
    pub status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Option<Decimal>,
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project_id: Option<Option<i64>>,
    /// Project name — the alternative to `project_id`; null unassigns. Never pass both.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub category: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

pub async fn update_card(pool: &PgPool, node_id: i64, patch: CardPatch) -> Result<CardRow> {
    if let Some(status) = &patch.status {
        validate_status(status, &CARD_STATUSES, "card status")?;
    }
    let project_id = resolve_project_patch(pool, patch.project_id, patch.project).await?;
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "card", "card").await?;
    if let Some(status) = &patch.status {
        sqlx::query("UPDATE card SET status = $2::card_status WHERE node_id = $1")
            .bind(node_id)
            .bind(status)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(rank) = patch.rank {
        sqlx::query("UPDATE card SET rank = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(rank)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(title) = &patch.title {
        sqlx::query("UPDATE card SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(title)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(description) = &patch.description {
        sqlx::query("UPDATE card SET description = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(description)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(archived) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(archived)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(project_id) = project_id {
        sqlx::query("UPDATE node SET project_id = $2 WHERE id = $1")
            .bind(node_id)
            .bind(project_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(category) = &patch.category {
        sqlx::query("UPDATE node SET category = $2 WHERE id = $1")
            .bind(node_id)
            .bind(category)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tags) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(tags)
            .execute(&mut *tx)
            .await?;
    }
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_card(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no card with node_id {node_id}")).into())
}

// --- comments -------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Comment {
    pub id: i64,
    pub node_id: i64,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// Comments are node-scoped: any node (work item, card, …) can carry comments.
pub async fn list_comments(pool: &PgPool, node_id: i64) -> Result<Vec<Comment>> {
    let rows = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn add_comment(pool: &PgPool, node_id: i64, body: &str) -> Result<Comment> {
    require_non_empty(body, "comment body")?;
    require_node(pool, node_id).await?;
    let c = sqlx::query_as::<_, Comment>(
        "INSERT INTO comment (node_id, body) VALUES ($1, $2) \
         RETURNING id, node_id, body, created, updated",
    )
    .bind(node_id)
    .bind(body)
    .fetch_one(pool)
    .await?;
    Ok(c)
}

/// Edit a comment's body (WI #232). The `updated` column advances via the
/// standard trigger; `created` is preserved.
pub async fn update_comment(pool: &PgPool, id: i64, body: &str) -> Result<Comment> {
    require_non_empty(body, "comment body")?;
    let c = sqlx::query_as::<_, Comment>(
        "UPDATE comment SET body = $2 WHERE id = $1 \
         RETURNING id, node_id, body, created, updated",
    )
    .bind(id)
    .bind(body)
    .fetch_optional(pool)
    .await?;
    c.ok_or_else(|| RepoError::NotFound(format!("no comment with id {id}")).into())
}

/// Delete a comment; `false` means there was no such comment (WI #525).
pub async fn delete_comment(pool: &PgPool, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM comment WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

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

// --- sprint proposals (agent planning) -------------------------------------

/// The routing contract's only mechanically-enforceable clause (WI #860), the
/// proposal counterpart of [`PROJECT_DESCRIPTION_MAX`].
///
/// 500 rather than projects' 160 because a proposal has more to say than a
/// routing line: what the bundle is, why now, roughly how big. Sized in 036's
/// D-1 against what the *full* row carries — after #852 the lean queue row has
/// no `summary` at all, so this is not sizing the pick.
pub const PROPOSAL_SUMMARY_MAX: usize = 500;

/// Checked here as well as by `sprint_proposal_summary_routing_line` (0021) so
/// an over-long summary is `invalid_input` naming the field and the overage,
/// rather than a raw constraint violation surfacing as `internal`. Characters,
/// not bytes — the constraint uses `char_length` and a summary is prose.
fn check_proposal_summary(summary: &str) -> Result<()> {
    let n = summary.chars().count();
    if n > PROPOSAL_SUMMARY_MAX {
        return Err(RepoError::InvalidInput(format!(
            "proposal summary is {n} characters; the routing contract caps it at \
             {PROPOSAL_SUMMARY_MAX}. Put the analysis in `notes` — it is unbounded, \
             and that is what it exists for."
        ))
        .into());
    }
    Ok(())
}

/// `propose_sprint` / `POST /api/proposals`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewProposal {
    /// **Required** — as either this or `project`, never both. A proposal is a
    /// single-project bundle (#967): it is what tells a session which repo to
    /// branch in, and what `covers` is validated against.
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both, and **one of them is required**. Resolved by exact name, and an
    /// unknown name returns `not_found` rather than mis-filing, so pass a name
    /// you are confident in directly. Call `list_projects` only when the name is
    /// genuinely unknown or ambiguous — the roster in this server's
    /// instructions already names every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    /// The routing contract. **≤500 characters** (rejected above that): what
    /// this bundle is, why now, roughly how big. Written for a session choosing
    /// a sprint from a queue, not for one that has already chosen it.
    pub summary: String,
    /// The analysis — measurements, dependencies, sequencing, alternatives
    /// considered. Unbounded, and the home for everything that will not fit
    /// `summary`. Returned by `get_proposal` and `list_proposals detail:"full"`,
    /// never by the lean queue row.
    #[serde(default)]
    pub notes: Option<String>,
    /// Drag-order position; lower sorts first among unpinned proposals.
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Decimal,
    #[serde(default)]
    pub pinned: bool,
    /// wi_numbers this proposal covers; numbers that don't resolve are dropped.
    /// A number that resolves to a work item in a **different project** is not
    /// dropped — it is `invalid_input` naming both projects (#967), because
    /// silently omitting the item you meant to include is the failure this rule
    /// exists to stop.
    #[serde(default, rename = "work_item_numbers")]
    #[schemars(rename = "work_item_numbers", schema_with = "schema::wi_numbers")]
    pub covers: Vec<i64>,
}

/// The created proposal plus which of the requested wi_numbers resolved.
/// `covered` is the honest echo of a drop-and-report input (F-06): compare it
/// against what you asked for.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalCreated {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: ProposalRow,
    pub covered: Vec<i64>,
}

/// Create a sprint proposal and its `covers` edges to the given work items in
/// one transaction. Mirrors `create_work_item`'s node+detail insert; the
/// wi_number -> node_id resolution happens before the transaction, matching
/// `update_work_item`'s handling of `parent`.
pub async fn create_proposal(pool: &PgPool, new: NewProposal) -> Result<ProposalCreated> {
    check_proposal_summary(&new.summary)?;
    // Sprint 043 (#967): a proposal without a project is what produced almost
    // every cross-project `covers` edge in the corpus — the edges had nothing
    // to be validated against. `resolve_project` returns None for "neither
    // selector given"; for a proposal that is now invalid_input rather than a
    // row nobody can route.
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref())
        .await?
        .ok_or_else(|| {
            RepoError::InvalidInput(
                "a sprint proposal needs a project — pass `project` (the name, e.g. \
                 \"korg\") or `project_id`. A proposal is a single-project bundle: it is \
                 what tells a session which repo to branch in, and what the covered work \
                 items are checked against. list_projects has the roster."
                    .into(),
            )
        })?;
    let project_name = project_name_for_id(pool, project_id).await?;

    let mut covered = Vec::with_capacity(new.covers.len());
    for wi in &new.covers {
        let Some(n) = node_id_for_wi(pool, *wi).await? else {
            continue;
        };
        // The bundled insert below does not go through `relate`, so the
        // single-project rule is applied here too — and before the transaction
        // opens, so a refusal leaves nothing behind. Unlike an unresolvable
        // wi_number (dropped and reported via `covered`, F-06), this is a hard
        // refusal: silently dropping the work item Ken meant to include is the
        // exact failure #967 exists to stop.
        if let Some(wp) = node_project(pool, n).await? {
            if wp != project_name {
                let (_, title) = wi_handle(pool, n).await?;
                return Err(cross_project_covers(&project_name, *wi, &wp, &title));
            }
        }
        covered.push(n);
    }

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('sprint_proposal', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query(
        "INSERT INTO sprint_proposal (node_id, title, summary, notes, rank, pinned) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(node_id)
    .bind(&new.title)
    .bind(&new.summary)
    .bind(&new.notes)
    .bind(new.rank)
    .bind(new.pinned)
    .execute(&mut *tx)
    .await?;

    // Semantic orientation: proposal -> work item (WI #531). This used to
    // insert (least, greatest), which recorded id ordering instead of meaning.
    // Provenance (D-17): origin is this writer's operation name; the ON CONFLICT
    // no-op preserves created/origin on a re-propose.
    for &target in &covered {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'covers', now(), 'propose_sprint') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(node_id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let row = get_proposal(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no proposal with node_id {node_id}")))?;
    Ok(ProposalCreated { row, covered })
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalRow {
    pub node_id: i64,
    pub title: String,
    /// The routing contract, ≤500 chars (WI #860) — what the bundle is, why
    /// now, roughly how big. It used to hold the whole analysis, which is why
    /// the unfiltered `list_proposals` measured ~46k tokens (#852).
    pub summary: String,
    /// The analysis the summary used to carry. Unbounded, `None` on a proposal
    /// whose summary always fit. Migration 0021 moved every over-cap summary
    /// here verbatim.
    pub notes: Option<String>,
    pub status: String,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub pinned: bool,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this proposal (WI #535).
    pub comment_count: i64,
    /// How many work items this proposal covers (WI #536) — the signal that
    /// saves the Planning page a `neighbors` call per row just to show chips.
    pub covered_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const PROPOSAL_SELECT: &str =
    "SELECT p.node_id, p.title, p.summary, p.notes, p.status::text AS status, p.rank, p.pinned, \
            pj.name AS project, n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = p.node_id) AS comment_count, \
            (SELECT count(*) FROM relationship r JOIN node wn ON wn.id = r.right_id \
              WHERE r.left_id = p.node_id AND r.relationship = 'covers' \
                AND wn.kind = 'workitem') AS covered_count, \
            n.created, n.updated \
     FROM sprint_proposal p \
     JOIN node n ON n.id = p.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

#[derive(Debug, Clone, Default)]
pub struct ProposalQuery {
    pub status: Option<String>,
    /// Project name (WI #565) — the queue spans repos, so "show me korg's
    /// sprints" is the common ask.
    pub project: Option<String>,
}

/// List proposals ordered pinned-first, then by rank — the drag-order a user
/// or agent leaves them in — with a `node_id` tie-breaker so equal ranks stop
/// shuffling between calls (F-19).
pub async fn list_proposals(pool: &PgPool, query: ProposalQuery) -> Result<Vec<ProposalRow>> {
    if let Some(status) = &query.status {
        validate_status(status, &PROPOSAL_STATUSES, "proposal status")?;
    }
    let rows = sqlx::query_as::<_, ProposalRow>(&format!(
        "{PROPOSAL_SELECT} WHERE ($1::text IS NULL OR p.status::text = $1) \
           AND ($2::text IS NULL OR pj.name = $2) \
         ORDER BY p.pinned DESC, p.rank ASC, p.node_id ASC"
    ))
    .bind(query.status.as_deref())
    .bind(query.project.as_deref())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// --- the lean proposal queue (WI #852) --------------------------------------
//
// `list_proposals` above is the REST/web read and stays as it is: the Planning
// page fetches unfiltered on purpose (WI #622) and renders `done` and
// `declined` columns out of the summaries. It is a browser, not a token budget.
//
// The MCP read is the one that measured 110 rows / ~185,500 chars / ~46k tokens
// unfiltered, 71% of it `done`. Everything below is that read: a lean
// projection, live statuses by default, and an `omitted` envelope so the
// narrowing is visible. Same split `list_projects` has had since #828.

/// A proposal as the lean queue read reports it — WI #852's field list exactly.
///
/// No `summary`: proposal summaries are the longest prose in korg (they are
/// written as plans) and they are the entire payload problem. `get_proposal` is
/// the full read, and `start-sprint` calls it immediately after the pick anyway
/// — the two-tier read this row completes was already the intended pattern.
///
/// No `archived` either: the lean list excludes archived rows by default, so the
/// flag would be constant. Ask for them and `detail: "full"` carries it.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalLeanRow {
    pub node_id: i64,
    pub title: String,
    pub status: String,
    pub project: Option<String>,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub pinned: bool,
    pub covered_count: i64,
    pub comment_count: i64,
}

/// What the queue read's filters hid (WI #852), computed as a **cascade** so no
/// row is counted twice: `archived` is what the archived filter excluded, and
/// `done`/`declined` are counted only over the rows that *passed* it. An
/// archived `done` proposal therefore lands in `archived` and nowhere else.
///
/// A field is 0 when the caller asked to see that class — `status: "all"` zeroes
/// both terminal counts, `archived: null` zeroes `archived`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalOmitted {
    pub done: i64,
    pub declined: i64,
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalListLean {
    pub items: Vec<ProposalLeanRow>,
    pub omitted: ProposalOmitted,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalListFull {
    pub items: Vec<ProposalRow>,
    pub omitted: ProposalOmitted,
}

/// Resolve the `status` argument into the statuses a queue read returns.
///
/// Absent → the live queue (`proposed` + `active`). The same principle #828
/// applied to projects: *the default must include every row that could be a
/// correct answer to the question the tool is for.* The question here is "what
/// should I work on", and a `done` proposal is never that.
///
/// `Some("all")` → no filter. Anything else is validated against
/// `PROPOSAL_STATUSES` and returned alone.
fn proposal_status_predicate(status: Option<&str>) -> Result<Option<Vec<String>>> {
    match status {
        None => Ok(Some(
            PROPOSAL_LIVE_STATUSES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &PROPOSAL_STATUSES, "proposal status")?;
            Ok(Some(vec![s.to_string()]))
        }
    }
}

const PROPOSAL_LEAN_SELECT: &str =
    "SELECT p.node_id, p.title, p.status::text AS status, p.rank, p.pinned, \
            pj.name AS project, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = p.node_id) AS comment_count, \
            (SELECT count(*) FROM relationship r JOIN node wn ON wn.id = r.right_id \
              WHERE r.left_id = p.node_id AND r.relationship = 'covers' \
                AND wn.kind = 'workitem') AS covered_count \
     FROM sprint_proposal p \
     JOIN node n ON n.id = p.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

/// The shared tail of both queue reads: status set, project, archived, ordering.
const PROPOSAL_QUEUE_WHERE: &str = " WHERE ($1::text[] IS NULL OR p.status::text = ANY($1)) \
        AND ($2::text IS NULL OR pj.name = $2) \
        AND ($3::bool IS NULL OR n.archived = $3) \
      ORDER BY p.pinned DESC, p.rank ASC, p.node_id ASC";

/// Count what the queue read's filters hid, in one round trip.
///
/// The counts are taken over the `project`-filtered corpus — narrowing to a
/// project is the caller *choosing* a scope, not a default hiding rows from
/// them, so it is not something `omitted` should report.
async fn proposal_omitted(
    pool: &PgPool,
    project: Option<&str>,
    archived: ArchivedFilter,
    shown: Option<&[String]>,
) -> Result<ProposalOmitted> {
    let (archived_hidden, done, declined) = sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT \
           count(*) FILTER (WHERE n.archived AND $2::bool IS NOT NULL AND NOT $2), \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND p.status::text = 'done'), \
           count(*) FILTER (WHERE ($2::bool IS NULL OR n.archived = $2) \
                              AND p.status::text = 'declined') \
         FROM sprint_proposal p \
         JOIN node n ON n.id = p.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1)",
    )
    .bind(project)
    .bind(archived)
    .fetch_one(pool)
    .await?;
    // A status the caller asked for isn't omitted, however many rows it has.
    let hidden = |status: &str, count: i64| match shown {
        Some(list) if list.iter().any(|s| s == status) => 0,
        None => 0,
        _ => count,
    };
    Ok(ProposalOmitted {
        done: hidden("done", done),
        declined: hidden("declined", declined),
        archived: archived_hidden,
    })
}

/// `list_proposals` over MCP: the lean projection, live statuses and unarchived
/// rows by default, with `omitted` saying what that hid.
pub async fn list_proposals_lean(
    pool: &PgPool,
    status: Option<&str>,
    project: Option<&str>,
    archived: ArchivedFilter,
) -> Result<ProposalListLean> {
    let shown = proposal_status_predicate(status)?;
    let items = sqlx::query_as::<_, ProposalLeanRow>(&format!(
        "{PROPOSAL_LEAN_SELECT}{PROPOSAL_QUEUE_WHERE}"
    ))
    .bind(shown.as_deref())
    .bind(project)
    .bind(archived)
    .fetch_all(pool)
    .await?;
    Ok(ProposalListLean {
        items,
        omitted: proposal_omitted(pool, project, archived, shown.as_deref()).await?,
    })
}

/// `list_proposals` with `detail: "full"` — the same filters and envelope, but
/// every column including `summary`. The escape hatch that keeps the lean
/// default from making anything unreachable.
pub async fn list_proposals_full(
    pool: &PgPool,
    status: Option<&str>,
    project: Option<&str>,
    archived: ArchivedFilter,
) -> Result<ProposalListFull> {
    let shown = proposal_status_predicate(status)?;
    let items =
        sqlx::query_as::<_, ProposalRow>(&format!("{PROPOSAL_SELECT}{PROPOSAL_QUEUE_WHERE}"))
            .bind(shown.as_deref())
            .bind(project)
            .bind(archived)
            .fetch_all(pool)
            .await?;
    Ok(ProposalListFull {
        items,
        omitted: proposal_omitted(pool, project, archived, shown.as_deref()).await?,
    })
}

/// A covered work item as a proposal's detail read reports it — enough to
/// decide and to render, without a second call per item (§4.3).
#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct CoveredRef {
    pub wi_number: i64,
    pub node_id: i64,
    pub title: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub project: Option<String>,
    pub comment_count: i64,
}

/// A proposal plus what it covers and its discussion (WI #536). This is the
/// authoritative "what is this sprint" read: before it, the Planning page
/// fetched every proposal, every work item, then called `neighbors` once per
/// proposal and joined client-side, and `start-sprint` did the same dance over
/// MCP in three tools.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProposalDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub proposal: ProposalRow,
    /// Covered work items, ordered by wi_number.
    pub covered: Vec<CoveredRef>,
    pub comments: Vec<Comment>,
    pub comments_truncated: bool,
    /// The proposal's non-`covers` edges, inlined (LB-3). `covers` is excluded
    /// because `covered` already carries it.
    pub related: Vec<RelatedRef>,
    /// True when there are more such edges than were inlined (call `neighbors`).
    pub related_truncated: bool,
}

/// `get_proposal` — the proposal, its covered work items, and capped comments.
/// `None` if no proposal has that node id (the transports turn that into
/// 404 / isError per D-6).
pub async fn get_proposal_detail(pool: &PgPool, node_id: i64) -> Result<Option<ProposalDetail>> {
    let Some(proposal) = get_proposal(pool, node_id).await? else {
        return Ok(None);
    };
    // Reads the `covers` edge in its semantic orientation (proposal -> work
    // item), which sprint 014 made trustworthy.
    let covered = sqlx::query_as::<_, CoveredRef>(
        "SELECT w.wi_number, w.node_id, w.title, w.wi_status, w.wi_tshirt, \
                pj.name AS project, \
                (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count \
         FROM relationship r \
         JOIN workitem w ON w.node_id = r.right_id \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE r.left_id = $1 AND r.relationship = 'covers' \
         ORDER BY w.wi_number",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created LIMIT $2",
    )
    .bind(node_id)
    .bind(WORKITEM_COMMENT_CAP)
    .fetch_all(pool)
    .await?;
    let comments_truncated = proposal.comment_count > WORKITEM_COMMENT_CAP;
    // Everything except covers — that is already inlined as `covered`.
    let (related, related_truncated) = related_context(pool, node_id, Some("covers")).await?;
    Ok(Some(ProposalDetail {
        proposal,
        covered,
        comments,
        comments_truncated,
        related,
        related_truncated,
    }))
}

pub async fn get_proposal(pool: &PgPool, node_id: i64) -> Result<Option<ProposalRow>> {
    Ok(
        sqlx::query_as::<_, ProposalRow>(&format!("{PROPOSAL_SELECT} WHERE p.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// `update_proposal` / `PATCH /api/proposals/:node_id`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ProposalPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    /// The routing contract, **≤500 characters** (rejected above that). Put the
    /// analysis in `notes` — it is unbounded, and that is what it is for.
    #[serde(default)]
    pub summary: Option<String>,
    /// The analysis: measurements, dependencies, sequencing, what was
    /// considered and rejected. Unbounded; pass `null` to clear it.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::proposal_status")]
    pub status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Option<Decimal>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

/// Partially update a proposal: status transitions (propose -> active ->
/// done/declined), reorder (rank), pin, archive. Same "only bind what's
/// present" shape as `update_card`.
pub async fn update_proposal(
    pool: &PgPool,
    node_id: i64,
    patch: ProposalPatch,
) -> Result<ProposalRow> {
    if let Some(v) = &patch.status {
        validate_status(v, &PROPOSAL_STATUSES, "proposal status")?;
    }
    if let Some(v) = &patch.summary {
        check_proposal_summary(v)?;
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "sprint_proposal", "proposal").await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE sprint_proposal SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.summary {
        sqlx::query("UPDATE sprint_proposal SET summary = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    // Double-option: absent leaves `notes` alone, explicit null clears it.
    if let Some(v) = &patch.notes {
        sqlx::query("UPDATE sprint_proposal SET notes = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v.as_deref())
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.status {
        sqlx::query(
            "UPDATE sprint_proposal SET status = $2::sprint_proposal_status WHERE node_id = $1",
        )
        .bind(node_id)
        .bind(v)
        .execute(&mut *tx)
        .await?;
    }
    if let Some(v) = patch.rank {
        sqlx::query("UPDATE sprint_proposal SET rank = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.pinned {
        sqlx::query("UPDATE sprint_proposal SET pinned = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    // D-7: `done`/`declined` are Ken's calls on the bundle, so an awaiting
    // marker on this proposal has been answered by the transition itself.
    settle_awaiting(&mut *tx, node_id).await?;
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_proposal(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no proposal with node_id {node_id}")).into())
}

// === programs: the multi-project layer (#968, sprint 044) ===================
//
// 0022 made a proposal single-project and enforced it. That was only half an
// answer: work that genuinely spans repos had nowhere legal to live, and the
// corpus showed it — 13 legacy proposals covering work across projects, 4 of
// them live in the queue, each one filed under whichever project the writer
// picked first. A program is where that work goes.
//
// A program `includes` proposals, ordered (the edge carries the order, D-2), and
// carries NO project of its own (D-6): its span is derived from its slices, so
// the fact has one home and cannot go stale when a slice is added.

/// `create_program` / `POST /api/programs`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewProgram {
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    /// The routing contract — what this program is trying to achieve, across
    /// which repos, and how you will know it is done. Same job a proposal's
    /// `summary` does one level down.
    pub aim: String,
    /// The analysis: sequencing, what each slice is waiting on, what was
    /// considered and rejected. Unbounded.
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Decimal,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    /// Proposal `node_id`s this program includes, **in the order given** — the
    /// first is slice 1. Ids that do not resolve to a proposal are refused, not
    /// dropped: a program's slice list is its plan, and a silently missing slice
    /// is a plan that lies.
    #[serde(default)]
    #[schemars(schema_with = "schema::node_ids")]
    pub slices: Vec<i64>,
    /// Present **only to be refused** (D-6). A program is the cross-project
    /// layer; filing one under a single project rebuilds exactly the mis-routing
    /// #967 cured for proposals. Passing either selector is `invalid_input`
    /// naming the rule, because a silent drop would leave the caller believing
    /// the program is filed somewhere it is not.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub project_id: Option<i64>,
}

/// The created program plus the slice ids that were linked, in order.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramCreated {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: ProgramRow,
    pub slices: Vec<i64>,
}

/// A program row. No `project` field — see [`NewProgram::project`] and the
/// `node_program_has_no_project` constraint; `span` is the derived answer to
/// "which repos does this touch".
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramRow {
    pub node_id: i64,
    pub title: String,
    pub aim: String,
    pub notes: Option<String>,
    pub status: String,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub pinned: bool,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    pub comment_count: i64,
    /// How many proposals this program includes.
    pub slice_count: i64,
    /// **Derived** (D-6): the distinct project names of the included proposals,
    /// alphabetical. Empty until the program has slices. This is the honest
    /// answer to "which repos does this touch" — it cannot drift from the
    /// slices the way a stored `project_id` would.
    pub span: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const PROGRAM_SELECT: &str =
    "SELECT g.node_id, g.title, g.aim, g.notes, g.status, g.rank, g.pinned, \
            n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = g.node_id) AS comment_count, \
            (SELECT count(*) FROM relationship r JOIN node sn ON sn.id = r.right_id \
              WHERE r.left_id = g.node_id AND r.relationship = 'includes' \
                AND sn.kind = 'sprint_proposal') AS slice_count, \
            (SELECT coalesce(array_agg(DISTINCT pj.name), '{}') \
               FROM relationship r JOIN node sn ON sn.id = r.right_id \
               JOIN project pj ON pj.id = sn.project_id \
              WHERE r.left_id = g.node_id AND r.relationship = 'includes') AS span, \
            n.created, n.updated \
     FROM program g \
     JOIN node n ON n.id = g.node_id";

/// Create a program and its ordered `includes` edges in one transaction.
///
/// Slice order is written as `relationship.rank` = the index in `slices`, so the
/// caller's order *is* the program's order without a second call (D-2).
pub async fn create_program(pool: &PgPool, new: NewProgram) -> Result<ProgramCreated> {
    if new.project.is_some() || new.project_id.is_some() {
        return Err(RepoError::InvalidInput(
            "a program does not take a project — it is the CROSS-project layer (#968). \
             A program filed under one project rebuilds the mis-routing single-project \
             proposals were just cured of. Its span is derived from the projects of the \
             proposals it includes, so add the slices and the span follows."
                .into(),
        )
        .into());
    }
    check_program_aim(&new.aim)?;

    // Resolved before the transaction opens, so a bad id leaves nothing behind
    // — same shape as create_proposal's wi_number resolution. Unlike a proposal's
    // unresolvable wi_number (dropped and echoed via `covered`, F-06), this is a
    // hard refusal: a program's slices are its plan.
    for id in &new.slices {
        let kind = node_kind(pool, *id).await?;
        if kind != "sprint_proposal" {
            return Err(RepoError::InvalidInput(format!(
                "slice {id} is a {kind}, not a sprint_proposal — a program includes \
                 proposals (each single-project), which is what makes it the layer where \
                 cross-project work is legal"
            ))
            .into());
        }
    }

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('program', NULL, $1, $2) RETURNING id",
    )
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query(
        "INSERT INTO program (node_id, title, aim, notes, rank, pinned) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(node_id)
    .bind(&new.title)
    .bind(&new.aim)
    .bind(&new.notes)
    .bind(new.rank)
    .bind(new.pinned)
    .execute(&mut *tx)
    .await?;

    for (i, &target) in new.slices.iter().enumerate() {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, rank, created, origin) \
             VALUES ($1, $2, 'includes', $3, now(), 'create_program') \
             ON CONFLICT (left_id, right_id, relationship) \
             DO UPDATE SET rank = EXCLUDED.rank",
        )
        .bind(node_id)
        .bind(target)
        .bind(Decimal::from(i as i64))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let row = get_program(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no program with node_id {node_id}")))?;
    Ok(ProgramCreated {
        row,
        slices: new.slices,
    })
}

/// A program's `aim` is its routing contract, capped like a proposal's
/// `summary` (0021/#860) and for the same reason: it is what a reader sees in a
/// list, and an unbounded one turns the list back into the payload problem
/// #852 measured. The analysis goes in `notes`.
fn check_program_aim(aim: &str) -> Result<()> {
    const CAP: usize = 500;
    if aim.chars().count() > CAP {
        return Err(RepoError::InvalidInput(format!(
            "a program's `aim` is a routing contract capped at {CAP} characters (got {}). \
             Put the analysis — sequencing, what each slice waits on, what was rejected — \
             in `notes`, which is unbounded.",
            aim.chars().count()
        ))
        .into());
    }
    Ok(())
}

pub async fn get_program(pool: &PgPool, node_id: i64) -> Result<Option<ProgramRow>> {
    Ok(
        sqlx::query_as::<_, ProgramRow>(&format!("{PROGRAM_SELECT} WHERE g.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// One slice of a program, with the rollup that stops a consumer crawling
/// (D-5). `open`/`resolved`/`done`/`closed` count the proposal's covered work
/// items, so a board renders progress from `get_program` alone.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramSlice {
    pub node_id: i64,
    pub title: String,
    pub status: String,
    /// The slice's project. Always present — a proposal is single-project since
    /// 0022 — and the union of these is the program's `span`.
    pub project: Option<String>,
    /// Position within this program. `None` on a slice linked by a bare
    /// `relate` without a rank; those sort last.
    #[ts(type = "string | null")]
    pub rank: Option<Decimal>,
    pub covered_count: i64,
    pub open: i64,
    pub resolved: i64,
    pub done: i64,
    pub closed: i64,
}

/// A program, its ordered slices with per-slice rollups, its comments, and its
/// other edges. The read a consumer makes instead of crawling
/// program → proposals → work items.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramDetail {
    #[serde(flatten)]
    #[ts(flatten)]
    pub program: ProgramRow,
    /// Included proposals, in program order (`rank`, then node_id).
    pub slices: Vec<ProgramSlice>,
    pub comments: Vec<Comment>,
    pub comments_truncated: bool,
    /// The program's non-`includes` edges, inlined (LB-3) — `slices` already
    /// carries `includes`. A program is precisely the kind that accrues a
    /// handoff, so this is where a `has_handoff` ref surfaces.
    pub related: Vec<RelatedRef>,
    pub related_truncated: bool,
}

/// `get_program` — the D-5 rollup read.
pub async fn get_program_detail(pool: &PgPool, node_id: i64) -> Result<Option<ProgramDetail>> {
    let Some(program) = get_program(pool, node_id).await? else {
        return Ok(None);
    };
    // One query for every slice and its work-item rollup: the whole point is
    // that a consumer never crawls, so this must not become N+1 either.
    let slices = sqlx::query_as::<_, ProgramSlice>(
        "SELECT sp.node_id, sp.title, sp.status::text AS status, pj.name AS project, r.rank, \
                count(w.node_id)                                       AS covered_count, \
                count(w.node_id) FILTER (WHERE w.wi_status = 'open')     AS open, \
                count(w.node_id) FILTER (WHERE w.wi_status = 'resolved') AS resolved, \
                count(w.node_id) FILTER (WHERE w.wi_status = 'done')     AS done, \
                count(w.node_id) FILTER (WHERE w.wi_status = 'closed')   AS closed \
         FROM relationship r \
         JOIN sprint_proposal sp ON sp.node_id = r.right_id \
         JOIN node sn ON sn.id = sp.node_id \
         LEFT JOIN project pj ON pj.id = sn.project_id \
         LEFT JOIN relationship cov ON cov.left_id = sp.node_id AND cov.relationship = 'covers' \
         LEFT JOIN workitem w ON w.node_id = cov.right_id \
         WHERE r.left_id = $1 AND r.relationship = 'includes' \
         GROUP BY sp.node_id, sp.title, sp.status, pj.name, r.rank \
         ORDER BY r.rank ASC NULLS LAST, sp.node_id ASC",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created LIMIT $2",
    )
    .bind(node_id)
    .bind(WORKITEM_COMMENT_CAP)
    .fetch_all(pool)
    .await?;
    let comments_truncated = program.comment_count > WORKITEM_COMMENT_CAP;
    let (related, related_truncated) = related_context(pool, node_id, Some("includes")).await?;
    Ok(Some(ProgramDetail {
        program,
        slices,
        comments,
        comments_truncated,
        related,
        related_truncated,
    }))
}

/// What `list_programs`' defaults hid. Same cascade rule as [`ProposalOmitted`]:
/// `archived` is counted first, and `done` only over the rows that passed it.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramOmitted {
    pub done: i64,
    pub archived: i64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramList {
    pub items: Vec<ProgramRow>,
    pub omitted: ProgramOmitted,
}

/// List programs, live-by-default, pinned first then rank.
///
/// **No lean/full split**, unlike proposals: `list_proposals` needed one because
/// 110 rows of plan-length prose measured ~46k tokens (#852). Programs are the
/// layer *above* proposals — there will be single digits of them — and the
/// `aim` is capped at 500 chars, so the full row is already the lean row. If
/// that stops being true the split is the same three functions proposals have.
pub async fn list_programs(
    pool: &PgPool,
    status: Option<&str>,
    archived: ArchivedFilter,
) -> Result<ProgramList> {
    let shown = program_status_predicate(status)?;
    let items = sqlx::query_as::<_, ProgramRow>(&format!(
        "{PROGRAM_SELECT} WHERE ($1::text[] IS NULL OR g.status = ANY($1)) \
            AND ($2::bool IS NULL OR n.archived = $2) \
          ORDER BY g.pinned DESC, g.rank ASC, g.node_id ASC"
    ))
    .bind(shown.as_deref())
    .bind(archived)
    .fetch_all(pool)
    .await?;

    let (archived_hidden, done) = sqlx::query_as::<_, (i64, i64)>(
        "SELECT count(*) FILTER (WHERE n.archived AND $1::bool IS NOT NULL AND NOT $1), \
                count(*) FILTER (WHERE ($1::bool IS NULL OR n.archived = $1) \
                                   AND g.status = 'done') \
         FROM program g JOIN node n ON n.id = g.node_id",
    )
    .bind(archived)
    .fetch_one(pool)
    .await?;
    let done_hidden = match shown.as_deref() {
        Some(list) if list.iter().any(|s| s == "done") => 0,
        None => 0,
        _ => done,
    };
    Ok(ProgramList {
        items,
        omitted: ProgramOmitted {
            done: done_hidden,
            archived: archived_hidden,
        },
    })
}

/// Absent → the live set (`active` + `holding`); `"all"` → no filter; anything
/// else validated and returned alone. Same contract as
/// [`proposal_status_predicate`].
fn program_status_predicate(status: Option<&str>) -> Result<Option<Vec<String>>> {
    match status {
        None => Ok(Some(
            PROGRAM_LIVE_STATUSES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        )),
        Some("all") => Ok(None),
        Some(s) => {
            validate_status(s, &PROGRAM_STATUSES, "program status")?;
            Ok(Some(vec![s.to_string()]))
        }
    }
}

/// `update_program` / `PATCH /api/programs/:node_id`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ProgramPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    /// The routing contract, **≤500 characters**. Analysis goes in `notes`.
    #[serde(default)]
    pub aim: Option<String>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub notes: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::program_status")]
    pub status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Option<Decimal>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

pub async fn update_program(
    pool: &PgPool,
    node_id: i64,
    patch: ProgramPatch,
) -> Result<ProgramRow> {
    if let Some(v) = &patch.status {
        validate_status(v, &PROGRAM_STATUSES, "program status")?;
    }
    if let Some(v) = &patch.aim {
        check_program_aim(v)?;
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "program", "program").await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE program SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.aim {
        sqlx::query("UPDATE program SET aim = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.notes {
        sqlx::query("UPDATE program SET notes = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v.as_deref())
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.status {
        sqlx::query("UPDATE program SET status = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.rank {
        sqlx::query("UPDATE program SET rank = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.pinned {
        sqlx::query("UPDATE program SET pinned = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    settle_awaiting(&mut *tx, node_id).await?;
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_program(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no program with node_id {node_id}")).into())
}

// === the awaiting-Ken marker (#969, sprint 044) =============================
//
// "This moves only when Ken acts" — the most valuable lane on the kfdc board,
// and until now expressible only as prose in a comment.
//
// Deliberately not a reserved tag, which is the cheaper mechanism #969 guessed
// at. Tags are written wholesale (`UPDATE node SET tags = $2`), so an agent
// editing tags for an unrelated reason silently clears the marker — and 76% of
// nodes carry tags, so that is a hot path. See 0023 §5 and D-3.

/// Set or clear the awaiting-Ken marker on any node.
///
/// `awaiting: false` clears both columns — an agent that raised a question and
/// got its answer mid-session **retracts its own marker**, rather than leaving
/// an answered ask in the lane until Ken clicks it away (D-8).
///
/// Re-asserting on a node already awaiting **preserves `awaiting_since`** and
/// updates only the note: the age is the whole reason the marker is a timestamp,
/// and a re-set that restarted the clock would make a nine-day-old ask look
/// fresh every time an agent touched it.
pub async fn set_awaiting(
    pool: &PgPool,
    node_id: i64,
    awaiting: bool,
    note: Option<&str>,
) -> Result<AwaitingRow> {
    if !awaiting && note.is_some() {
        return Err(RepoError::InvalidInput(
            "`note` is meaningless when clearing — pass awaiting:true with a note to \
             change what is being asked, or awaiting:false alone to clear the marker"
                .into(),
        )
        .into());
    }
    let mut tx = pool.begin().await?;
    // Any kind may await Ken — a work item, a proposal and a program can all be
    // waiting on him — so this is `require_node`, not `require_kind`.
    require_node(&mut *tx, node_id).await?;
    if awaiting {
        // COALESCE preserves the original timestamp on a re-set (D-8).
        sqlx::query(
            "UPDATE node SET awaiting_since = COALESCE(awaiting_since, now()), \
                             awaiting_note  = $2 \
             WHERE id = $1",
        )
        .bind(node_id)
        .bind(note)
        .execute(&mut *tx)
        .await?;
    } else {
        sqlx::query("UPDATE node SET awaiting_since = NULL, awaiting_note = NULL WHERE id = $1")
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    awaiting_row(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no node with id {node_id}")).into())
}

/// One row of the awaiting lane.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct AwaitingRow {
    pub node_id: i64,
    pub kind: String,
    /// Present when the node is a work item — its user-facing handle.
    pub wi_number: Option<i64>,
    /// Resolved across kinds, the way `related_context` resolves it.
    pub title: String,
    pub project: Option<String>,
    /// The node's **own** status, resolved per kind (`wi_status`, a proposal's
    /// or program's status, a card's column). `None` for kinds that have none.
    /// Carried so a board renders state without a follow-up read per row.
    pub status: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    #[ts(type = "string | null")]
    pub awaiting_since: Option<OffsetDateTime>,
    pub awaiting_note: Option<String>,
    pub archived: bool,
}

const AWAITING_SELECT: &str = "SELECT n.id AS node_id, n.kind, w.wi_number, \
            COALESCE(w.title, sp.title, g.title, cd.title, lk.title, lk.url, tp.name, \
                     rp.summary, hd.title, n.kind || ' #' || n.id) AS title, \
            pj.name AS project, \
            COALESCE(w.wi_status, sp.status::text, g.status, cd.status::text) AS status, \
            n.awaiting_since, n.awaiting_note, n.archived \
     FROM node n \
     LEFT JOIN workitem w         ON w.node_id  = n.id \
     LEFT JOIN sprint_proposal sp ON sp.node_id = n.id \
     LEFT JOIN program g          ON g.node_id  = n.id \
     LEFT JOIN card cd            ON cd.node_id = n.id \
     LEFT JOIN link lk            ON lk.node_id = n.id \
     LEFT JOIN topic tp           ON tp.node_id = n.id \
     LEFT JOIN report rp          ON rp.node_id = n.id \
     LEFT JOIN handoff hd         ON hd.node_id = n.id \
     LEFT JOIN project pj         ON pj.id = n.project_id";

async fn awaiting_row(pool: &PgPool, node_id: i64) -> Result<Option<AwaitingRow>> {
    Ok(
        sqlx::query_as::<_, AwaitingRow>(&format!("{AWAITING_SELECT} WHERE n.id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// The Commander's Call lane: everything waiting on Ken, oldest ask first.
///
/// **Ghost-free (D-7).** Archived nodes and nodes in a status only Ken sets
/// (`closed` work items, `done`/`declined` proposals, `done` programs) are
/// filtered out even if their marker somehow survived — the write rules clear
/// it, and this is the belt to that pair of braces. A lane that accumulates
/// answered asks is the failure the whole marker was designed to avoid, one
/// door over.
///
/// Note what is *not* filtered: `resolved` and `done` work items stay. "I
/// implemented it, it needs your user test" is the canonical awaiting-Ken state
/// — filtering those would empty the lane of its best rows.
pub async fn list_awaiting(pool: &PgPool) -> Result<Vec<AwaitingRow>> {
    Ok(sqlx::query_as::<_, AwaitingRow>(&format!(
        "{AWAITING_SELECT} WHERE n.awaiting_since IS NOT NULL \
            AND NOT n.archived \
            AND COALESCE(w.wi_status, 'open') <> 'closed' \
            AND COALESCE(sp.status::text, 'proposed') NOT IN ('done', 'declined') \
            AND COALESCE(g.status, 'active') <> 'done' \
          ORDER BY n.awaiting_since ASC, n.id ASC"
    ))
    .fetch_all(pool)
    .await?)
}

/// Clear the awaiting marker when the node has reached a state **only Ken
/// sets** — because if Ken set it, the ask is answered by definition (D-7).
///
/// The trigger is deliberately *not* "terminal". A `resolved` or `done` work
/// item is the canonical awaiting-Ken state (`vocab` calls `resolved`
/// "implemented; may still need a user test"), so clearing there would delete
/// exactly the rows the lane exists to show. `closed` is different: `vocab`
/// records it as Ken-only.
///
/// Called from every update path rather than a trigger — LB-2 settled that edge
/// and lifecycle rules live in core, the one path both transports share.
async fn settle_awaiting<'e, E>(executor: E, node_id: i64) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        "UPDATE node n SET awaiting_since = NULL, awaiting_note = NULL \
         WHERE n.id = $1 AND n.awaiting_since IS NOT NULL AND ( \
               n.archived \
            OR EXISTS (SELECT 1 FROM workitem w \
                        WHERE w.node_id = n.id AND w.wi_status = 'closed') \
            OR EXISTS (SELECT 1 FROM sprint_proposal sp \
                        WHERE sp.node_id = n.id AND sp.status::text IN ('done', 'declined')) \
            OR EXISTS (SELECT 1 FROM program g \
                        WHERE g.node_id = n.id AND g.status = 'done'))",
    )
    .bind(node_id)
    .execute(executor)
    .await?;
    Ok(())
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

/// Resolve a work item's node id from its user-facing wi_number.
pub async fn node_id_for_wi(pool: &PgPool, wi_number: i64) -> Result<Option<i64>> {
    let id: Option<i64> = sqlx::query_scalar("SELECT node_id FROM workitem WHERE wi_number = $1")
        .bind(wi_number)
        .fetch_optional(pool)
        .await?;
    Ok(id)
}

// --- the planning rail rollup (WI #823) -------------------------------------

/// One project's planning weather, for the Planning page's project rail
/// (WI #823): `<proposals> | <wi_in_proposal> / <wi_total>`.
///
/// The same aggregate `membership_select!` evaluates per row, grouped by
/// project instead — which is why #823 belongs in this sprint rather than near
/// it. Ken's instruction was *"the right SQL query makes the first one quick
/// enough, but timing the op beats guessing"*, so all three figures ship and
/// the measurement is in the sprint record rather than a pre-emptive
/// degradation to fewer numbers.
///
/// **What each figure counts, and why it is not the obvious thing.**
///
/// - `proposals` — *live* proposals (`proposed` + `active`), matching the
///   queue the Planning page renders. A done proposal is off the queue.
/// - `wi_total` — *live, unarchived* work items. Not every item: `closed` is
///   78% of the corpus (the #861 measurement), and a denominator that counts
///   years of finished work makes the ratio unreadable. This is "how much
///   open work is here", which is the question a planning rail is asked.
/// - `wi_in_proposal` — that same set, narrowed to items a live proposal
///   covers. Deliberately the *same* liveness rule the row marker uses: the
///   rail and the rows sit on one screen, and two definitions of "spoken for"
///   on one screen is a bug report waiting to happen.
///
/// Every project is returned, including one with three zeroes — a rail entry
/// that vanishes when its counts are zero is a rail you cannot click.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct PlanningRollupRow {
    pub project: String,
    /// The project's lifecycle status (see `PROJECT_STATUSES`). Added in sprint
    /// 045: this read returns *every* project, so without it a consumer cannot
    /// tell the 30 active ones from the 39 rows. The board (D-3) counts and dims
    /// by it rather than carrying a parallel project counter that could disagree
    /// with these rows; the Planning rail simply ignores it.
    pub status: String,
    /// Live proposals filed against this project.
    pub proposals: i64,
    /// Live, unarchived work items a live proposal covers.
    pub wi_in_proposal: i64,
    /// Live, unarchived work items in the project — the denominator.
    pub wi_total: i64,
}

/// Every project's [`PlanningRollupRow`], in one round trip, ordered by name.
///
/// One statement rather than three-per-project: the rail renders ~30 projects,
/// and a per-project read is the N+1 this sprint exists to delete.
///
/// Built from two grouped CTEs left-joined onto `project`, for the reason
/// `membership_columns!` documents at length: the first draft was three
/// correlated subqueries per project row and measured 7.1ms, which is a lot
/// for three numbers. Aggregating once and joining is the same answer without
/// the per-row multiplier — and it reuses the *same* liveness predicates, so
/// the rail cannot disagree with the rows it sits beside.
///
/// `coalesce` on both counts is what keeps a project with nothing in it in the
/// result with three zeroes rather than three nulls.
pub async fn planning_rollup(pool: &PgPool) -> Result<Vec<PlanningRollupRow>> {
    Ok(sqlx::query_as::<_, PlanningRollupRow>(
        "WITH live_covers AS ( \
             SELECT DISTINCT r.right_id AS node_id \
               FROM relationship r \
               JOIN sprint_proposal sp ON sp.node_id = r.left_id \
              WHERE r.relationship = 'covers' \
                AND sp.status::text IN ('proposed', 'active') \
         ), wi AS ( \
             SELECT n.project_id, \
                    count(*) AS wi_total, \
                    count(lc.node_id) AS wi_in_proposal \
               FROM workitem w \
               JOIN node n ON n.id = w.node_id \
               LEFT JOIN live_covers lc ON lc.node_id = w.node_id \
              WHERE NOT n.archived AND w.wi_status <> 'closed' \
              GROUP BY n.project_id \
         ), props AS ( \
             SELECT sn.project_id, count(*) AS proposals \
               FROM sprint_proposal sp \
               JOIN node sn ON sn.id = sp.node_id \
              WHERE NOT sn.archived \
                AND sp.status::text IN ('proposed', 'active') \
              GROUP BY sn.project_id \
         ) \
         SELECT pj.name AS project, pj.status, \
                coalesce(props.proposals, 0) AS proposals, \
                coalesce(wi.wi_in_proposal, 0) AS wi_in_proposal, \
                coalesce(wi.wi_total, 0) AS wi_total \
           FROM project pj \
           LEFT JOIN wi ON wi.project_id = pj.id \
           LEFT JOIN props ON props.project_id = pj.id \
          ORDER BY pj.name",
    )
    .fetch_all(pool)
    .await?)
}

// --- the board rollup (WI #970) ---------------------------------------------

/// How many reports the board carries. The read takes no arguments (D-1), so
/// this is a fixed policy rather than a parameter: Sensor Net renders a handful
/// of health lines, and a consumer that wants the series calls `list_reports`.
pub const BOARD_REPORT_CAP: i64 = 5;

/// A proposal as the board renders it: the queue row, plus the work-item status
/// rollup that makes Fire Missions' progress track, plus `summary`.
///
/// **Why `summary` is here** (D-5) when `list_proposals` deliberately dropped
/// it: Fire Missions renders it as the mission's subtitle — it is the panel, not
/// decoration. #852 dropped it because 110 unfiltered rows of plan-length prose
/// measured ~46k tokens; #860 then capped `summary` at 500 characters and
/// migration 0021 moved every over-cap summary into `notes`. Production's
/// longest is 499 and all fifteen live summaries together are 5,950 characters,
/// so the thing that made it unaffordable no longer exists. `notes` is still
/// unbounded and still lives behind `get_proposal`.
///
/// **Why no covered work items.** The board renders *progress*; a consumer that
/// wants the items makes the focused read the two-level contract points it at.
/// Inlining them would put the whole open corpus on a dashboard refresh.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardProposal {
    pub node_id: i64,
    pub title: String,
    /// The routing contract, ≤500 chars (#860).
    pub summary: String,
    pub status: String,
    pub project: Option<String>,
    #[ts(type = "string")]
    pub rank: Decimal,
    pub pinned: bool,
    pub comment_count: i64,
    /// The work items this proposal covers, and their statuses. The four
    /// counts sum to `covered_count` — `WI_STATUSES` is exactly these four.
    pub covered_count: i64,
    pub open: i64,
    pub resolved: i64,
    pub done: i64,
    pub closed: i64,
    /// When the proposal row last changed. korg's only staleness signal: it is
    /// *last touched*, not last progressed, which is why the board does not
    /// build an event feed out of it (D-7).
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// A program with its slices inlined — the Operations panel in one value.
///
/// Both halves are the types `get_program` already returns (D-4), so a consumer
/// renders a board program and a program detail page with the same code.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardProgram {
    #[serde(flatten)]
    #[ts(flatten)]
    pub program: ProgramRow,
    /// Included proposals in program order (`rank` on the edge, then node_id).
    pub slices: Vec<ProgramSlice>,
}

/// Everything a board renders, in one read (WI #970).
///
/// The 2026-07-31 backlog review assembled a fraction of this with 17
/// `get_proposal` calls and a script. Consumers: kfdc (the widescreen overseer
/// board — `kai:~/src/tools/kfdc`, `docs/design/kfdc-concept.html`) and
/// korg-dash (the kdeskdash Pi feed, which derives its panel counts from the
/// same read rather than growing its own queries).
///
/// **There is no counters block** (D-3). Every figure the concept's header
/// statline shows is derivable from what is here — live proposals is
/// `active.len() + queue.len()`, shipped is `proposals_omitted.done`, awaiting
/// is `awaiting.len()`, projects is `depth` filtered by `status`. A counter that
/// can disagree with the list printed beside it is a bug generator, and it is
/// precisely the aggregate creep #976 filed a warning about.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardRollup {
    /// When this board was assembled, from **Postgres's** clock — the same one
    /// every other timestamp here came from. A consumer computes "waiting 9
    /// days" as `generated - awaiting_since`, and reading the two from different
    /// clocks is how that goes subtly wrong on a cached or proxied board.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub generated: OffsetDateTime,
    /// Fire Missions: proposals in `active`, pinned first then rank.
    pub active: Vec<BoardProposal>,
    /// On Deck: proposals in `proposed`, same order.
    pub queue: Vec<BoardProposal>,
    /// What the live-and-unarchived default hid across `active` + `queue` —
    /// the same envelope, meaning the same thing, as `list_proposals`.
    pub proposals_omitted: ProposalOmitted,
    /// Operations: live programs with their ordered slices.
    pub programs: Vec<BoardProgram>,
    pub programs_omitted: ProgramOmitted,
    /// Commander's Call: everything waiting on Ken, oldest ask first.
    pub awaiting: Vec<AwaitingRow>,
    /// Queue depth per project — every project, with its `status`, so the board
    /// can count the active ones and dim the rest.
    pub depth: Vec<PlanningRollupRow>,
    /// Sensor Net: the newest [`BOARD_REPORT_CAP`] reports. `report_date` is the
    /// only date in korg that records when something *happened*, which is why
    /// this is the whole of the board's event story (D-7).
    pub reports: Vec<ReportRow>,
}

/// One row of the board's proposal pass, before it is split by panel.
///
/// `archived` is carried only to make that split honest: a proposal fetched
/// because it is a *slice* of a live program is returned whatever its state, and
/// an archived one must not leak into the queue on its way past.
#[derive(sqlx::FromRow)]
struct BoardProposalRow {
    node_id: i64,
    title: String,
    summary: String,
    status: String,
    project: Option<String>,
    rank: Decimal,
    pinned: bool,
    archived: bool,
    comment_count: i64,
    covered_count: i64,
    open: i64,
    resolved: i64,
    done: i64,
    closed: i64,
    updated: OffsetDateTime,
}

impl BoardProposalRow {
    fn into_proposal(self) -> BoardProposal {
        BoardProposal {
            node_id: self.node_id,
            title: self.title,
            summary: self.summary,
            status: self.status,
            project: self.project,
            rank: self.rank,
            pinned: self.pinned,
            comment_count: self.comment_count,
            covered_count: self.covered_count,
            open: self.open,
            resolved: self.resolved,
            done: self.done,
            closed: self.closed,
            updated: self.updated,
        }
    }

    fn as_slice(&self, rank: Option<Decimal>) -> ProgramSlice {
        ProgramSlice {
            node_id: self.node_id,
            title: self.title.clone(),
            status: self.status.clone(),
            project: self.project.clone(),
            rank,
            covered_count: self.covered_count,
            open: self.open,
            resolved: self.resolved,
            done: self.done,
            closed: self.closed,
        }
    }
}

/// `get_board` / `GET /api/board` — the whole board, one call.
///
/// **The aggregate story, decided rather than inherited (D-2, #976).** #976
/// recorded that `wi_counts` dominates both list reads and warned that a board's
/// worth of new aggregates on the same surface could multiply that shape per
/// panel. Two things stop it here:
///
/// - This read never calls the list reads, so it never pays `wi_counts` — that
///   aggregate belongs to `list_work_items`' paging envelope, and the board does
///   not page work items.
/// - Fire Missions' progress and Operations' per-slice rollups are the *same*
///   counts one level apart, so they are **one pass** over the `covers` edges
///   (450 rows in production), not two. Slices are then bucketed in Rust from
///   the `includes` edge list — a handful of rows — rather than by re-running
///   the aggregate per program the way `get_program_detail` must.
///
/// The proposal pass fetches every proposal that is either live *or* a slice of
/// a live program, so a `done` slice in a finished program is rolled up by that
/// same pass. Fetching the programs first and keying the slice pass off *their*
/// node ids is what makes `programs` and `slices` unable to disagree.
pub async fn board_rollup(pool: &PgPool) -> Result<BoardRollup> {
    let generated: OffsetDateTime = sqlx::query_scalar("SELECT now()").fetch_one(pool).await?;

    // Live, unarchived programs — the same defaults `list_programs` applies, via
    // the same function, so the board's Operations panel and `/api/programs`
    // cannot show different programs.
    let ProgramList {
        items: program_rows,
        omitted: programs_omitted,
    } = list_programs(pool, None, archived_default()).await?;
    let program_ids: Vec<i64> = program_rows.iter().map(|p| p.node_id).collect();

    // The `includes` edges of exactly those programs, in program order.
    let slice_edges = sqlx::query_as::<_, (i64, i64, Option<Decimal>)>(
        "SELECT r.left_id, r.right_id, r.rank \
           FROM relationship r \
           JOIN sprint_proposal sp ON sp.node_id = r.right_id \
          WHERE r.relationship = 'includes' AND r.left_id = ANY($1) \
          ORDER BY r.rank ASC NULLS LAST, r.right_id ASC",
    )
    .bind(&program_ids)
    .fetch_all(pool)
    .await?;
    let slice_ids: Vec<i64> = slice_edges.iter().map(|(_, right, _)| *right).collect();

    // The one aggregate pass. `cov` groups the whole `covers` corpus once; the
    // outer query keeps the live queue and anything a live program includes.
    let live_statuses: Vec<String> = PROPOSAL_LIVE_STATUSES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = sqlx::query_as::<_, BoardProposalRow>(
        "WITH cov AS ( \
             SELECT r.left_id AS proposal_id, \
                    count(*)                                                AS covered_count, \
                    count(*) FILTER (WHERE w.wi_status = 'open')            AS open, \
                    count(*) FILTER (WHERE w.wi_status = 'resolved')        AS resolved, \
                    count(*) FILTER (WHERE w.wi_status = 'done')            AS done, \
                    count(*) FILTER (WHERE w.wi_status = 'closed')          AS closed \
               FROM relationship r \
               JOIN workitem w ON w.node_id = r.right_id \
              WHERE r.relationship = 'covers' \
              GROUP BY r.left_id \
         ) \
         SELECT sp.node_id, sp.title, sp.summary, sp.status::text AS status, \
                pj.name AS project, sp.rank, sp.pinned, n.archived, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = sp.node_id) \
                                                              AS comment_count, \
                coalesce(cov.covered_count, 0) AS covered_count, \
                coalesce(cov.open, 0)          AS open, \
                coalesce(cov.resolved, 0)      AS resolved, \
                coalesce(cov.done, 0)          AS done, \
                coalesce(cov.closed, 0)        AS closed, \
                n.updated \
           FROM sprint_proposal sp \
           JOIN node n ON n.id = sp.node_id \
           LEFT JOIN project pj ON pj.id = n.project_id \
           LEFT JOIN cov ON cov.proposal_id = sp.node_id \
          WHERE (sp.status::text = ANY($1) AND NOT n.archived) \
             OR sp.node_id = ANY($2) \
          ORDER BY sp.pinned DESC, sp.rank ASC, sp.node_id ASC",
    )
    .bind(&live_statuses)
    .bind(&slice_ids)
    .fetch_all(pool)
    .await?;

    // Slices first: `as_slice` borrows, so this runs before the rows are
    // consumed into the two panels.
    let programs = program_rows
        .into_iter()
        .map(|program| {
            let slices = slice_edges
                .iter()
                .filter(|(left, _, _)| *left == program.node_id)
                .filter_map(|(_, right, rank)| {
                    rows.iter()
                        .find(|r| r.node_id == *right)
                        .map(|r| r.as_slice(*rank))
                })
                .collect();
            BoardProgram { program, slices }
        })
        .collect();

    // A slice-only row is not part of the queue: it was fetched for Operations,
    // and its own status (or its archived flag) is what decides.
    let (mut active, mut queue) = (Vec::new(), Vec::new());
    for row in rows {
        if row.archived {
            continue;
        }
        match row.status.as_str() {
            "active" => active.push(row.into_proposal()),
            "proposed" => queue.push(row.into_proposal()),
            _ => {}
        }
    }

    Ok(BoardRollup {
        generated,
        active,
        queue,
        proposals_omitted: proposal_omitted(pool, None, archived_default(), Some(&live_statuses))
            .await?,
        programs,
        programs_omitted,
        awaiting: list_awaiting(pool).await?,
        depth: planning_rollup(pool).await?,
        reports: list_reports(pool, None, BOARD_REPORT_CAP).await?,
    })
}

// --- work item update (Edit + Archive) ------------------------------------

/// `update_work_item` / `PATCH /api/work-items/:wi_number`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct WorkItemPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub details: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_type")]
    pub wi_type: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_status")]
    pub wi_status: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_tshirt")]
    pub wi_tshirt: Option<String>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub sprint: Option<Option<String>>,
    /// Move to this project (id); null unassigns. Get ids from list_projects.
    // `Some(Some(id))` moves, `Some(None)` unassigns, `None` leaves it (WI
    // #291). A move clears an area that no longer belongs to the target project
    // unless a valid `area_id` is given in the same call.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project_id: Option<Option<i64>>,
    /// Project name — the alternative to `project_id`; null unassigns. Never pass both.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub project: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub area_id: Option<Option<i64>>,
    /// Area name — the alternative to `area_id`; null clears. Resolved in the new project.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub area: Option<Option<String>>,
    /// Parent work item's wi_number; null clears the parent.
    #[serde(default, deserialize_with = "ops::double_option")]
    pub parent: Option<Option<i64>>,
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub category: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
}

pub async fn update_work_item(
    pool: &PgPool,
    wi_number: i64,
    patch: WorkItemPatch,
) -> Result<WorkItemRow> {
    let node_id = node_id_for_wi(pool, wi_number)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_number}")))?;
    if let Some(v) = &patch.wi_status {
        validate_status(v, &WI_STATUSES, "wi_status")?;
    }
    if let Some(v) = &patch.wi_type {
        validate_status(v, &vocab::WI_TYPES, "wi_type")?;
    }
    if let Some(v) = &patch.wi_tshirt {
        validate_status(v, &vocab::WI_TSHIRTS, "wi_tshirt")?;
    }
    // Resolve parent wi_number -> node id before the transaction. An
    // unresolvable number used to fall through to `Some(None)` — silently
    // *clearing* the parent (F-06).
    let parent_node: Option<Option<i64>> = match &patch.parent {
        Some(Some(num)) => Some(Some(node_id_for_wi(pool, *num).await?.ok_or_else(
            || RepoError::InvalidInput(format!("no work item #{num} to use as parent")),
        )?)),
        Some(None) => Some(None),
        None => None,
    };
    // Selectors resolve before the transaction, like `parent` above: a name
    // that doesn't resolve must change nothing (WI #575). The area name is the
    // exception — it resolves inside the transaction, because it is only
    // meaningful relative to the project the item will have *after* this
    // update, which isn't known until then.
    let project_id = resolve_project_patch(pool, patch.project_id, patch.project).await?;
    let mut tx = pool.begin().await?;

    if let Some(v) = &patch.title {
        sqlx::query("UPDATE workitem SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.content {
        sqlx::query("UPDATE workitem SET content = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.details {
        sqlx::query("UPDATE workitem SET details = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_type {
        sqlx::query("UPDATE workitem SET wi_type = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_status {
        sqlx::query("UPDATE workitem SET wi_status = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.wi_tshirt {
        sqlx::query("UPDATE workitem SET wi_tshirt = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.sprint {
        sqlx::query("UPDATE workitem SET sprint = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    // Project move + area consistency (WI #291). An area must belong to the
    // work item's project; moving projects drops an area that no longer fits
    // (unless a valid area_id is supplied in the same call).
    {
        let current_pid: Option<i64> =
            sqlx::query_scalar("SELECT project_id FROM node WHERE id = $1")
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        // Project the work item will have after this update.
        let effective_pid = match &project_id {
            Some(v) => *v,
            None => current_pid,
        };

        // An area name resolves against that effective project, then joins the
        // id path below — so `area` and `area_id` are validated identically.
        let area_id =
            resolve_area_patch(&mut *tx, effective_pid, patch.area_id, patch.area).await?;

        // Decide the area to leave in place. Some(Some(id)) = set+validate,
        // Some(None) = explicit clear, None = keep (auto-clearing on a move
        // when the current area no longer fits).
        let new_area: Option<Option<i64>> = match &area_id {
            Some(Some(aid)) => {
                let area_pid: Option<i64> =
                    sqlx::query_scalar("SELECT project_id FROM area WHERE id = $1")
                        .bind(aid)
                        .fetch_optional(&mut *tx)
                        .await?;
                if effective_pid.is_some() && area_pid == effective_pid {
                    Some(Some(*aid))
                } else {
                    return Err(RepoError::InvalidInput(format!(
                        "area {aid} does not belong to the work item's project"
                    ))
                    .into());
                }
            }
            Some(None) => Some(None),
            None => {
                if project_id.is_some() {
                    let cur_area: Option<i64> =
                        sqlx::query_scalar("SELECT area_id FROM workitem WHERE node_id = $1")
                            .bind(node_id)
                            .fetch_one(&mut *tx)
                            .await?;
                    match cur_area {
                        Some(aid) => {
                            let area_pid: Option<i64> =
                                sqlx::query_scalar("SELECT project_id FROM area WHERE id = $1")
                                    .bind(aid)
                                    .fetch_optional(&mut *tx)
                                    .await?;
                            if effective_pid.is_some() && area_pid == effective_pid {
                                None
                            } else {
                                Some(None)
                            }
                        }
                        None => None,
                    }
                } else {
                    None
                }
            }
        };

        if let Some(v) = &project_id {
            sqlx::query("UPDATE node SET project_id = $2 WHERE id = $1")
                .bind(node_id)
                .bind(*v)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(v) = new_area {
            sqlx::query("UPDATE workitem SET area_id = $2 WHERE node_id = $1")
                .bind(node_id)
                .bind(v)
                .execute(&mut *tx)
                .await?;
        }
    }
    if let Some(v) = parent_node {
        sqlx::query("UPDATE workitem SET parent_node_id = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.category {
        sqlx::query("UPDATE node SET category = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    // D-7: `closed` is Ken-only (see `vocab::WI_STATUSES`), so reaching it means
    // the ask this item was waiting on has been answered. `resolved`/`done`
    // deliberately do NOT clear — "implemented, needs your user test" is the
    // canonical awaiting-Ken state.
    settle_awaiting(&mut *tx, node_id).await?;
    touch_node(&mut *tx, node_id).await?;

    tx.commit().await?;
    get_work_item(pool, wi_number)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no work item #{wi_number}")).into())
}

// --- daily reports (kmon et al.) --------------------------------------------

/// `create_report`. `report_date` is `YYYY-MM-DD`; both transports used to
/// parse it by hand into a `time::Date` with their own error message.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewReport {
    /// reporter id, e.g. 'kmon'
    #[schemars(schema_with = "schema::non_empty")]
    pub source: String,
    #[serde(with = "report_date_fmt")]
    #[schemars(schema_with = "schema::report_date")]
    pub report_date: time::Date,
    #[schemars(schema_with = "schema::report_status")]
    pub status: String,
    /// one-liner for the list view
    #[schemars(schema_with = "schema::non_empty")]
    pub summary: String,
    /// full markdown report
    pub body: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub escalated: bool,
    /// wi_numbers of finding work items; numbers that don't resolve are dropped.
    #[serde(default, rename = "finding_work_items")]
    #[schemars(rename = "finding_work_items", schema_with = "schema::wi_numbers")]
    pub findings: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportRef {
    pub node_id: i64,
    pub replaced: bool,
    pub findings_linked: Vec<i64>,
}

/// Create or replace the report for (source, report_date). A same-day re-run
/// updates content in place and KEEPS the node_id, so relationships and
/// comments survive. The finding edge set (label 'finding') is *replaced*, not
/// accumulated (D-7): a corrected re-run that drops a finding drops its edge,
/// so `get_report.findings` reflects the latest run only.
pub async fn upsert_report(pool: &PgPool, new: NewReport) -> Result<ReportRef> {
    validate_status(&new.status, &REPORT_STATUSES, "report status")?;
    let mut resolved = Vec::with_capacity(new.findings.len());
    for wi in &new.findings {
        if let Some(n) = node_id_for_wi(pool, *wi).await? {
            resolved.push(n);
        }
    }

    let mut tx = pool.begin().await?;
    let existing: Option<i64> =
        sqlx::query("SELECT node_id FROM report WHERE source = $1 AND report_date = $2")
            .bind(&new.source)
            .bind(new.report_date)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| r.get("node_id"));

    let (node_id, replaced) = match existing {
        Some(id) => {
            sqlx::query(
                "UPDATE report SET status = $2, summary = $3, body = $4, model = $5, \
                 escalated = $6 WHERE node_id = $1",
            )
            .bind(id)
            .bind(&new.status)
            .bind(&new.summary)
            .bind(&new.body)
            .bind(&new.model)
            .bind(new.escalated)
            .execute(&mut *tx)
            .await?;
            touch_node(&mut *tx, id).await?;
            (id, true)
        }
        None => {
            let id: i64 = sqlx::query("INSERT INTO node (kind) VALUES ('report') RETURNING id")
                .fetch_one(&mut *tx)
                .await?
                .get("id");
            sqlx::query(
                "INSERT INTO report \
                 (node_id, source, report_date, status, summary, body, model, escalated) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(id)
            .bind(&new.source)
            .bind(new.report_date)
            .bind(&new.status)
            .bind(&new.summary)
            .bind(&new.body)
            .bind(&new.model)
            .bind(new.escalated)
            .execute(&mut *tx)
            .await?;
            (id, false)
        }
    };

    // Drop finding edges this run didn't produce. Matching on "the other end"
    // rather than on left_id keeps this correct for any pre-0014 edge that a
    // database might still carry unoriented.
    sqlx::query(
        "DELETE FROM relationship \
         WHERE relationship = 'finding' AND (left_id = $1 OR right_id = $1) \
           AND (CASE WHEN left_id = $1 THEN right_id ELSE left_id END) <> ALL($2)",
    )
    .bind(node_id)
    .bind(&resolved)
    .execute(&mut *tx)
    .await?;

    // Semantic orientation: report -> work item (WI #531). Provenance (D-17):
    // origin is this writer's operation name; ON CONFLICT preserves the
    // original created/origin on re-report.
    for &target in &resolved {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'finding', now(), 'create_report') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(node_id)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(ReportRef {
        node_id,
        replaced,
        findings_linked: resolved,
    })
}

time::serde::format_description!(report_date_fmt, Date, "[year]-[month]-[day]");

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportRow {
    pub node_id: i64,
    pub source: String,
    #[serde(with = "report_date_fmt")]
    #[ts(type = "string")]
    pub report_date: time::Date,
    pub status: String,
    pub summary: String,
    pub model: Option<String>,
    pub escalated: bool,
    /// Comments on this report (WI #535).
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// Newest first; summary fields only (the list view).
pub async fn list_reports(
    pool: &PgPool,
    source: Option<&str>,
    limit: i64,
) -> Result<Vec<ReportRow>> {
    let rows = sqlx::query_as::<_, ReportRow>(
        "SELECT r.node_id, r.source, r.report_date, r.status, r.summary, r.model, \
                r.escalated, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = r.node_id) AS comment_count, \
                n.updated \
         FROM report r JOIN node n ON n.id = r.node_id \
         WHERE ($1::text IS NULL OR r.source = $1) \
         ORDER BY r.report_date DESC, r.source ASC LIMIT $2",
    )
    .bind(source)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportFinding {
    pub wi_number: i64,
    pub title: String,
    pub wi_status: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ReportFull {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: ReportRow,
    pub body: String,
    pub findings: Vec<ReportFinding>,
}

/// One report with body + linked findings ('finding' edges to work items).
pub async fn get_report(pool: &PgPool, node_id: i64) -> Result<Option<ReportFull>> {
    let Some(r) = sqlx::query(
        "SELECT r.node_id, r.source, r.report_date, r.status, r.summary, r.model, \
                r.escalated, r.body, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = r.node_id) AS comment_count, \
                n.updated \
         FROM report r JOIN node n ON n.id = r.node_id WHERE r.node_id = $1",
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    let findings = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT w.wi_number, w.title, w.wi_status \
         FROM relationship rel \
         JOIN workitem w ON w.node_id = CASE WHEN rel.left_id = $1 THEN rel.right_id ELSE rel.left_id END \
         WHERE (rel.left_id = $1 OR rel.right_id = $1) AND rel.relationship = 'finding' \
         ORDER BY w.wi_number",
    )
    .bind(node_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(wi_number, title, wi_status)| ReportFinding { wi_number, title, wi_status })
    .collect();
    Ok(Some(ReportFull {
        row: ReportRow {
            node_id: r.get("node_id"),
            source: r.get("source"),
            report_date: r.get("report_date"),
            status: r.get("status"),
            summary: r.get("summary"),
            model: r.get("model"),
            escalated: r.get("escalated"),
            comment_count: r.get("comment_count"),
            updated: r.get("updated"),
        },
        body: r.get("body"),
        findings,
    }))
}

// --- handoffs (durable cross-agent/session context) ------------------------
//
// A handoff is a node like any other (the report/proposal pattern): a detail
// table for its own fields, attached to the work it describes through the
// generalized `relationship` table (label `has_handoff`, subject -> handoff).
// The read path is inherited whole from LB-3: a `has_handoff` edge surfaces in
// `get_work_item`/`get_proposal`'s `related` block automatically, so there are
// no handoff-specific projection fields on those reads (sprint 025).

/// `create_handoff` / `POST /api/handoffs`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewHandoff {
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name, e.g. `klams` — the alternative to `project_id`; never pass
    /// both. Resolved by exact name, and an unknown name returns `not_found`
    /// rather than mis-filing, so pass a name you are confident in directly.
    /// Call `list_projects` only when the name is genuinely unknown or
    /// ambiguous — the roster in this server's instructions already names
    /// every active project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    #[schemars(schema_with = "schema::non_empty")]
    pub summary: String,
    /// The full handoff document, Markdown.
    pub body: String,
    /// Nodes this handoff describes — its owners. Each becomes a `has_handoff`
    /// edge (owner -> handoff). Rejected if any id does not resolve (the whole
    /// create rolls back — a handoff must not silently lose an owner).
    #[serde(default)]
    pub related_node_ids: Vec<i64>,
    /// Opt in to a handoff with no owners. Off by default so a forgotten link
    /// step cannot silently orphan a handoff (plan Write contract).
    #[serde(default)]
    pub allow_standalone: bool,
}

/// The created handoff plus the owner node ids actually linked (deduped). Since
/// create rejects any id that does not resolve, this echoes the request minus
/// duplicates — the honest confirmation of what was attached.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct HandoffCreated {
    #[serde(flatten)]
    #[ts(flatten)]
    pub handoff: HandoffRow,
    pub related_node_ids: Vec<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct HandoffRow {
    pub node_id: i64,
    pub title: String,
    pub summary: String,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    /// Comments on this handoff (nodes are comment-generic, 0007).
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const HANDOFF_SELECT: &str = "SELECT h.node_id, h.title, h.summary, \
            pj.name AS project, n.category, n.tags, n.archived, \
            (SELECT count(*) FROM comment cm WHERE cm.node_id = h.node_id) AS comment_count, \
            n.created, n.updated \
     FROM handoff h \
     JOIN node n ON n.id = h.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id";

async fn get_handoff_row(pool: &PgPool, node_id: i64) -> Result<Option<HandoffRow>> {
    Ok(
        sqlx::query_as::<_, HandoffRow>(&format!("{HANDOFF_SELECT} WHERE h.node_id = $1"))
            .bind(node_id)
            .fetch_optional(pool)
            .await?,
    )
}

/// A handoff with its full Markdown body and the nodes it is attached to,
/// inlined the LB-3 way (both directions, nothing excluded). This is the
/// authoritative "read this handoff" call.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct HandoffFull {
    #[serde(flatten)]
    #[ts(flatten)]
    pub row: HandoffRow,
    pub body: String,
    /// The nodes this handoff is attached to (has_handoff both ways, plus any
    /// other edges), inlined up to [`RELATED_CONTEXT_CAP`] (LB-3).
    pub related: Vec<RelatedRef>,
    pub related_truncated: bool,
}

/// Create a handoff, its detail row, and one `has_handoff` edge per owner in a
/// single transaction. Mirrors `create_proposal`'s node+detail+edges insert;
/// the owner-existence check happens before the transaction, matching how
/// `create_proposal` resolves its work items. Unlike `upsert_report`, which
/// silently drops findings that don't resolve, this *rejects* an unknown owner:
/// a handoff that loses an owner unnoticed is the exact invisible-context
/// failure the handoff feature exists to prevent.
pub async fn create_handoff(pool: &PgPool, new: NewHandoff) -> Result<HandoffCreated> {
    if new.title.trim().is_empty() {
        return Err(RepoError::InvalidInput("handoff title must not be empty".into()).into());
    }
    if new.summary.trim().is_empty() {
        return Err(RepoError::InvalidInput("handoff summary must not be empty".into()).into());
    }
    if new.related_node_ids.is_empty() && !new.allow_standalone {
        return Err(RepoError::InvalidInput(
            "a handoff needs at least one related node (its owner); \
             pass allow_standalone to create one with none"
                .into(),
        )
        .into());
    }
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;

    // Reject any owner that doesn't resolve, and dedup. node_kind turns a missing
    // id into a clean `not_found` naming it, rather than the raw FK violation the
    // edge insert would otherwise surface as `internal` (WI #524). The FK on
    // relationship.left_id is still the backstop if one is deleted mid-flight.
    let mut owners: Vec<i64> = Vec::with_capacity(new.related_node_ids.len());
    for &id in &new.related_node_ids {
        node_kind(pool, id).await?;
        if !owners.contains(&id) {
            owners.push(id);
        }
    }

    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('handoff', $1, $2, $3) RETURNING id",
    )
    .bind(project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    sqlx::query("INSERT INTO handoff (node_id, title, summary, body) VALUES ($1, $2, $3, $4)")
        .bind(node_id)
        .bind(&new.title)
        .bind(&new.summary)
        .bind(&new.body)
        .execute(&mut *tx)
        .await?;

    // Owner -> handoff (subject on the left, per the registry). Provenance
    // stamped; ON CONFLICT preserves the original created/origin, as every other
    // edge writer does.
    for &owner in &owners {
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
             VALUES ($1, $2, 'has_handoff', now(), 'create_handoff') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(owner)
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    let handoff = get_handoff_row(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no handoff with node_id {node_id}")))?;
    Ok(HandoffCreated {
        handoff,
        related_node_ids: owners,
    })
}

/// `get_handoff` — the handoff, its Markdown body, and the nodes it is attached
/// to. `None` if no handoff has that node id (transports turn that into 404 /
/// isError per D-6).
pub async fn get_handoff(pool: &PgPool, node_id: i64) -> Result<Option<HandoffFull>> {
    let Some(row) = get_handoff_row(pool, node_id).await? else {
        return Ok(None);
    };
    let body: String = sqlx::query_scalar("SELECT body FROM handoff WHERE node_id = $1")
        .bind(node_id)
        .fetch_one(pool)
        .await?;
    // Nothing excluded: a handoff wants to show every owner it is attached to.
    let (related, related_truncated) = related_context(pool, node_id, None).await?;
    Ok(Some(HandoffFull {
        row,
        body,
        related,
        related_truncated,
    }))
}

/// `update_handoff` / `PATCH /api/handoffs/:node_id`. Partial: only passed
/// fields change. Relationship changes go through `relate`/`unrelate`, not here
/// (plan Write contract). Same "only bind what's present" shape as
/// `update_proposal`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct HandoffPatch {
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub title: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty")]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub archived: Option<bool>,
}

pub async fn update_handoff(
    pool: &PgPool,
    node_id: i64,
    patch: HandoffPatch,
) -> Result<HandoffRow> {
    if patch.title.as_ref().is_some_and(|v| v.trim().is_empty()) {
        return Err(RepoError::InvalidInput("handoff title must not be empty".into()).into());
    }
    if patch.summary.as_ref().is_some_and(|v| v.trim().is_empty()) {
        return Err(RepoError::InvalidInput("handoff summary must not be empty".into()).into());
    }
    let mut tx = pool.begin().await?;
    require_kind(&mut *tx, node_id, "handoff", "handoff").await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE handoff SET title = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.summary {
        sqlx::query("UPDATE handoff SET summary = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.body {
        sqlx::query("UPDATE handoff SET body = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
    }
    touch_node(&mut *tx, node_id).await?;
    tx.commit().await?;
    get_handoff_row(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no handoff with node_id {node_id}")).into())
}
