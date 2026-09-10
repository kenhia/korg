//! Programs: the multi-project layer (#968, sprint 044)
//!
//! 0022 made a proposal single-project and enforced it. That was only half an
//! answer: work that genuinely spans repos had nowhere legal to live, and the
//! corpus showed it — 13 legacy proposals covering work across projects, 4 of
//! them live in the queue, each one filed under whichever project the writer
//! picked first. A program is where that work goes.
//!
//! A program `includes` proposals, ordered (the edge carries the order, D-2), and
//! carries NO project of its own (D-6): its span is derived from its slices, so
//! the fact has one home and cannot go stale when a slice is added.

use anyhow::Result;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use ts_rs::TS;

use crate::error::RepoError;
use crate::ops::{self, schema};
use crate::relationships;
use crate::vocab::{
    self, PROGRAM_INITIAL_STATUS, PROGRAM_LIFTABLE_STATUSES, PROGRAM_LIVE_STATUSES,
    PROGRAM_STATUSES, PROPOSAL_STARTED_STATUSES, PROPOSAL_TERMINAL_STATUSES, WI_STATUSES,
    WI_UNFINISHED_STATUSES,
};

use super::awaiting::settle_awaiting;
use super::comments::Comment;
use super::common::{
    node_kind, parked_last, record_transition, report_date_fmt, require_kind, touch_node,
    validate_status,
};
use super::page::ArchivedFilter;
use super::relationships::{related_context, RelatedRef};
use super::work_items::WORKITEM_COMMENT_CAP;

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

    // #1424: a program is born `queued` — but only if that is *true*. A program
    // wrapped around a slice already running is in flight, and starting it
    // `queued` would strand it there: the promotion below fires on a slice
    // *changing* status, and this one already changed. Derived rather than
    // assumed, for the same reason the status is written at all instead of left
    // to the column default (#526, and 0023's default is what produced the bug).
    let started = slices_started(&mut *tx, &new.slices).await?;
    let status = if started {
        "active"
    } else {
        PROGRAM_INITIAL_STATUS
    };

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
        "INSERT INTO program (node_id, title, aim, notes, status, rank, pinned) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(node_id)
    .bind(&new.title)
    .bind(&new.aim)
    .bind(&new.notes)
    .bind(status)
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

/// Has any of these proposals been begun? (#1424, sprint 069.)
///
/// [`PROPOSAL_STARTED_STATUSES`] carries the definition and the reasoning —
/// `declined` is not a start, `done` is. Empty slice list is `false`: a program
/// with nothing in it has certainly not started.
async fn slices_started<'e, E>(executor: E, slices: &[i64]) -> Result<bool>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    if slices.is_empty() {
        return Ok(false);
    }
    let started: Vec<String> = PROPOSAL_STARTED_STATUSES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let any: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sprint_proposal \
                         WHERE node_id = ANY($1) AND status::text = ANY($2))",
    )
    .bind(slices)
    .bind(&started)
    .fetch_one(executor)
    .await?;
    Ok(any)
}

