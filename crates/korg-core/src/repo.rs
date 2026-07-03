//! korg-core repository layer: typed creation of nodes (work items, cards,
//! reading-list links) and generalized cross-kind relationships.
//!
//! Every entity is a `node`; kind-specific data lives in a detail table; any
//! two nodes can be linked through a single `relationship` edge regardless of
//! kind. This is the API the MCP/CLI/web surfaces (M4/M5) build on.

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgPool, Row};
use time::OffsetDateTime;

// --- work items -----------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewWorkItem {
    pub project_id: Option<i64>,
    pub area_id: Option<i64>,
    pub wi_type: String,
    pub wi_status: String,
    pub wi_tshirt: String,
    pub sprint: Option<String>,
    pub title: String,
    pub content: String,
    pub details: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WorkItemRef {
    pub node_id: i64,
    pub wi_number: i64,
}

pub async fn create_work_item(pool: &PgPool, new: NewWorkItem) -> Result<WorkItemRef> {
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('workitem', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
    .bind(&new.category)
    .bind(&new.tags)
    .fetch_one(&mut *tx)
    .await?
    .get("id");

    let wi_number: i64 = sqlx::query(
        "INSERT INTO workitem \
         (node_id, area_id, wi_type, wi_status, wi_tshirt, sprint, title, content, details) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING wi_number",
    )
    .bind(node_id)
    .bind(new.area_id)
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
    Ok(WorkItemRef { node_id, wi_number })
}

// --- cards ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewCard {
    pub project_id: Option<i64>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub status: String,
    pub title: String,
    pub description: String,
    pub rank: Decimal,
}

pub async fn create_card(pool: &PgPool, new: NewCard) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('card', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
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
    Ok(node_id)
}

// --- reading-list links ---------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewLink {
    pub project_id: Option<i64>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct LinkRow {
    pub node_id: i64,
    pub url: String,
    pub title: Option<String>,
    pub read: bool,
    pub disposition: String,
    pub category: Option<String>,
    pub tags: Vec<String>,
}

pub async fn create_link(pool: &PgPool, new: NewLink) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let node_id: i64 = sqlx::query(
        "INSERT INTO node (kind, project_id, category, tags) \
         VALUES ('link', $1, $2, $3) RETURNING id",
    )
    .bind(new.project_id)
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
    Ok(node_id)
}

