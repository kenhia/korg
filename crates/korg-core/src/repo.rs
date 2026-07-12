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

/// Domain errors the API surface should translate to 4xx rather than 500
/// (WI #289). Anything else stays an opaque `anyhow` error → 500. Carried
/// through `anyhow` and recovered by `downcast_ref` in korg-api's error layer.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// Caller passed a bad value (unknown status, area not in project, …) → 400.
    #[error("{0}")]
    InvalidInput(String),
    /// Named/keyed entity doesn't exist (no project X, …) → 404.
    #[error("{0}")]
    NotFound(String),
}

// --- work items -----------------------------------------------------------

/// Canonical work-item statuses (WI #285). Lifecycle: `open → resolved`
/// (implemented; may still need a user test / may not be PR'd) `→ done`
/// (agent satisfied — terminal but still visible in default lists)
/// `→ closed` (Ken only; hidden by default). Writes outside this set are
/// rejected.
pub const WI_STATUSES: [&str; 4] = ["open", "resolved", "done", "closed"];

/// Project lifecycle statuses (WI #246). Default WI-page rail shows only
/// `active` + `maintenance` unless "show all" is on.
pub const PROJECT_STATUSES: [&str; 4] = ["active", "maintenance", "inactive", "archived"];

fn validate_status(value: &str, allowed: &[&str], what: &str) -> Result<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(RepoError::InvalidInput(format!(
            "invalid {what} '{value}' — expected one of: {}",
            allowed.join(", ")
        ))
        .into())
    }
}

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
    validate_status(&new.wi_status, &WI_STATUSES, "wi_status")?;
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

    // Since 0009_identity, wi_number IS the node id — one number everywhere.
    let wi_number: i64 = sqlx::query(
        "INSERT INTO workitem \
         (node_id, wi_number, area_id, wi_type, wi_status, wi_tshirt, sprint, title, content, details) \
         VALUES ($1, $1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING wi_number",
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

/// Update the cross-cutting tags on any node.
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
    /// "out" = the queried node is the edge's left (label reads queried → this
    /// neighbor, e.g. queried `depends_on` neighbor); "in" = the reverse.
    pub direction: String,
}

pub async fn relate(pool: &PgPool, left: i64, right: i64, label: &str) -> Result<i64> {
    // Relationships are DIRECTED (sprint 008, supersedes WI #84's undirected
    // canonicalization): the label reads left-to-right, e.g. left `depends_on`
    // right. Exact duplicates still dedup via the unique constraint; the
    // reverse orientation is a distinct edge. Labels with no meaningful
    // direction simply ignore it, as all pre-008 consumers already did.
    let id: i64 = sqlx::query(
        "INSERT INTO relationship (left_id, right_id, relationship) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id \
         RETURNING id",
    )
    .bind(left)
    .bind(right)
    .bind(label)
    .fetch_one(pool)
    .await?
    .get("id");
    Ok(id)
}

/// Neighbors of `node`: the node on the other end of each edge (direction
/// tells you which end the queried node is),
/// with that node's kind and the relationship label. Works across kinds.
pub async fn neighbors(pool: &PgPool, node: i64) -> Result<Vec<Neighbor>> {
    let rows = sqlx::query_as::<_, Neighbor>(
        "SELECT r.id AS rel_id, n.id AS node_id, n.kind, r.relationship AS label, \
                CASE WHEN r.left_id = $1 THEN 'out' ELSE 'in' END AS direction \
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

pub async fn unrelate(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM relationship WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

// --- cross-kind node preview (WI #260) -------------------------------------

/// A label/value metadata row in a node preview (e.g. "Area" → "ui").
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize)]
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
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
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
    /// Number of comments on this work item (WI #392) — the hint that tells an
    /// agent "this row has discussion; fetch it".
    pub comment_count: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub created: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
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

pub async fn list_work_items(pool: &PgPool) -> Result<Vec<WorkItemRow>> {
    let sql = format!("{WORKITEM_SELECT} ORDER BY w.wi_number");
    Ok(sqlx::query_as::<_, WorkItemRow>(&sql)
        .fetch_all(pool)
        .await?)
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
#[derive(Debug, Clone, Serialize)]
pub struct WorkItemDetail {
    #[serde(flatten)]
    pub item: WorkItemRow,
    pub comments: Vec<Comment>,
    /// True when there are more comments than were inlined (call `list_comments`).
    pub comments_truncated: bool,
}

/// `get_work_item` plus inlined, capped comments (WI #392). `None` if the
/// work item doesn't exist.
pub async fn get_work_item_detail(
    pool: &PgPool,
    wi_number: i64,
) -> Result<Option<WorkItemDetail>> {
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
    Ok(Some(WorkItemDetail {
        item,
        comments,
        comments_truncated,
    }))
}

// --- work item survey (slim, paginated) -------------------------------------

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Default)]
pub struct ProjectPatch {
    pub gh_repo: Option<Option<String>>,
    pub cn_path: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub status: Option<String>,
    pub machines: Option<Vec<String>>,
    pub deploy_to: Option<Vec<String>>,
    pub category: Option<Option<String>>,
}