/// Move every `queued` program that includes this proposal to `active`, if the
/// proposal has been started (#1424, sprint 069).
///
/// `queued` asserts a fact about the slices — *none of them has begun* — so it
/// has to be maintained wherever that fact can change, or the state outlives the
/// condition and the Operations panel goes back to lying, just in the other
/// direction. Three paths can change it and all three call this or its sibling:
/// `update_proposal` (a slice starts under an existing program),
/// `create_program` (the program is built over a running slice, via
/// [`slices_started`]) and `relate` (a running slice is attached afterwards).
///
/// One statement, so it takes any executor and rides the caller's transaction:
/// the promotion and the change that caused it commit together or not at all.
/// The CTE writes the `transition` row itself, so a promotion is in the log
/// exactly as a hand-made one is (#977) — `record_transition` is not reachable
/// here because the rows are found and updated in the same query.
///
/// Deliberately one-directional and deliberately only out of `queued`.
/// `holding` is a statement somebody made on purpose — "resting between slices"
/// — and `done` is finished; neither is korg's to overturn off a slice edit.
/// Nothing demotes *into* `queued`, either: a program that has begun and paused
/// is `holding`.
///
/// **`parked` (#1535, sprint 072) inherits that sentence rather than extending
/// it**, and the WI asked for it to be decided rather than discovered. Parking
/// is a declaration — this whole line of work is dormant, regardless of what its
/// slices are doing — so a slice starting under a parked program must not lift
/// it, and a parked program may legitimately hold `active` slices.
/// `parked_programs_are_never_auto_promoted` asserts that against the database
/// instead of this comment asserting it in prose. That is the same distinction
/// `PROGRAM_STATUSES` draws between the derived state and the declared one.
///
/// **`soaking` joins `queued` here (#2151, sprint 079)** and is why this is no
/// longer called `promote_queued_programs_over`: the set it gates on is
/// [`PROGRAM_LIFTABLE_STATUSES`], and a name asserting one member of a two-member
/// set is the drift kfdc #1196 was about. Both are states meaning *nothing is in
/// flight*, and a slice starting is the fact that falsifies them. For `soaking`
/// this is the design's failure path: a failed soak returns the program to
/// `holding`, the fix arrives as a new slice, and that slice starting lifts the
/// program back to `active` — without every skill re-implementing it.
pub(super) async fn promote_programs_over<'e, E>(executor: E, proposal_node_id: i64) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let started: Vec<String> = PROPOSAL_STARTED_STATUSES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let liftable: Vec<String> = PROGRAM_LIFTABLE_STATUSES
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Two CTEs rather than one, because the transition log needs the status
    // each program moved OUT of and `UPDATE ... RETURNING` yields the new row.
    // With one liftable status that could be a bound literal; with two it
    // cannot, and a log that recorded every lift as leaving `queued` would
    // quietly falsify the one record of a soak having failed.
    sqlx::query(
        "WITH candidates AS ( \
             SELECT g.node_id, g.status AS from_status \
               FROM program g \
              WHERE g.status = ANY($2) \
                AND EXISTS ( \
                    SELECT 1 FROM relationship r \
                      JOIN sprint_proposal sp ON sp.node_id = r.right_id \
                     WHERE r.left_id = g.node_id \
                       AND r.relationship = 'includes' \
                       AND r.right_id = $1 \
                       AND sp.status::text = ANY($3)) \
         ), moved AS ( \
             UPDATE program g SET status = 'active' \
               FROM candidates c \
              WHERE c.node_id = g.node_id \
             RETURNING g.node_id, c.from_status) \
         INSERT INTO transition (node_id, from_status, to_status) \
         SELECT node_id, from_status, 'active' FROM moved",
    )
    .bind(proposal_node_id)
    .bind(&liftable)
    .bind(&started)
    .execute(executor)
    .await?;
    Ok(())
}

