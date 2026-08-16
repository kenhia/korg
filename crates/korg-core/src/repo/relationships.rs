//! Generalized relationships — the single edge table any two nodes are linked through,
//! whatever their kinds.

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgPool, Row};
use ts_rs::TS;

use crate::error::RepoError;
use crate::relationships;

use super::common::{cross_project_covers, node_kind, node_project, wi_handle};
use super::selectors::unknown_label;

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
                COALESCE(w.title, sp.title, pg.title, cd.title, lk.title, lk.url, \
                         rp.summary, hd.title, at.filename, \
                         other.kind || ' #' || other.id) AS title, \
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
         LEFT JOIN report rp          ON rp.node_id = other.id \
         LEFT JOIN handoff hd         ON hd.node_id = other.id \
         LEFT JOIN attachment at      ON at.node_id = other.id \
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
