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
use sqlx::{Executor, PgPool, Postgres, Row};
use time::OffsetDateTime;
use ts_rs::TS;

pub use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::relationships;
use crate::vocab::{self, CARD_STATUSES, LINK_DISPOSITIONS, PROPOSAL_STATUSES, REPORT_STATUSES};
pub use crate::vocab::{PROJECT_STATUSES, WI_STATUSES};

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
/// registry is a closed, four-entry set, so every suggestion is a real label.
/// The near-miss is case-insensitive exact, then a prefix overlap
/// (`related` -> `related-to`); anything further is left to the named
/// vocabulary rather than guessed at.
fn unknown_label(label: &str) -> anyhow::Error {
    let registered: Vec<&str> = relationships::REGISTRY.iter().map(|s| s.label).collect();
    let lower = label.to_ascii_lowercase();
    let suggestion = registered
        .iter()
        .find(|l| l.eq_ignore_ascii_case(label))
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

/// Look a project up by name.
async fn project_id_for_name(pool: &PgPool, name: &str) -> Result<i64> {
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM project WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    match id {
        Some(id) => Ok(id),
        None => Err(unknown_project(pool, name).await),
    }
}

/// Confirm a project id exists. Without this a typo'd id reached the FK and
/// came back as a raw Postgres error in a 500 — the same shape WI #524 fixed
/// for `relate`'s endpoints.
async fn require_project(pool: &PgPool, id: i64) -> Result<()> {
    let found: Option<i64> = sqlx::query_scalar("SELECT id FROM project WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    found.map(|_| ()).ok_or_else(|| {
        RepoError::InvalidInput(format!(
            "no project with id {id} — call list_projects (GET /api/projects) for the available projects"
        ))
        .into()
    })
}

/// Resolve a create-time project selector: id, name, or neither.
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
/// Unbounded list reads were the review's context bomb: `list_work_items`
/// returned every row with full content, which is why `survey_work_items` had
/// to exist at all.
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
    /// Project name — the alternative to `project_id` (see list_projects). Never pass both.
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
    /// Project name — the alternative to `project_id` (see list_projects). Never pass both.
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
    /// Project name — the alternative to `project_id` (see list_projects). Never pass both.
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
            n.category, n.tags \
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
    tx.commit().await?;
    reread_link(pool, node_id).await
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
    // ON CONFLICT no-op touches only left_id, so re-relate preserves the
    // original created/origin (what LB-1's migration comment reserved).
    let id: i64 = sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship, created, origin) \
         VALUES ($1, $2, $3, now(), $4) \
         ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id \
         RETURNING id",
    )
    .bind(left)
    .bind(right)
    .bind(label)
    .bind(origin)
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
                COALESCE(w.title, sp.title, cd.title, lk.title, lk.url, tp.name, rp.summary, \
                         other.kind || ' #' || other.id) AS title, \
                r.relationship AS label, \
                CASE WHEN r.left_id = $1 THEN 'out' ELSE 'in' END AS direction, \
                count(*) OVER() AS total \
         FROM relationship r \
         JOIN node other \
           ON other.id = CASE WHEN r.left_id = $1 THEN r.right_id ELSE r.left_id END \
         LEFT JOIN workitem w         ON w.node_id  = other.id \
         LEFT JOIN sprint_proposal sp ON sp.node_id = other.id \
         LEFT JOIN card cd            ON cd.node_id = other.id \
         LEFT JOIN link lk            ON lk.node_id = other.id \
         LEFT JOIN topic tp           ON tp.node_id = other.id \
         LEFT JOIN report rp          ON rp.node_id = other.id \
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
                "SELECT title, summary, status::text AS status, pinned \
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
                p.body = Some(r.get("summary"));
                p.body_label = Some("Summary".into());
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
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

const WORKITEM_SELECT: &str = "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, \
        (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
        n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id";

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
/// (WI #534). Kept alongside `survey_work_items` per D-10: one project's items
/// with content stay a single call; cross-project callers want the survey.
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
/// the caller can page the tail via `list_comments`.
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