pub async fn update_project(pool: &PgPool, id: i64, patch: &ProjectPatch) -> Result<()> {
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
    Ok(())
}

/// Name-keyed wrapper (the REST/MCP surfaces key projects by name; the
/// name itself is immutable — see WI #246).
pub async fn update_project_by_name(pool: &PgPool, name: &str, patch: &ProjectPatch) -> Result<()> {
    let id: Option<i64> = sqlx::query_scalar("SELECT id FROM project WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    match id {
        Some(id) => update_project(pool, id, patch).await,
        None => Err(RepoError::NotFound(format!("no project named '{name}'")).into()),
    }
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<ProjectRow>> {
    let rows = sqlx::query_as::<_, ProjectRow>(
        "SELECT id, name, gh_repo, cn_path, description, status, machines, deploy_to, category \
         FROM project ORDER BY name",
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

/// Edit a comment's body (WI #232). The `updated` column advances via the
/// standard trigger; `created` is preserved.
pub async fn update_comment(pool: &PgPool, id: i64, body: &str) -> Result<Comment> {
    let c = sqlx::query_as::<_, Comment>(
        "UPDATE comment SET body = $2 WHERE id = $1 \
         RETURNING id, node_id, body, created, updated",
    )
    .bind(id)
    .bind(body)
    .fetch_optional(pool)
    .await?;
    c.ok_or_else(|| anyhow::anyhow!("no comment with id {id}"))
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
        let (lo, hi) = if node_id <= target {
            (node_id, target)
        } else {
            (target, node_id)
        };
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
    /// Move the work item to another project (WI #291). `Some(Some(id))` moves,
    /// `Some(None)` unassigns, `None` leaves it. A move clears an area that no
    /// longer belongs to the target project unless a valid `area_id` is given
    /// in the same call.
    pub project_id: Option<Option<i64>>,
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
        validate_status(v, &WI_STATUSES, "wi_status")?;
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
        let effective_pid = match &patch.project_id {
            Some(v) => *v,
            None => current_pid,
        };

        // Decide the area to leave in place. Some(Some(id)) = set+validate,
        // Some(None) = explicit clear, None = keep (auto-clearing on a move
        // when the current area no longer fits).
        let new_area: Option<Option<i64>> = match &patch.area_id {
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
                if patch.project_id.is_some() {
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

        if let Some(v) = &patch.project_id {
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
    Ok(())
}

// --- daily reports (kmon et al.) --------------------------------------------

#[derive(Debug, Clone)]
pub struct NewReport {
    pub source: String,
    pub report_date: time::Date,
    pub status: String,
    pub summary: String,
    pub body: String,
    pub model: Option<String>,
    pub escalated: bool,
    /// wi_numbers of finding work items; numbers that don't resolve are dropped.
    pub findings: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportRef {
    pub node_id: i64,
    pub replaced: bool,
    pub findings_linked: Vec<i64>,
}

/// Create or replace the report for (source, report_date). A same-day re-run
/// updates content in place and KEEPS the node_id, so relationships and
/// comments survive. Finding edges (label 'finding') are added idempotently.
pub async fn upsert_report(pool: &PgPool, new: NewReport) -> Result<ReportRef> {
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

    for &target in &resolved {
        let (lo, hi) = if node_id <= target {
            (node_id, target)
        } else {
            (target, node_id)
        };
        sqlx::query(
            "INSERT INTO relationship (left_id, right_id, relationship) \
             VALUES ($1, $2, 'finding') \
             ON CONFLICT (left_id, right_id, relationship) DO UPDATE SET left_id = relationship.left_id",
        )
        .bind(lo)
        .bind(hi)
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

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct ReportRow {
    pub node_id: i64,
    pub source: String,
    #[serde(with = "report_date_fmt")]
    pub report_date: time::Date,
    pub status: String,
    pub summary: String,
    pub model: Option<String>,
    pub escalated: bool,
    #[serde(with = "time::serde::rfc3339")]
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
                r.escalated, n.updated \
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

#[derive(Debug, Clone, Serialize)]
pub struct ReportFinding {
    pub wi_number: i64,
    pub title: String,
    pub wi_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportFull {
    #[serde(flatten)]
    pub row: ReportRow,
    pub body: String,
    pub findings: Vec<ReportFinding>,
}

/// One report with body + linked findings ('finding' edges to work items).
pub async fn get_report(pool: &PgPool, node_id: i64) -> Result<Option<ReportFull>> {
    let Some(r) = sqlx::query(
        "SELECT r.node_id, r.source, r.report_date, r.status, r.summary, r.model, \
                r.escalated, r.body, n.updated \
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
            updated: r.get("updated"),
        },
        body: r.get("body"),
        findings,
    }))
}