pub async fn list_links(pool: &PgPool) -> Result<Vec<LinkRow>> {
    let rows = sqlx::query_as::<_, LinkRow>(
        "SELECT l.node_id, l.url, l.title, l.read, l.disposition::text AS disposition, \
                n.category, n.tags \
         FROM link l JOIN node n ON n.id = l.node_id \
         ORDER BY l.node_id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn set_link_disposition(pool: &PgPool, node_id: i64, disposition: &str) -> Result<()> {
    sqlx::query("UPDATE link SET disposition = $2::link_disposition WHERE node_id = $1")
        .bind(node_id)
        .bind(disposition)
        .execute(pool)
        .await?;
    Ok(())
}

/// Update the cross-cutting tags on any node (work item, card, link, slot).
pub async fn set_node_tags(pool: &PgPool, node_id: i64, tags: &[String]) -> Result<()> {
    sqlx::query("UPDATE node SET tags = $2 WHERE id = $1")
        .bind(node_id)
        .bind(tags)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_link_read(pool: &PgPool, node_id: i64, read: bool) -> Result<()> {
    sqlx::query("UPDATE link SET read = $2 WHERE node_id = $1")
        .bind(node_id)
        .bind(read)
        .execute(pool)
        .await?;
    Ok(())
}

// --- generalized relationships --------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq)]
pub struct Neighbor {
    pub rel_id: i64,
    pub node_id: i64,
    pub kind: String,
    pub label: String,
}

pub async fn relate(pool: &PgPool, left: i64, right: i64, label: &str) -> Result<i64> {
    // Relationships are undirected: canonicalize to left < right so the unique
    // constraint and dedup treat (a,b) and (b,a) as the same edge.
    let (lo, hi) = if left <= right { (left, right) } else { (right, left) };
    let id: i64 = sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id \
         RETURNING id",
    )
    .bind(lo)
    .bind(hi)
    .bind(label)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// Undirected neighbors of `node`: the node on the other end of each edge,
/// with that node's kind and the relationship label. Works across kinds.
pub async fn neighbors(pool: &PgPool, node: i64) -> Result<Vec<Neighbor>> {
    let rows = sqlx::query_as::<_, Neighbor>(
        "SELECT r.id AS rel_id, n.id AS node_id, n.kind, r.relationship AS label \
         FROM relationship r \
         JOIN node n \
           ON n.id = CASE WHEN r.left_id = $1 THEN r.right_id ELSE r.left_id END \
         WHERE r.left_id = $1 OR r.right_id = $1 \
         ORDER BY n.id",
    )
    .bind(node)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn unrelate(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM relationship WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- read views -----------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
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
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated: OffsetDateTime,
}

const WORKITEM_SELECT: &str = "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id";

pub async fn list_work_items(pool: &PgPool) -> Result<Vec<WorkItemRow>> {
    let sql = format!("{WORKITEM_SELECT} ORDER BY w.wi_number");
    Ok(sqlx::query_as::<_, WorkItemRow>(&sql).fetch_all(pool).await?)
}

pub async fn get_work_item(pool: &PgPool, wi_number: i64) -> Result<Option<WorkItemRow>> {
    let sql = format!("{WORKITEM_SELECT} WHERE w.wi_number = $1");
    Ok(sqlx::query_as::<_, WorkItemRow>(&sql)
        .bind(wi_number)
        .fetch_optional(pool)
        .await?)
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct CardRow {
    pub node_id: i64,
    pub status: String,
    pub title: String,
    pub description: String,
    pub rank: Decimal,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated: OffsetDateTime,
}

pub async fn list_cards(pool: &PgPool) -> Result<Vec<CardRow>> {
    let rows = sqlx::query_as::<_, CardRow>(
        "SELECT c.node_id, c.status::text AS status, c.title, c.description, c.rank, \
                pj.name AS project, n.category, n.tags, n.archived, n.created, n.updated \
         FROM card c \
         JOIN node n ON n.id = c.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         ORDER BY c.status, c.rank ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ProjectRow {
    pub id: i64,
    pub name: String,
    pub gh_repo: Option<String>,
    pub cn_path: Option<String>,
    pub description: Option<String>,
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectRow>> {
    let rows = sqlx::query_as::<_, ProjectRow>(
        "SELECT id, name, gh_repo, cn_path, description FROM project ORDER BY name",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
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

#[derive(Debug, Clone, Default)]
pub struct CardPatch {
    pub status: Option<String>,
    pub rank: Option<Decimal>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub archived: Option<bool>,
    pub project_id: Option<Option<i64>>,
    pub category: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
}

pub async fn update_card(pool: &PgPool, node_id: i64, patch: CardPatch) -> Result<()> {
    let mut tx = pool.begin().await?;
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
    if let Some(project_id) = &patch.project_id {
        sqlx::query("UPDATE node SET project_id = $2 WHERE id = $1")
            .bind(node_id)
            .bind(*project_id)
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
    Ok(())
}

// --- comments -------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Comment {
    pub id: i64,
    pub node_id: i64,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
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

pub async fn delete_comment(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM comment WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- areas ----------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
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

#[derive(Debug, Clone)]
pub struct NewProposal {
    pub project_id: Option<i64>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub title: String,
    pub summary: String,
    pub rank: Decimal,
    pub pinned: bool,
    /// wi_numbers this proposal covers; numbers that don't resolve are dropped.
    pub covers: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProposalRef {
    pub node_id: i64,
    pub covered: Vec<i64>,
}

/// Create a sprint proposal and its `covers` edges to the given work items in
/// one transaction. Mirrors `create_work_item`'s node+detail insert; the
/// wi_number -> node_id resolution happens before the transaction, matching
/// `update_work_item`'s handling of `parent`.
pub async fn create_proposal(pool: &PgPool, new: NewProposal) -> Result<ProposalRef> {
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
    .bind(new.project_id)
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

    for &target in &covered {
        let (lo, hi) = if node_id <= target { (node_id, target) } else { (target, node_id) };
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship) \
             VALUES ($1, $2, 'covers') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(lo)
        .bind(hi)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(ProposalRef { node_id, covered })
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ProposalRow {
    pub node_id: i64,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub rank: Decimal,
    pub pinned: bool,
    pub project: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub archived: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated: OffsetDateTime,
}

/// List proposals ordered pinned-first, then by rank — the drag-order a user
/// or agent leaves them in. `status` optionally filters (e.g. "proposed").
pub async fn list_proposals(pool: &PgPool, status: Option<&str>) -> Result<Vec<ProposalRow>> {
    let rows = sqlx::query_as::<_, ProposalRow>(
        "SELECT p.node_id, p.title, p.summary, p.status::text AS status, p.rank, p.pinned, \
                pj.name AS project, n.category, n.tags, n.archived, n.created, n.updated \
         FROM sprint_proposal p \
         JOIN node n ON n.id = p.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE ($1::text IS NULL OR p.status::text = $1) \
         ORDER BY p.pinned DESC, p.rank ASC",
    )
    .bind(status)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, Default)]
pub struct ProposalPatch {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub status: Option<String>,
    pub rank: Option<Decimal>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub tags: Option<Vec<String>>,
}

/// Partially update a proposal: status transitions (propose -> active ->
/// done/declined), reorder (rank), pin, archive. Same "only bind what's
/// present" shape as `update_card`.
pub async fn update_proposal(pool: &PgPool, node_id: i64, patch: ProposalPatch) -> Result<()> {
    let mut tx = pool.begin().await?;
    if let Some(v) = &patch.title {
        sqlx::query("UPDATE sprint_proposal SET title = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.summary {
        sqlx::query("UPDATE sprint_proposal SET summary = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.status {
        sqlx::query("UPDATE sprint_proposal SET status = $2::sprint_proposal_status WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = patch.rank {
        sqlx::query("UPDATE sprint_proposal SET rank = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = patch.pinned {
        sqlx::query("UPDATE sprint_proposal SET pinned = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    tx.commit().await?;
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
        .fetch_one(pool)
        .await?;
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

#[derive(Debug, Clone, Default)]
pub struct WorkItemPatch {
    pub title: Option<String>,
    pub content: Option<String>,
    pub details: Option<Option<String>>,
    pub wi_type: Option<String>,
    pub wi_status: Option<String>,
    pub wi_tshirt: Option<String>,
    pub sprint: Option<Option<String>>,
    pub area_id: Option<Option<i64>>,
    /// Parent expressed as the parent's user-facing wi_number (None clears).
    pub parent: Option<Option<i64>>,
    pub archived: Option<bool>,
    pub category: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
}

pub async fn update_work_item(pool: &PgPool, wi_number: i64, patch: WorkItemPatch) -> Result<()> {
    let node_id = match node_id_for_wi(pool, wi_number).await? {
        Some(n) => n,
        None => return Ok(()),
    };
    // Resolve parent wi_number -> node id before the transaction.
    let parent_node: Option<Option<i64>> = match &patch.parent {
        Some(Some(num)) => Some(node_id_for_wi(pool, *num).await?),
        Some(None) => Some(None),
        None => None,
    };
    let mut tx = pool.begin().await?;

    if let Some(v) = &patch.title {
        sqlx::query("UPDATE workitem SET title = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.content {
        sqlx::query("UPDATE workitem SET content = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.details {
        sqlx::query("UPDATE workitem SET details = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.wi_type {
        sqlx::query("UPDATE workitem SET wi_type = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.wi_status {
        sqlx::query("UPDATE workitem SET wi_status = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.wi_tshirt {
        sqlx::query("UPDATE workitem SET wi_tshirt = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.sprint {
        sqlx::query("UPDATE workitem SET sprint = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.area_id {
        sqlx::query("UPDATE workitem SET area_id = $2 WHERE node_id = $1").bind(node_id).bind(*v).execute(&mut *tx).await?;
    }
    if let Some(v) = parent_node {
        sqlx::query("UPDATE workitem SET parent_node_id = $2 WHERE node_id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = patch.archived {
        sqlx::query("UPDATE node SET archived = $2 WHERE id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.category {
        sqlx::query("UPDATE node SET category = $2 WHERE id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }
    if let Some(v) = &patch.tags {
        sqlx::query("UPDATE node SET tags = $2 WHERE id = $1").bind(node_id).bind(v).execute(&mut *tx).await?;
    }

    tx.commit().await?;
    Ok(())
}
