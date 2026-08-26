//! Projects: the row, the tiered read surface (WI #828), and the writes.
//!
//! A project is not a node — it is the routing table every node hangs off,
//! which is why it has an `id` of its own rather than a `node_id`.

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::vocab::{PROJECT_CATEGORIES, PROJECT_STATUSES};

use super::areas::{list_areas, AreaRow};
use super::common::validate_status;
use super::work_items::{WorkItemRow, WORKITEM_SELECT};

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
    /// Hot right now (WI #1629, migration 0032). Both project rails lift the
    /// starred set into a band above the category groups, and leave each
    /// project in its normal category position as well.
    ///
    /// `ProjectRow`-only, never `ProjectLeanRow`: the lean row answers *does
    /// this work belong here?* and hotness is not evidence for that. A routing
    /// agent that can see which projects are hot is a routing agent with a
    /// thumb on the scale — the misroute the lean projection exists to prevent.
    pub starred: bool,
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
    /// Mark this project hot, lifting it into the band at the top of both
    /// project rails (it also stays in its normal category position). Not a
    /// priority and not a tier: it is a working set that turns over every week
    /// or so, and nothing in korg reads it except the rails.
    ///
    /// Deliberately not spelled `pinned`, which `sprint_proposal` and `program`
    /// use to order a queue. Two meanings behind one word on three node kinds
    /// is ambiguity in the surface an agent reads.
    #[serde(default)]
    pub starred: Option<bool>,
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
    if let Some(v) = &patch.starred {
        sqlx::query("UPDATE project SET starred = $2 WHERE id = $1")
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
                              machines, deploy_to, category, starred, created, updated \
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
