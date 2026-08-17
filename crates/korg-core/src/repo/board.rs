//! The board rollup (WI #970) — the one-call read behind korg's front page.

use anyhow::Result;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use ts_rs::TS;

use crate::relationships;
use crate::vocab::{
    PROGRAM_TERMINAL_STATUSES, PROPOSAL_LIVE_STATUSES, PROPOSAL_TERMINAL_STATUSES,
    WI_FINISHED_STATUSES,
};

use super::awaiting::{list_awaiting, AwaitingRow};
use super::page::archived_default;
use super::planning::{planning_rollup, PlanningRollupRow};
use super::programs::{
    covered_bucket_coalesce, covered_bucket_filters, list_programs, ProgramList, ProgramOmitted,
    ProgramRow, ProgramSlice,
};
use super::proposals::{proposal_omitted, ProposalOmitted};
use super::reports::{list_report_sources, list_reports, ReportRow, SourceHealth};
use super::schedules::{list_schedules, ScheduleRow};

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
    /// The work items this proposal covers, and their statuses. There is one
    /// count per `WI_STATUSES` value and they sum to `covered_count` — a
    /// property the `rollup_buckets_cover_wi_statuses` fence keeps true rather
    /// than this comment claiming it (#1386: it claimed four for the three
    /// sprints `parked` had been the fifth).
    pub covered_count: i64,
    pub open: i64,
    pub resolved: i64,
    pub done: i64,
    pub closed: i64,
    /// Deferred indefinitely (#810). Unfinished, so it counts against progress
    /// the way `open` does, not toward it.
    pub parked: i64,
    /// When the proposal row last changed. korg's only staleness signal: it is
    /// *last touched*, not last progressed, which is why the board does not
    /// build an event feed out of it (D-7).
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
    /// The curated synopsis (korg #1003): the newest [`CURATOR_MARKER`]-opened
    /// comment on this proposal, or `None` when the curator hasn't written one.
    pub synopsis: Option<BoardSynopsis>,
}

/// The curated-synopsis comment convention (korg #1003, kfdc sprint 003).
///
/// A comment whose body **starts with** this marker is a machine-curated
/// synopsis of the node it sits on — one per node, updated in place by the
/// curator (never appended per pass), with a `mined from:` provenance trailer.
/// The board surfaces the newest such comment per live proposal as
/// [`BoardProposal::synopsis`]; unmarked comments never ride the board.
pub const CURATOR_MARKER: &str = "⟦curator⟧";

/// A curated synopsis — the newest [`CURATOR_MARKER`]-opened comment on a live
/// board proposal. `body` is verbatim (marker and trailer included: the writer's
/// convention is the reader's to render), `updated` is the comment's own stamp,
/// so a consumer can say how fresh the curation is.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardSynopsis {
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub updated: OffsetDateTime,
}

/// An edge between two live board proposals — Deconfliction's substrate
/// (korg #1003). Only edges whose **both** endpoints are `active`/`queue` rows
/// ride the board: an edge to a done/declined/archived proposal is history,
/// not a collision, and slice-only rows fetched for Operations don't smuggle
/// theirs in.
///
/// This is the first read surface for D-17's write-side edge provenance:
/// `origin` (self-reported, unverified — "kfdc-curator" vs "web" is how curated
/// edges stay distinguishable from human ones) and `created` (Postgres's
/// clock, insert-time).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardProposalEdge {
    /// The edge's stored left endpoint; the label reads left → right.
    pub left: i64,
    pub right: i64,
    pub label: String,
    /// From the registry: when `false` the orientation is storage accident and
    /// the edge must be read symmetrically.
    pub directed: bool,
    pub origin: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub created: OffsetDateTime,
}

/// How many transitions the board's ticker carries (#977).
///
/// Four times [`BOARD_REPORT_CAP`], and the asymmetry is deliberate. A report is
/// a whole document and five of them is a panel; a transition is one line, and a
/// ticker showing five would cover about a day of a working sprint — short
/// enough that "nothing shipped this week" and "five things shipped this
/// morning" render identically. Twenty covers a sprint's worth of movement at
/// korg's actual write rate, and the series lives behind the node's own history
/// for anyone who wants more.
pub const BOARD_EVENT_CAP: i64 = 20;

