//! The awaiting-Ken marker (#969, sprint 044)
//!
//! "This moves only when Ken acts" — the most valuable lane on the kfdc board,
//! and until now expressible only as prose in a comment.
//!
//! Deliberately not a reserved tag, which is the cheaper mechanism #969 guessed
//! at. Tags are written wholesale (`UPDATE node SET tags = $2`), so an agent
//! editing tags for an unrelated reason silently clears the marker — and 76% of
//! nodes carry tags, so that is a hot path. See 0023 §5 and D-3.

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::vocab::CARD_TERMINAL_STATUSES;

use super::common::require_node;

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
            COALESCE(w.title, sp.title, g.title, cd.title, lk.title, lk.url, \
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
/// (`closed` work items, `done`/`declined` proposals, `done` programs,
/// `Done`/`Cut` cards) are filtered out even if their marker somehow survived —
/// the write rules clear it, and this is the belt to that pair of braces. A
/// lane that accumulates answered asks is the failure the whole marker was
/// designed to avoid, one door over.
///
/// The card arm arrived late (#1389): `set_awaiting` invited marking a card
/// from the start and neither this filter nor [`settle_awaiting`] knew the
/// kind, so a card answered on the kanban kept its ghost.
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
            AND COALESCE(cd.status::text, 'Backlog') <> ALL({card_terminal}) \
          ORDER BY n.awaiting_since ASC, n.id ASC",
        card_terminal = sql_list(&CARD_TERMINAL_STATUSES)
    ))
    .fetch_all(pool)
    .await?)
}

/// A vocabulary array as a SQL literal list, so the clearing rule and the lane
/// filter both read [`CARD_TERMINAL_STATUSES`] rather than restating it. The
/// values are compile-time constants from `vocab`, never caller input.
fn sql_list(values: &[&str]) -> String {
    let inner = values
        .iter()
        .map(|v| format!("'{v}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("(ARRAY[{inner}])")
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
/// A card's version of that line is [`CARD_TERMINAL_STATUSES`] (#1389) — the
/// kanban is Ken's board end to end, so reaching its last column is him
/// answering, and `Cut` retires the ask the way archiving does. `update_card`
/// has always called this function; until #1389 it only ever inherited the
/// archived clause.
///
/// Called from every update path rather than a trigger — LB-2 settled that edge
/// and lifecycle rules live in core, the one path both transports share.
pub(super) async fn settle_awaiting<'e, E>(executor: E, node_id: i64) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(&format!(
        "UPDATE node n SET awaiting_since = NULL, awaiting_note = NULL \
         WHERE n.id = $1 AND n.awaiting_since IS NOT NULL AND ( \
               n.archived \
            OR EXISTS (SELECT 1 FROM workitem w \
                        WHERE w.node_id = n.id AND w.wi_status = 'closed') \
            OR EXISTS (SELECT 1 FROM sprint_proposal sp \
                        WHERE sp.node_id = n.id AND sp.status::text IN ('done', 'declined')) \
            OR EXISTS (SELECT 1 FROM program g \
                        WHERE g.node_id = n.id AND g.status = 'done') \
            OR EXISTS (SELECT 1 FROM card cd \
                        WHERE cd.node_id = n.id \
                          AND cd.status::text = ANY({card_terminal})))",
        card_terminal = sql_list(&CARD_TERMINAL_STATUSES)
    ))
    .bind(node_id)
    .execute(executor)
    .await?;
    Ok(())
}