/// Entering `soaking` (#2151) — korg's first real transition rule on a program.
///
/// `update_program` has always validated a status for enum membership only, and
/// that stays true of every other value: `holding` and `done` are somebody's
/// decision and korg does not audit them. `soaking` is different because it
/// *asserts something about the slices* the way `queued` does — "all slice work
/// is done; what remains is a clock" — and an assertion korg emits on the board
/// is one korg should be able to stand behind.
///
/// One rule, deliberately, and no general transition matrix. Leaving `soaking`
/// is unconstrained in all three directions the design needs: `done` is the
/// explicit close (korg never closes a program itself), `holding` is a failed
/// soak, and `active` is the new slice that fixes it. A matrix would have to be
/// re-litigated every time the lifecycle grows a state, and the failure path
/// must stay frictionless — a soak that fails at 04:00 should become new work
/// without arguing with a state machine.
async fn check_soaking_entry<'e, E>(executor: E, node_id: i64) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let terminal: Vec<String> = PROPOSAL_TERMINAL_STATUSES
        .iter()
        .map(|s| s.to_string())
        .collect();
    let unfinished: Vec<String> = WI_UNFINISHED_STATUSES
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Both halves in one round trip: the count of slices that are NOT terminal,
    // and the count of soaks that are still unfinished.
    let (unterminal_slices, live_soaks): (i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT count(*) FROM relationship r \
              JOIN sprint_proposal sp ON sp.node_id = r.right_id \
             WHERE r.left_id = $1 AND r.relationship = 'includes' \
               AND sp.status::text <> ALL($2)), \
           (SELECT count(*) FROM relationship r \
              JOIN workitem w ON w.node_id = r.right_id \
             WHERE r.left_id = $1 AND r.relationship = $4 \
               AND w.wi_status = ANY($3))",
    )
    .bind(node_id)
    .bind(&terminal)
    .bind(&unfinished)
    .bind(relationships::SOAKS_LABEL)
    .fetch_one(executor)
    .await?;

    if unterminal_slices > 0 {
        return Err(RepoError::InvalidInput(format!(
            "cannot move program {node_id} to '{soaking}': {unterminal_slices} slice(s) are not \
             yet terminal ({terminal_list}). '{soaking}' means all slice work is done and only a \
             clock remains — finish or decline the outstanding slices first, or use 'holding' if \
             the program is simply between slices.",
            soaking = vocab::SOAKING_STATUS,
            terminal_list = PROPOSAL_TERMINAL_STATUSES.join(" / "),
        ))
        .into());
    }
    if live_soaks == 0 {
        return Err(RepoError::InvalidInput(format!(
            "cannot move program {node_id} to '{soaking}': it has no unfinished '{label}' edge to \
             a work item. '{soaking}' means waiting on extended tests, so there must be at least \
             one — relate the test work items to this program under the '{label}' label first, \
             and give each one 'check_after' and 'invalidated_if'. A program whose slices are all \
             done and which is soaking nothing is 'done', not '{soaking}'.",
            soaking = vocab::SOAKING_STATUS,
            label = relationships::SOAKS_LABEL,
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

/// The per-status `count(...) FILTER` list every covered-work rollup selects,
/// derived from [`WI_STATUSES`] rather than hand-listed (#1386, F-1).
///
/// Sprint 054 added `parked` to the vocabulary and to both partitions, and
/// missed the two aggregates that bucket covered work — so a parked item
/// counted toward `covered_count` and toward no bucket, and the counts stopped
/// summing to the total. Hand-written bucket lists are the drift surface;
/// generating them from the vocabulary is what stops status number six
/// repeating it. `count_expr` differs by call site (`*` under an inner join,
/// `w.node_id` under a left one), so it is the parameter; the column aliases
/// are the status names, which is what the row structs' field names must be —
/// fenced by `rollup_buckets_cover_wi_statuses`.
pub(super) fn covered_bucket_filters(count_expr: &str) -> String {
    WI_STATUSES
        .iter()
        .map(|status| {
            format!(", count({count_expr}) FILTER (WHERE w.wi_status = '{status}') AS {status} ")
        })
        .collect()
}

/// The outer half of [`covered_bucket_filters`] for a rollup that aggregates in
/// a CTE and left-joins it: one `coalesce` per bucket, from the same list, so
/// the two halves cannot name different sets of buckets.
pub(super) fn covered_bucket_coalesce(cte: &str) -> String {
    WI_STATUSES
        .iter()
        .map(|status| format!(", coalesce({cte}.{status}, 0) AS {status} "))
        .collect()
}

/// One slice of a program, with the rollup that stops a consumer crawling
/// (D-5). One count per [`WI_STATUSES`] value buckets the proposal's covered
/// work items — they sum to `covered_count` — so a board renders progress from
/// `get_program` alone.
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
    /// Deferred indefinitely (#810) — unfinished work that is not in flight.
    /// Missing here until #1386: parked items were counted in `covered_count`
    /// and nowhere else, so a consumer deriving open-by-subtraction read them
    /// as open.
    pub parked: i64,
}

/// One extended test holding a program in `soaking` (#2152) — a member of the
/// terminal array the `soaks` edge defines.
///
/// Carried on `get_program` and on the board beside `slices`, for the reason
/// `slices` exists at all (D-5): a consumer must never crawl. A bare edge ref
/// would say a soak exists and nothing about whether it can be judged yet, and
/// "ready to judge on the 13th unless kai's baseline is regenerated" is exactly
/// the fact a Delayed Ops row is made of.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct ProgramSoak {
    /// Since 0009 this is also the `wi_number` — one number everywhere. Both
    /// are emitted because a consumer rendering `#2058` and a consumer linking
    /// `/n/2058` should not have to know that.
    pub node_id: i64,
    pub wi_number: i64,
    pub title: String,
    /// The soak's own project. A program has none, and its soaks routinely
    /// span the repos its slices touched.
    pub project: Option<String>,
    pub wi_status: String,
    /// Earliest date this soak's evidence can be judged, `YYYY-MM-DD`. Never
    /// `None` in practice — the `soaks` edge refuses a member without it — but
    /// typed nullable because the column is, and a type that claims otherwise
    /// would be a claim about data korg does not guarantee after the fact.
    #[serde(with = "report_date_fmt::option")]
    #[ts(type = "string | null")]
    pub check_after: Option<time::Date>,
    /// What voids this test. Same nullability reasoning as `check_after`.
    pub invalidated_if: Option<String>,
    /// Position within the array. `None` on a soak related without a rank;
    /// those sort last, as `includes` does.
    #[ts(type = "string | null")]
    pub rank: Option<Decimal>,
}

/// The soak arrays for `program_ids`, each in rank order, paired with the
/// program they belong to.
///
/// One query for every program on purpose. `get_program` needs one array and
/// the board needs one per live program, and the board assembles its programs
/// synchronously — so the naive shape here is an `await` inside a `map`, which
/// is the N+1 that `slices` was explicitly built to avoid (D-5). Written once
/// and shared for the second reason too: two copies of this query is how the
/// two reads drift on what the array contains, and #2152's risk class is
/// precisely a fact with two homes.
pub(super) async fn program_soaks_for(
    pool: &PgPool,
    program_ids: &[i64],
) -> Result<Vec<(i64, ProgramSoak)>> {
    if program_ids.is_empty() {
        return Ok(Vec::new());
    }
    #[derive(sqlx::FromRow)]
    struct Row {
        program_id: i64,
        #[sqlx(flatten)]
        soak: ProgramSoak,
    }
    let rows = sqlx::query_as::<_, Row>(
        "SELECT r.left_id AS program_id, \
                w.node_id, w.wi_number, w.title, pj.name AS project, w.wi_status, \
                w.check_after, w.invalidated_if, r.rank \
         FROM relationship r \
         JOIN workitem w ON w.node_id = r.right_id \
         JOIN node n ON n.id = w.node_id \
         LEFT JOIN project pj ON pj.id = n.project_id \
         WHERE r.left_id = ANY($1) AND r.relationship = $2 \
         ORDER BY r.left_id ASC, r.rank ASC NULLS LAST, w.node_id ASC",
    )
    .bind(program_ids)
    .bind(relationships::SOAKS_LABEL)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|r| (r.program_id, r.soak)).collect())
}