/// One line of the board's ticker: a status change that actually happened
/// (#977).
///
/// Every field but `from_status`/`to_status`/`at` is resolved here so a ticker
/// renders a line without a follow-up read per event — the same reason
/// [`AwaitingRow`] carries a title and a status.
///
/// **What is not here, and why the absence is the feature.** There is no actor:
/// korg has no authenticated writer, and a self-reported one would read as
/// provenance while being nothing of the sort. There is no free-text label:
/// a transition is `from` → `to` and the renderer names it, so the feed cannot
/// drift from the statuses it describes.
#[derive(Debug, Clone, sqlx::FromRow, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardEvent {
    pub node_id: i64,
    /// `workitem`, `sprint_proposal` or `program` — the three kinds whose
    /// update paths write the log.
    pub kind: String,
    /// Present when the node is a work item — its user-facing handle.
    pub wi_number: Option<i64>,
    pub title: String,
    pub project: Option<String>,
    pub from_status: String,
    pub to_status: String,
    /// When it happened, from Postgres's clock — the same one
    /// [`BoardRollup::generated`] reads, so an age computed against it is right.
    #[serde(with = "time::serde::rfc3339")]
    #[ts(type = "string")]
    pub at: OffsetDateTime,
}

/// The board's ticker: the newest [`BOARD_EVENT_CAP`] status transitions
/// (#977), newest first.
///
/// Archived nodes are excluded — they are out of every other panel, and an
/// event feed is the wrong place to reintroduce them. `id` breaks the `at` tie
/// because two transitions committed in one transaction share `now()` to the
/// microsecond, and a ticker that reshuffles between refreshes is one nobody
/// reads twice.
pub async fn list_transitions(pool: &PgPool, limit: i64) -> Result<Vec<BoardEvent>> {
    Ok(sqlx::query_as::<_, BoardEvent>(
        "SELECT t.node_id, n.kind, w.wi_number, \
                COALESCE(w.title, sp.title, g.title, n.kind || ' #' || n.id) AS title, \
                pj.name AS project, t.from_status, t.to_status, t.at \
           FROM transition t \
           JOIN node n ON n.id = t.node_id \
           LEFT JOIN workitem w         ON w.node_id  = n.id \
           LEFT JOIN sprint_proposal sp ON sp.node_id = n.id \
           LEFT JOIN program g          ON g.node_id  = n.id \
           LEFT JOIN project pj         ON pj.id = n.project_id \
          WHERE NOT n.archived \
          ORDER BY t.at DESC, t.id DESC \
          LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

/// One unmet dependency holding up a live board row — Deconfliction's
/// deterministic half (#978).
///
/// The concept renders a card naming both sides (`korg 825 ⟂ korg #861`), which
/// is why this is the blocking *ref* and not a boolean: a boolean cannot be
/// widened later without another contract change, and it cannot name the thing
/// the reader has to go look at.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct BoardBlocker {
    /// The `active`/`queue` row that cannot proceed.
    pub proposal: i64,
    /// Where the dependency was written — `proposal` when the proposal itself
    /// carries the `depends_on` edge, `covered` when one of its covered work
    /// items does. Both count (D-3), and they are distinguished rather than
    /// merged because they mean different things to a reader: the first is a
    /// sequencing decision about the sprint, the second is one task inside it
    /// waiting on something outside.
    pub via: String,
    /// The node carrying the edge: the proposal itself, or the covered item.
    pub dependent: i64,
    pub dependent_wi_number: Option<i64>,
    /// What is depended on, resolved enough to render without a follow-up read.
    pub blocker: i64,
    pub blocker_kind: String,
    pub blocker_wi_number: Option<i64>,
    pub blocker_title: String,
    pub blocker_project: Option<String>,
    pub blocker_status: String,
    /// The `program` whose ordered slices already express this dependency —
    /// set when the blocked proposal **and** the blocker's owning proposal are
    /// both slices of the same live program, `null` otherwise.
    ///
    /// This field is kfdc #1070's answer (D-5). Ken's objection to the
    /// Deconfliction panel was that for program-ordered work it "is essentially
    /// showing the same data twice, in Operations and Deconfliction" — and he is
    /// right, but the fix cannot live in kfdc, because **kfdc cannot filter what
    /// korg did not label**. korg says which dependencies Operations is already
    /// rendering as sequence; the render decides whether to show them. Neither
    /// side has to guess, and korg does not lose a true fact to make one panel
    /// tidier.
    pub sequenced_by: Option<i64>,
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
    /// Deconfliction: every edge whose both endpoints are `active`/`queue`
    /// rows, with label, registry `directed`, and D-17 provenance. See
    /// [`BoardProposalEdge`].
    pub proposal_edges: Vec<BoardProposalEdge>,
    /// Deconfliction, the deterministic half (#978): every unmet `depends_on`
    /// holding up a live row, one entry per (row, blocker) pair. See
    /// [`BoardBlocker`] — in particular `sequenced_by`, which is how a consumer
    /// tells a real collision from work Operations already orders.
    ///
    /// Distinct from `proposal_edges` and not a superset of it: that list is
    /// *every* edge between two live rows whatever it says, this one is the
    /// subset that means "cannot start yet", widened to blockers that are not
    /// proposals at all. An edge to a finished or archived node appears in
    /// neither, because a satisfied dependency is not a blocker.
    pub blocked: Vec<BoardBlocker>,
    /// Operations: live programs with their ordered slices.
    pub programs: Vec<BoardProgram>,
    pub programs_omitted: ProgramOmitted,
    /// Commander's Call: everything waiting on Ken, oldest ask first.
    pub awaiting: Vec<AwaitingRow>,
    /// Queue depth per project — every project, with its `status`, so the board
    /// can count the active ones and dim the rest.
    pub depth: Vec<PlanningRollupRow>,
    /// Sensor Net: the newest [`BOARD_REPORT_CAP`] reports. `report_date` is the
    /// only date in korg that records when something *happened* to the fleet.
    pub reports: Vec<ReportRow>,
    /// The ticker: the newest [`BOARD_EVENT_CAP`] status transitions, newest
    /// first (#977, sprint 053 — sprint 045's D-7, now built).
    ///
    /// **Read the cold-start behaviour before wiring a panel to this.** The
    /// transition log begins at migration 0026 and was deliberately not
    /// backfilled: no honest history existed to backfill *from*, and
    /// manufacturing one out of `node.updated` is the precise lie the log was
    /// built to stop telling. So this is empty on a freshly migrated corpus and
    /// fills forward. An empty ticker means "nothing has moved since 0026", not
    /// "nothing has ever moved", and a renderer that cannot say that difference
    /// out loud should render nothing rather than "no recent activity".
    pub events: Vec<BoardEvent>,
    /// Sensor Net, the other half (#950, sprint 051): every known reporting
    /// source and whether korg can currently believe it.
    ///
    /// **Unbounded, unlike `reports`.** `BOARD_REPORT_CAP` is defensible because
    /// a board renders a handful of recent lines and the series lives behind
    /// `list_reports`; a cap here would mean a stale source could be pushed off
    /// the board by fresher ones, which is the failure mode inverted. There is
    /// one row per source — single digits — and the ordering already puts every
    /// `stale` row first.
    pub sources: Vec<SourceHealth>,
    /// What is **due right now**, soonest-due first (#1385, sprint 063).
    ///
    /// The finding this answers: schedule 1112 sat due for nine days and
    /// nothing that gets looked at daily could say so. Due-ness was computed
    /// only on `/schedules` and `list_schedules` — the two places you go when
    /// you are already thinking about schedules. That is #950's own lesson ("a
    /// channel nobody looks at is not a channel") reappearing in the feature
    /// built in its honour, and the fix is the same shape #950's was: put it on
    /// the read every daily surface already makes.
    ///
    /// **This is `list_schedules(due_only)`'s rows, not a second predicate.**
    /// Due-ness has exactly one definition (`schedule_due_sql`), so the board
    /// inherits all three of its clauses — active, interval elapsed, and no
    /// outstanding materialised item — including the last one, which is what
    /// stops the block nagging about a drill whose work item is already open
    /// and sitting in the queue.
    ///
    /// **Uncapped, like `sources` and for the same reason**: a cap sorts by
    /// due-ness, so the row it would drop is the one that has been waiting
    /// longest — the exact failure the block exists to catch, inverted. The set
    /// is bounded in practice by construction, since a schedule that fires
    /// leaves the block until its item is finished.
    ///
    /// The rows are `ScheduleRow` — the type `list_schedules` and
    /// `get_schedule` already return (the D-4 move `programs` made with
    /// `ProgramRow`), so a consumer renders a due schedule with the component
    /// it already has. `preview_title` is what materialising would create right
    /// now, substitutions applied, so a panel needs no follow-up read.
    pub due_schedules: Vec<ScheduleRow>,
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
    parked: i64,
    updated: OffsetDateTime,
}

impl BoardProposalRow {
    fn into_proposal(self, synopsis: Option<BoardSynopsis>) -> BoardProposal {
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
            parked: self.parked,
            updated: self.updated,
            synopsis,
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
            parked: self.parked,
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
    let buckets = covered_bucket_filters("*");
    let coalesced = covered_bucket_coalesce("cov");
    let rows = sqlx::query_as::<_, BoardProposalRow>(&format!(
        "WITH cov AS ( \
             SELECT r.left_id AS proposal_id, \
                    count(*) AS covered_count{buckets}\
               FROM relationship r \
               JOIN workitem w ON w.node_id = r.right_id \
              WHERE r.relationship = 'covers' \
              GROUP BY r.left_id \
         ) \
         SELECT sp.node_id, sp.title, sp.summary, sp.status::text AS status, \
                pj.name AS project, sp.rank, sp.pinned, n.archived, \
                (SELECT count(*) FROM comment cm WHERE cm.node_id = sp.node_id) \
                                                              AS comment_count, \
                coalesce(cov.covered_count, 0) AS covered_count{coalesced}, \
                n.updated \
           FROM sprint_proposal sp \
           JOIN node n ON n.id = sp.node_id \
           LEFT JOIN project pj ON pj.id = n.project_id \
           LEFT JOIN cov ON cov.proposal_id = sp.node_id \
          WHERE (sp.status::text = ANY($1) AND NOT n.archived) \
             OR sp.node_id = ANY($2) \
          ORDER BY sp.pinned DESC, sp.rank ASC, sp.node_id ASC"
    ))
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

    // The curated layer (korg #1003) keys off the *board* rows — live status
    // and unarchived — not the fetched set, so a slice-only row contributes
    // neither edges nor a synopsis.
    let live_ids: Vec<i64> = rows
        .iter()
        .filter(|r| !r.archived && matches!(r.status.as_str(), "active" | "proposed"))
        .map(|r| r.node_id)
        .collect();

    let proposal_edges = sqlx::query_as::<_, (i64, i64, String, Option<String>, OffsetDateTime)>(
        "SELECT r.left_id, r.right_id, r.relationship, r.origin, r.created \
           FROM relationship r \
          WHERE r.left_id = ANY($1) AND r.right_id = ANY($1) \
          ORDER BY r.id ASC",
    )
    .bind(&live_ids)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(left, right, label, origin, created)| BoardProposalEdge {
        directed: relationships::direction_is_meaningful(&label),
        left,
        right,
        label,
        origin,
        created,
    })
    .collect();

    let blocked = board_blockers(pool, &live_ids, &slice_edges).await?;

    // Newest marker-opened comment per live row. `updated` orders because the
    // curator edits its one comment in place (CURATOR_MARKER's contract), so
    // insertion order goes stale the first time a synopsis is refreshed.
    let mut synopses: std::collections::HashMap<i64, BoardSynopsis> =
        sqlx::query_as::<_, (i64, String, OffsetDateTime)>(
            "SELECT DISTINCT ON (cm.node_id) cm.node_id, cm.body, cm.updated \
               FROM comment cm \
              WHERE cm.node_id = ANY($1) AND cm.body LIKE $2 \
              ORDER BY cm.node_id, cm.updated DESC",
        )
        .bind(&live_ids)
        .bind(format!("{CURATOR_MARKER}%"))
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|(node_id, body, updated)| (node_id, BoardSynopsis { body, updated }))
        .collect();

    // A slice-only row is not part of the queue: it was fetched for Operations,
    // and its own status (or its archived flag) is what decides.
    let (mut active, mut queue) = (Vec::new(), Vec::new());
    for row in rows {
        if row.archived {
            continue;
        }
        let synopsis = synopses.remove(&row.node_id);
        match row.status.as_str() {
            "active" => active.push(row.into_proposal(synopsis)),
            "proposed" => queue.push(row.into_proposal(synopsis)),
            _ => {}
        }
    }

    Ok(BoardRollup {
        generated,
        active,
        queue,
        proposals_omitted: proposal_omitted(pool, None, archived_default(), Some(&live_statuses))
            .await?,
        proposal_edges,
        blocked,
        programs,
        programs_omitted,
        awaiting: list_awaiting(pool).await?,
        depth: planning_rollup(pool).await?,
        reports: list_reports(pool, None, BOARD_REPORT_CAP).await?,
        events: list_transitions(pool, BOARD_EVENT_CAP).await?,
        sources: list_report_sources(pool).await?,
        // Live schedules only (the `list_schedules` default), unarchived, and
        // due — through the list read itself, so the board cannot drift from
        // `/schedules` about what "due" means.
        due_schedules: list_schedules(pool, None, None, true, archived_default())
            .await?
            .items,
    })
}

/// The blocker pass (#978): which live board rows cannot start, and on what.
///
/// Three queries and no new data — `depends_on` edges are already real, typed
/// and deterministic, which is the entire case the WI makes for building this
/// rather than mining it out of prose.
///
/// **Where the four design questions landed** (#978 listed them as questions
/// with several defensible answers each; these are the answers, and each one is
/// pinned by a test in `sprint053.rs`):
///
/// 1. **Granularity — both.** A proposal is blocked when it depends on
///    something unfinished *or* when a work item it covers does. `via`
///    distinguishes them so a render can choose; merging them would have thrown
///    away the distinction for good.
/// 2. **Doneness — from the vocabulary, per kind.** [`WI_FINISHED_STATUSES`]
///    for work items (**not** `WI_TERMINAL_STATUSES` — see that const's docs for
///    the bug that confusion produces), [`PROPOSAL_TERMINAL_STATUSES`] for
///    proposals, [`PROGRAM_TERMINAL_STATUSES`] for programs. Deriving rather
///    than hardcoding is what makes a future status handled here for free.
/// 3. **Transitivity — one hop.** A closure is a different query and a
///    different UI, and nothing has asked for one. A blocked blocker still
///    appears in its own right, so the chain is visible without being computed.
/// 4. **Shape — the refs.** See [`BoardBlocker`].
///
/// Two exclusions that are decisions rather than oversights:
///
/// * **An archived blocker does not block.** It is out of every other view, and
///   a row held blocked forever by something nobody will ever do is worse than
///   a row that quietly frees up. What archiving a depended-on node *should*
///   surface is a dangling-dependency warning — a different card, and one
///   nothing has asked for.
/// * **A finished covered item's dependencies do not block.** If a covered work
///   item is `done` and it depended on something still open, the proposal is not
///   waiting on it: the work happened anyway. Only an unfinished dependent can
///   be stuck.
///
/// A `depends_on` whose target is a kind with no lifecycle korg tracks (a link,
/// a report, a handoff) is not a blocker: korg cannot say whether such a node is
/// "finished", and inventing an answer is the failure mode this panel exists to
/// avoid.
async fn board_blockers(
    pool: &PgPool,
    live_ids: &[i64],
    slice_edges: &[(i64, i64, Option<Decimal>)],
) -> Result<Vec<BoardBlocker>> {
    let wi_finished: Vec<String> = WI_FINISHED_STATUSES.iter().map(|s| s.to_string()).collect();

    // Both granularities in one pass. The `covered` arm requires the covered
    // item to be unfinished and unarchived — a `done` task's own dependencies
    // are nobody's blocker.
    let deps = sqlx::query_as::<_, (String, i64, i64, Option<i64>, i64)>(
        "SELECT 'proposal' AS via, r.left_id AS proposal, r.left_id AS dependent, \
                NULL::bigint AS dependent_wi_number, r.right_id AS blocker \
           FROM relationship r \
          WHERE r.relationship = 'depends_on' AND r.left_id = ANY($1) \
          UNION ALL \
         SELECT 'covered', c.left_id, r.left_id, w.wi_number, r.right_id \
           FROM relationship c \
           JOIN relationship r ON r.left_id = c.right_id \
                              AND r.relationship = 'depends_on' \
           JOIN workitem w ON w.node_id = c.right_id \
           JOIN node dn    ON dn.id     = c.right_id \
          WHERE c.relationship = 'covers' AND c.left_id = ANY($1) \
            AND NOT (w.wi_status = ANY($2)) AND NOT dn.archived",
    )
    .bind(live_ids)
    .bind(&wi_finished)
    .fetch_all(pool)
    .await?;

    if deps.is_empty() {
        return Ok(Vec::new());
    }

    // Resolve every candidate blocker once, across kinds — the same COALESCE
    // shape `AWAITING_SELECT` uses, for the same reason.
    let blocker_ids: Vec<i64> = deps.iter().map(|(_, _, _, _, b)| *b).collect();
    let resolved = sqlx::query_as::<
        _,
        (
            i64,
            String,
            bool,
            Option<i64>,
            String,
            Option<String>,
            Option<String>,
        ),
    >(
        "SELECT n.id, n.kind, n.archived, w.wi_number, \
                COALESCE(w.title, sp.title, g.title, cd.title, lk.title, lk.url, \
                         rp.summary, hd.title, n.kind || ' #' || n.id) AS title, \
                pj.name AS project, \
                COALESCE(w.wi_status, sp.status::text, g.status) AS status \
           FROM node n \
           LEFT JOIN workitem w         ON w.node_id  = n.id \
           LEFT JOIN sprint_proposal sp ON sp.node_id = n.id \
           LEFT JOIN program g          ON g.node_id  = n.id \
           LEFT JOIN card cd            ON cd.node_id = n.id \
           LEFT JOIN link lk            ON lk.node_id = n.id \
           LEFT JOIN report rp          ON rp.node_id = n.id \
           LEFT JOIN handoff hd         ON hd.node_id = n.id \
           LEFT JOIN project pj         ON pj.id = n.project_id \
          WHERE n.id = ANY($1)",
    )
    .bind(&blocker_ids)
    .fetch_all(pool)
    .await?;

    // The blocker's owning proposal, for the `sequenced_by` test below. A
    // blocker that IS a proposal owns itself.
    let owners: std::collections::HashMap<i64, i64> = sqlx::query_as::<_, (i64, i64)>(
        "SELECT r.right_id, r.left_id FROM relationship r \
              WHERE r.relationship = 'covers' AND r.right_id = ANY($1)",
    )
    .bind(&blocker_ids)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    // Slice membership, from the edges the caller already fetched — so
    // `sequenced_by` can only ever name a program the Operations panel is
    // actually rendering.
    let mut programs_of: std::collections::HashMap<i64, Vec<i64>> =
        std::collections::HashMap::new();
    for (program, proposal, _) in slice_edges {
        programs_of.entry(*proposal).or_default().push(*program);
    }

    let mut out = Vec::new();
    for (via, proposal, dependent, dependent_wi_number, blocker) in deps {
        let Some((_, kind, archived, wi_number, title, project, status)) =
            resolved.iter().find(|r| r.0 == blocker)
        else {
            continue;
        };
        if *archived {
            continue;
        }
        // A kind with no lifecycle status resolves to NULL and is not a blocker.
        let Some(status) = status else { continue };
        let unfinished = match kind.as_str() {
            "workitem" => !WI_FINISHED_STATUSES.contains(&status.as_str()),
            "sprint_proposal" => !PROPOSAL_TERMINAL_STATUSES.contains(&status.as_str()),
            "program" => !PROGRAM_TERMINAL_STATUSES.contains(&status.as_str()),
            _ => false,
        };
        if !unfinished {
            continue;
        }
        // Both ends slices of one live program => Operations already draws it.
        let owning = owners.get(&blocker).copied().unwrap_or(blocker);
        let sequenced_by = programs_of.get(&proposal).and_then(|mine| {
            programs_of
                .get(&owning)
                .and_then(|theirs| mine.iter().find(|p| theirs.contains(p)).copied())
        });
        out.push(BoardBlocker {
            proposal,
            via,
            dependent,
            dependent_wi_number,
            blocker,
            blocker_kind: kind.clone(),
            blocker_wi_number: *wi_number,
            blocker_title: title.clone(),
            blocker_project: project.clone(),
            blocker_status: status.clone(),
            sequenced_by,
        });
    }
    // Stable across calls: equal ranks used to shuffle rows (F-19) and a
    // Deconfliction card that moves between refreshes is one nobody trusts.
    out.sort_by_key(|b| (b.proposal, b.blocker, b.dependent, b.via.clone()));
    Ok(out)
}
