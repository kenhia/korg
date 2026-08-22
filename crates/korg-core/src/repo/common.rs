//! Plumbing every module in this layer reaches for: the existence and kind
//! checks every mutation opens with, the `node.updated` and transition-log
//! writes that follow one, and the date format the dated surfaces share.

use anyhow::Result;
use sqlx::{Executor, PgPool, Postgres};

use crate::error::RepoError;
use crate::vocab;

/// The leading `ORDER BY` term that puts parked rows below the line (#1534 /
/// #1535, sprint 072) — the SQL half of the divider #810 drew for work items.
///
/// One helper rather than three hand-written comparisons because three reads
/// need it (the two proposal queue reads, the board, and `list_programs`) and
/// they must agree: a consumer that sees parked rows last in `list_proposals`
/// and interleaved on the board has to write its own sort to reconcile them,
/// which is exactly the derivation GP-19 forbids.
///
/// `false` sorts before `true` in Postgres, so unparked rows come first with no
/// `DESC` and no `CASE`. The literal comes from [`vocab::PARKED_STATUS`], so it
/// cannot drift from the vocabularies it is filtering on; `column` is a caller-
/// supplied SQL fragment (`p.status::text`, `g.status`), never user input.
pub(super) fn parked_last(column: &str) -> String {
    format!("({column} = '{}') ASC", vocab::PARKED_STATUS)
}

pub(super) fn validate_status(value: &str, allowed: &[&str], what: &str) -> Result<()> {
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
pub(super) fn require_non_empty(value: &str, what: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(RepoError::invalid(format!("{what} must not be empty")).into());
    }
    Ok(())
}

/// Every mutation starts here (WI #525): the target must exist *and* be the
/// kind the operation is about. Without the kind half, `update_card` against a
/// work item's node id silently archived the work item and reported success —
/// exactly the slip an agent makes now that `wi_number == node_id`.
pub(super) async fn require_kind<'e, E>(
    executor: E,
    node_id: i64,
    kind: &str,
    what: &str,
) -> Result<()>
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
pub(super) async fn require_node<'e, E>(executor: E, node_id: i64) -> Result<()>
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
pub(super) fn cross_project_covers(
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
pub(super) async fn node_project<'e, E>(executor: E, node_id: i64) -> Result<Option<String>>
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
pub(super) async fn wi_handle<'e, E>(executor: E, node_id: i64) -> Result<(i64, String)>
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
pub(super) async fn node_kind(pool: &PgPool, node_id: i64) -> Result<String> {
    sqlx::query_scalar::<_, String>("SELECT kind FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| RepoError::NotFound(format!("no node with id {node_id}")).into())
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
pub(super) async fn touch_node<'e, E>(executor: E, node_id: i64) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query("UPDATE node SET updated = now() WHERE id = $1")
        .bind(node_id)
        .execute(executor)
        .await?;
    Ok(())
}

/// Append a status transition to the log (#977), or do nothing if the status
/// did not actually move.
///
/// **The no-op guard is the point.** `node.updated` cannot serve a "recently
/// shipped" feed because it advances on any edit — a proposal touched for its
/// tags dates as newly shipped. A transition log that recorded every status
/// *write* rather than every status *change* would rebuild that same lie one
/// table over: korg's own post-deploy check re-PATCHes a status to the value it
/// already holds, and agents re-set statuses routinely. So the caller passes
/// what it read and what it is about to write, and equality means silence.
/// Migration 0026's `transition_actually_changed` CHECK is the backstop.
///
/// Takes the transaction the status write itself is in, so an event cannot
/// survive a rolled-back update — the same discipline `advance_completed_anchor`
/// follows, and for the same reason.
///
/// Called from the three update paths that hook [`settle_awaiting`]. Schedules
/// are deliberately absent: `materialize_schedule` moves a `once` schedule to
/// `done` outside `update_schedule`, so hooking only the update path would give
/// that kind a half-recorded history, and a feed that is silently partial for
/// one kind is worse than one that never claims to cover it. A schedule's real
/// history is its `materializes` edges, which already record every firing.
pub(super) async fn record_transition<'e, E>(
    executor: E,
    node_id: i64,
    from: &str,
    to: &str,
) -> Result<()>
where
    E: Executor<'e, Database = Postgres>,
{
    if from == to {
        return Ok(());
    }
    sqlx::query("INSERT INTO transition (node_id, from_status, to_status) VALUES ($1, $2, $3)")
        .bind(node_id)
        .bind(from)
        .bind(to)
        .execute(executor)
        .await?;
    Ok(())
}

// The `YYYY-MM-DD` wire format for a bare date, shared by the report
// surfaces and the flow series — one adapter so two surfaces cannot
// disagree about what a date looks like on the wire.
time::serde::format_description!(pub(super) report_date_fmt, Date, "[year]-[month]-[day]");