/// The soak array for one program, in rank order.
pub(super) async fn program_soaks(pool: &PgPool, node_id: i64) -> Result<Vec<ProgramSoak>> {
    Ok(program_soaks_for(pool, &[node_id])
        .await?
        .into_iter()
        .map(|(_, soak)| soak)
        .collect())
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
    /// The extended tests holding this program in `soaking` (#2152), in rank
    /// order. Empty for the great majority of programs; a non-empty array is
    /// the precondition for entering `soaking` and the thing the soak scan
    /// reads.
    pub soaks: Vec<ProgramSoak>,
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
    let buckets = covered_bucket_filters("w.node_id");
    let slices = sqlx::query_as::<_, ProgramSlice>(&format!(
        "SELECT sp.node_id, sp.title, sp.status::text AS status, pj.name AS project, r.rank, \
                count(w.node_id) AS covered_count{buckets}\
         FROM relationship r \
         JOIN sprint_proposal sp ON sp.node_id = r.right_id \
         JOIN node sn ON sn.id = sp.node_id \
         LEFT JOIN project pj ON pj.id = sn.project_id \
         LEFT JOIN relationship cov ON cov.left_id = sp.node_id AND cov.relationship = 'covers' \
         LEFT JOIN workitem w ON w.node_id = cov.right_id \
         WHERE r.left_id = $1 AND r.relationship = 'includes' \
         GROUP BY sp.node_id, sp.title, sp.status, pj.name, r.rank \
         ORDER BY r.rank ASC NULLS LAST, sp.node_id ASC"
    ))
    .bind(node_id)
    .fetch_all(pool)
    .await?;
    let comments = sqlx::query_as::<_, Comment>(
        "SELECT id, node_id, body, origin, created, updated FROM comment \
         WHERE node_id = $1 ORDER BY created LIMIT $2",
    )
    .bind(node_id)
    .bind(WORKITEM_COMMENT_CAP)
    .fetch_all(pool)
    .await?;
    let comments_truncated = program.comment_count > WORKITEM_COMMENT_CAP;
    let soaks = program_soaks(pool, node_id).await?;
    let (related, related_truncated) =
        related_context(pool, node_id, &["includes", relationships::SOAKS_LABEL]).await?;
    Ok(Some(ProgramDetail {
        program,
        slices,
        soaks,
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
    // Parked below the line (#1535), the same outermost key the proposal queue
    // reads use — Operations and Planning draw one divider, not two.
    let parked = parked_last("g.status");
    let items = sqlx::query_as::<_, ProgramRow>(&format!(
        "{PROGRAM_SELECT} WHERE ($1::text[] IS NULL OR g.status = ANY($1)) \
            AND ($2::bool IS NULL OR n.archived = $2) \
          ORDER BY {parked}, g.pinned DESC, g.rank ASC, g.node_id ASC"
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

/// Absent → the live set (`queued` + `active` + `holding`); `"all"` → no filter;
/// anything
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
        // Read-before-write (#977), as in `update_proposal`.
        let before: String = sqlx::query_scalar("SELECT status FROM program WHERE node_id = $1")
            .bind(node_id)
            .fetch_one(&mut *tx)
            .await?;
        // The one entry rule (#2151), checked only on the way IN and only when
        // the status actually changes — re-asserting `soaking` on a program
        // already soaking must not fail once its soaks start being judged, or
        // an unrelated `update_program` call would be refused for a reason that
        // has nothing to do with it.
        if v == vocab::SOAKING_STATUS && before != vocab::SOAKING_STATUS {
            check_soaking_entry(&mut *tx, node_id).await?;
        }
        sqlx::query("UPDATE program SET status = $2 WHERE node_id = $1")
            .bind(node_id)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        record_transition(&mut *tx, node_id, &before, v).await?;
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