// --- work item survey (slim, paginated) -------------------------------------

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
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct WorkItemSurvey {
    pub items: Vec<WorkItemSummary>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// A slim, paginated projection of work items (no content/details) for
/// cross-project surveys — e.g. the `refill-queue` skill — which can't
/// afford `list_work_items`'s full payload at instance scale. `total` is
/// the full filtered count (before LIMIT/OFFSET), so callers can page.
pub async fn survey_work_items(
    pool: &PgPool,
    project: Option<&str>,
    wi_status: Option<&str>,
    archived: Option<bool>,
    limit: i64,
    offset: i64,
) -> Result<WorkItemSurvey> {
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
        total: i64,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT w.wi_number, w.node_id, pj.name AS project, w.title, \
                w.wi_type, w.wi_status, w.wi_tshirt, \
                (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
                count(*) OVER() AS total \
         FROM workitem w \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR pj.name = $1) \
           AND ($2::text IS NULL OR w.wi_status = $2) \
           AND ($3::bool IS NULL OR n.archived = $3) \
         ORDER BY w.wi_number \
         LIMIT $4 OFFSET $5",
    )
    .bind(project)
    .bind(wi_status)
    .bind(archived)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
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
        })
        .collect();
    Ok(WorkItemSurvey {
        items,
        total,
        limit,
        offset,
    })
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
    pub cn_path: Option<String>,
    pub description: Option<String>,
    /// Lifecycle status — see PROJECT_STATUSES.
    pub status: String,
    /// Machines this project's working copy lives on (kai/kubs0/cleo…).
    pub machines: Vec<String>,
    /// Machines this project deploys to (e.g. korg → kubsdb).
    pub deploy_to: Vec<String>,
    pub category: Option<String>,
}

/// Everything but `name` is editable (WI #246). `None` = leave unchanged;
/// inner `None` on the nullable fields clears them.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ProjectPatch {
    #[serde(default, deserialize_with = "ops::double_option")]
    pub gh_repo: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub cn_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub description: Option<Option<String>>,
    #[serde(default)]
    #[schemars(schema_with = "schema::project_status")]
    pub status: Option<String>,
    #[serde(default)]
    pub machines: Option<Vec<String>>,
    #[serde(default)]
    pub deploy_to: Option<Vec<String>>,
    #[serde(default, deserialize_with = "ops::double_option")]
    pub category: Option<Option<String>>,
}

pub async fn update_project(pool: &PgPool, id: i64, patch: &ProjectPatch) -> Result<ProjectRow> {
    if let Some(v) = &patch.status {
        validate_status(v, &PROJECT_STATUSES, "project status")?;
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
    if let Some(v) = &patch.cn_path {
        sqlx::query("UPDATE project SET cn_path = $2 WHERE id = $1")
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

const PROJECT_SELECT: &str =
    "SELECT id, name, gh_repo, cn_path, description, status, machines, deploy_to, category \
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
}

pub async fn list_areas(pool: &PgPool, project: &str) -> Result<Vec<AreaRow>> {
    let rows = sqlx::query_as::<_, AreaRow>(
        "SELECT a.id, a.name FROM area a \
         JOIN project p ON p.id = a.project_id \
         WHERE p.name = $1 ORDER BY a.name",
    )
    .bind(project)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// --- sprint proposals (agent planning) -------------------------------------

/// `propose_sprint` / `POST /api/proposals`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewProposal {
    #[serde(default)]
    pub project_id: Option<i64>,
    /// Project name — the alternative to `project_id` (see list_projects). Never pass both.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "schema::tags")]
    pub tags: Vec<String>,
    #[schemars(schema_with = "schema::non_empty")]
    pub title: String,
    pub summary: String,
    /// Drag-order position; lower sorts first among unpinned proposals.
    #[serde(default)]
    #[schemars(schema_with = "schema::rank")]
    pub rank: Decimal,
    #[serde(default)]
    pub pinned: bool,
    /// wi_numbers this proposal covers; numbers that don't resolve are dropped.
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
    let project_id = resolve_project(pool, new.project_id, new.project.as_deref()).await?;
    let mut covered = Vec::with_capacity(new.covers.len());
    for wi in &new.covers {
        if let Some(n) = node_id_for_wi(pool, *wi).await? {
            covered.push(n);
        }
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
        "INSERT INTO sprint_proposal (node_id, title, summary, rank, pinned) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(node_id)
    .bind(&new.title)
    .bind(&new.summary)
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
    pub summary: String,
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
    "SELECT p.node_id, p.title, p.summary, p.status::text AS status, p.rank, p.pinned, \
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
    #[serde(default)]
    pub summary: Option<String>,
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
    tx.commit().await?;
    get_proposal(pool, node_id)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no proposal with node_id {node_id}")).into())
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
            sqlx::query("UPDATE node SET updated = now() WHERE id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
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
