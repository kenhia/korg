//! The planning rail rollup (WI #823).

use anyhow::Result;
use serde::Serialize;
use sqlx::PgPool;
use ts_rs::TS;

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
