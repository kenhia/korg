//! Sprint 042's measurement — the one the proposal insisted on rather than
//! assumed (#813: "deserves a measurement rather than an assumption"; Ken on
//! #823: *"the right SQL query makes the first one quick enough, but timing
//! the op beats guessing"*).
//!
//! `#[ignore]`d: it seeds a production-scale corpus and times queries, which
//! is minutes of CI for a number nobody reads on every push. Run it
//! deliberately:
//!
//! ```sh
//! cargo test -p korg-core --test sprint042_measure -- --ignored --nocapture
//! ```
//!
//! What it answers: this sprint added two membership markers to the **two
//! hottest list reads**, plus a per-project aggregate behind the Planning
//! rail. Two questions, not one — *how much does it cost*, and *which SQL
//! shape*. Both WIs sketched correlated subqueries; the bake-off below says
//! pre-aggregated joins are roughly twice as fast on both reads, which is why
//! the repo ships joins and the WIs' drafts are preserved here as the losing
//! variant rather than in the code.
//!
//! Everything is measured against the *old* SQL on the *same* rows in the
//! *same* session — never a remembered number, and never a repo function
//! against a raw statement (that mistake is documented inline below; it
//! manufactured a +960% that did not exist).

use korg_core::repo::{
    archived_default, create_handoff, create_project, create_proposal, create_work_item,
    list_work_items, list_work_items_lean, node_id_for_wi, planning_rollup, NewHandoff,
    NewProposal, NewWorkItem, WorkItemQuery,
};
use korg_test_support::fresh_korg;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::time::Instant;

/// Production shape as of 2026-08-05, rounded up: ~30 active projects, ~900
/// work items, ~90 proposals. Seeded a little larger so the numbers are an
/// upper bound rather than a fit.
const PROJECTS: usize = 30;
const WORK_ITEMS: usize = 1_000;
const PROPOSALS: usize = 100;
const COVERS_PER_PROPOSAL: usize = 3;
const HANDOFFS: usize = 60;

/// The pre-042 `WORKITEM_SELECT`, kept verbatim so "before" is a real
/// baseline on the same rows rather than a guess. If the live select changes
/// shape, this drifts — which is fine: it is a measurement harness, not a
/// contract, and the sprint record carries the numbers it produced.
const OLD_WORKITEM_SELECT: &str = "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, \
        (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
        n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id \
     WHERE ($1::text IS NULL OR pj.name = $1) \
       AND ($2::bool IS NULL OR n.archived = $2) \
     ORDER BY w.wi_number LIMIT $3 OFFSET $4";

const OLD_LEAN_SELECT: &str = "SELECT w.wi_number, w.node_id, pj.name AS project, w.title, \
            w.wi_type, w.wi_status, w.wi_tshirt, \
            (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     WHERE ($1::text IS NULL OR pj.name = $1) \
       AND ($2::text[] IS NULL OR w.wi_status = ANY($2)) \
       AND ($3::bool IS NULL OR n.archived = $3) \
     ORDER BY w.wi_number LIMIT $4 OFFSET $5";

/// `OLD_WORKITEM_SELECT` plus the markers as correlated subqueries.
const CORRELATED_WORKITEM_SELECT: &str = "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, \
        (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
        EXISTS (SELECT 1 FROM relationship r \
                 WHERE r.left_id = w.node_id AND r.relationship = 'has_handoff') AS has_handoff, \
        (SELECT min(r.left_id) FROM relationship r \
           JOIN sprint_proposal sp ON sp.node_id = r.left_id \
          WHERE r.right_id = w.node_id AND r.relationship = 'covers' \
            AND sp.status::text IN ('proposed', 'active')) AS proposal_node_id, \
        n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id \
     WHERE ($1::text IS NULL OR pj.name = $1) \
       AND ($2::bool IS NULL OR n.archived = $2) \
     ORDER BY w.wi_number LIMIT $3 OFFSET $4";

/// `OLD_WORKITEM_SELECT` plus the markers as pre-aggregated joins — the
/// shipped shape.
const JOINED_WORKITEM_SELECT: &str = "SELECT w.wi_number, w.node_id, \
        pj.name AS project, a.name AS area, \
        w.wi_type, w.wi_status, w.wi_tshirt, w.sprint, w.title, w.content, w.details, \
        n.category, n.tags, pw.wi_number AS parent, n.archived, \
        (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
        (ho.left_id IS NOT NULL) AS has_handoff, cov.proposal_node_id, \
        n.created, n.updated \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN area a ON a.id = w.area_id \
     LEFT JOIN workitem pw ON pw.node_id = w.parent_node_id \
     LEFT JOIN (SELECT r.right_id AS node_id, min(r.left_id) AS proposal_node_id \
                  FROM relationship r \
                  JOIN sprint_proposal sp ON sp.node_id = r.left_id \
                 WHERE r.relationship = 'covers' \
                   AND sp.status::text IN ('proposed', 'active') \
                 GROUP BY r.right_id) cov ON cov.node_id = w.node_id \
     LEFT JOIN (SELECT DISTINCT r.left_id FROM relationship r \
                 WHERE r.relationship = 'has_handoff') ho ON ho.left_id = w.node_id \
     WHERE ($1::text IS NULL OR pj.name = $1) \
       AND ($2::bool IS NULL OR n.archived = $2) \
     ORDER BY w.wi_number LIMIT $3 OFFSET $4";

/// The correlated-subquery shape both WIs sketched, as a third variant so the
/// choice between it and the join shape is measured rather than argued. Same
/// answers as the shipped SQL — `only_live_proposals_mark_an_item_as_spoken_for`
/// and friends pass against either.
const CORRELATED_LEAN_SELECT: &str =
    "SELECT w.wi_number, w.node_id, pj.name AS project, w.title, \
            w.wi_type, w.wi_status, w.wi_tshirt, \
            (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
            (w.details IS NOT NULL AND w.details <> '') AS has_details, \
            EXISTS (SELECT 1 FROM relationship r \
                     WHERE r.left_id = w.node_id AND r.relationship = 'has_handoff') AS has_handoff, \
            (SELECT min(r.left_id) FROM relationship r \
               JOIN sprint_proposal sp ON sp.node_id = r.left_id \
              WHERE r.right_id = w.node_id AND r.relationship = 'covers' \
                AND sp.status::text IN ('proposed', 'active')) AS proposal_node_id \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     WHERE ($1::text IS NULL OR pj.name = $1) \
       AND ($2::text[] IS NULL OR w.wi_status = ANY($2)) \
       AND ($3::bool IS NULL OR n.archived = $3) \
     ORDER BY w.wi_number LIMIT $4 OFFSET $5";

/// The shipped join shape, spelled out here so the three-way comparison runs
/// the same statement text the repo does without reaching into its macros.
const JOINED_LEAN_SELECT: &str = "SELECT w.wi_number, w.node_id, pj.name AS project, w.title, \
            w.wi_type, w.wi_status, w.wi_tshirt, \
            (SELECT count(*) FROM comment c WHERE c.node_id = w.node_id) AS comment_count, \
            (w.details IS NOT NULL AND w.details <> '') AS has_details, \
            (ho.left_id IS NOT NULL) AS has_handoff, cov.proposal_node_id \
     FROM workitem w \
     JOIN node n ON n.id = w.node_id \
     LEFT JOIN project pj ON pj.id = n.project_id \
     LEFT JOIN (SELECT r.right_id AS node_id, min(r.left_id) AS proposal_node_id \
                  FROM relationship r \
                  JOIN sprint_proposal sp ON sp.node_id = r.left_id \
                 WHERE r.relationship = 'covers' \
                   AND sp.status::text IN ('proposed', 'active') \
                 GROUP BY r.right_id) cov ON cov.node_id = w.node_id \
     LEFT JOIN (SELECT DISTINCT r.left_id FROM relationship r \
                 WHERE r.relationship = 'has_handoff') ho ON ho.left_id = w.node_id \
     WHERE ($1::text IS NULL OR pj.name = $1) \
       AND ($2::text[] IS NULL OR w.wi_status = ANY($2)) \
       AND ($3::bool IS NULL OR n.archived = $3) \
     ORDER BY w.wi_number LIMIT $4 OFFSET $5";

/// Median of `runs` timings, in microseconds. Median rather than mean: the
/// first run pays for a cold cache and would otherwise set the number.
async fn time_us<F, Fut>(runs: usize, mut f: F) -> u128
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        f().await;
        samples.push(t.elapsed().as_micros());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

async fn seed(pool: &PgPool) {
    let names: Vec<String> = (0..PROJECTS).map(|i| format!("proj-{i:02}")).collect();
    for n in &names {
        create_project(pool, n).await.expect("project");
    }

    let mut wi_numbers = Vec::with_capacity(WORK_ITEMS);
    for i in 0..WORK_ITEMS {
        // 78% closed, matching the #861 corpus measurement — the lean read's
        // default filter has to walk past them, so they belong in the fixture.
        let status = if i % 100 < 78 { "closed" } else { "open" };
        let created = create_work_item(
            pool,
            NewWorkItem {
                project: Some(names[i % PROJECTS].clone()),
                project_id: None,
                area_id: None,
                area: None,
                wi_type: "task".into(),
                wi_status: status.into(),
                wi_tshirt: "Unknown".into(),
                sprint: None,
                title: format!("seeded work item {i}"),
                content: "x".repeat(400),
                details: if i % 3 == 0 {
                    Some("y".repeat(600))
                } else {
                    None
                },
                category: None,
                tags: vec![],
            },
        )
        .await
        .expect("work item");
        wi_numbers.push(created.wi_number);
    }

    for i in 0..PROPOSALS {
        let covers: Vec<i64> = (0..COVERS_PER_PROPOSAL)
            .map(|k| wi_numbers[(i * COVERS_PER_PROPOSAL + k) % WORK_ITEMS])
            .collect();
        create_proposal(
            pool,
            NewProposal {
                project: Some(names[i % PROJECTS].clone()),
                project_id: None,
                category: None,
                tags: vec![],
                title: format!("seeded proposal {i}"),
                summary: "s".into(),
                notes: None,
                rank: Decimal::new(i as i64, 0),
                pinned: false,
                covers,
            },
        )
        .await
        .expect("proposal");
    }

    for i in 0..HANDOFFS {
        let node = node_id_for_wi(pool, wi_numbers[i * 7 % WORK_ITEMS])
            .await
            .expect("lookup")
            .expect("node");
        create_handoff(
            pool,
            NewHandoff {
                project_id: None,
                project: None,
                category: None,
                tags: vec![],
                title: format!("seeded handoff {i}"),
                summary: "s".into(),
                body: "b".into(),
                related_node_ids: vec![node],
                allow_standalone: false,
            },
        )
        .await
        .expect("handoff");
    }
}

#[tokio::test]
#[ignore = "seeds a production-scale corpus and times queries; run deliberately"]
async fn membership_markers_cost_on_the_hottest_reads() {
    let (_c, pool) = fresh_korg().await;
    let t = Instant::now();
    seed(&pool).await;
    println!(
        "\nseeded {WORK_ITEMS} work items / {PROPOSALS} proposals / {HANDOFFS} handoffs \
         across {PROJECTS} projects in {:.1}s\n",
        t.elapsed().as_secs_f64()
    );

    const RUNS: usize = 15;
    // 500 is the REST read's cap and what the Work Items page walks to
    // completion — the worst realistic page, not a friendly one.
    const PAGE: i64 = 500;

    // Statement-level, not function-level.
    //
    // The first cut of this harness compared `OLD_WORKITEM_SELECT` against
    // `list_work_items()` and reported a +960% "regression" on the lean read.
    // That number was an artefact: the repo functions also run `wi_counts` (a
    // second statement for `total` + `omitted`), so it was timing two queries
    // against one. Every comparison below runs raw SQL against raw SQL on the
    // same rows in the same session — the only way the delta means what it
    // says. The function timings are reported separately, as absolutes.
    async fn full_variant(pool: &PgPool, sql: &str, runs: usize, page: i64) -> u128 {
        time_us(runs, || async {
            sqlx::query(sql)
                .bind(Option::<String>::None)
                .bind(archived_default())
                .bind(page)
                .bind(0i64)
                .fetch_all(pool)
                .await
                .expect("full read");
        })
        .await
    }
    async fn lean_variant(pool: &PgPool, sql: &str, runs: usize, page: i64) -> u128 {
        time_us(runs, || async {
            sqlx::query(sql)
                .bind(Option::<String>::None)
                .bind(Some(vec![
                    "open".to_string(),
                    "resolved".to_string(),
                    "done".to_string(),
                ]))
                .bind(archived_default())
                .bind(page)
                .bind(0i64)
                .fetch_all(pool)
                .await
                .expect("lean read");
        })
        .await
    }
    let old_full = full_variant(&pool, OLD_WORKITEM_SELECT, RUNS, PAGE).await;
    let correlated_full = full_variant(&pool, CORRELATED_WORKITEM_SELECT, RUNS, PAGE).await;
    let joined_full = full_variant(&pool, JOINED_WORKITEM_SELECT, RUNS, PAGE).await;

    let old_lean = lean_variant(&pool, OLD_LEAN_SELECT, RUNS, PAGE).await;
    let correlated_lean = lean_variant(&pool, CORRELATED_LEAN_SELECT, RUNS, PAGE).await;
    let joined_lean = lean_variant(&pool, JOINED_LEAN_SELECT, RUNS, PAGE).await;

    // Absolutes for the shipped call paths, statement timings aside — this is
    // what a caller actually waits for, count query included.
    let fn_full = time_us(RUNS, || async {
        list_work_items(
            &pool,
            WorkItemQuery {
                page: korg_core::repo::PageQuery {
                    limit: Some(PAGE),
                    offset: Some(0),
                },
                ..Default::default()
            },
        )
        .await
        .expect("list_work_items");
    })
    .await;
    let fn_lean = time_us(RUNS, || async {
        list_work_items_lean(&pool, None, None, archived_default(), PAGE, 0)
            .await
            .expect("list_work_items_lean");
    })
    .await;
    let rollup = time_us(RUNS, || async {
        planning_rollup(&pool).await.expect("rollup");
    })
    .await;

    let pct = |old: u128, new: u128| (new as f64 - old as f64) / old as f64 * 100.0;
    let row = |what: &str, base: u128, corr: u128, join: u128| {
        println!(
            "{what:<26} {base:>7}µs {corr:>7}µs {:>+7.1}% {join:>7}µs {:>+7.1}%",
            pct(base, corr),
            pct(base, join)
        );
    };
    println!("statement timings, {PAGE}-row page, median of {RUNS}:\n");
    println!(
        "{:<26} {:>9} {:>9} {:>8} {:>9} {:>8}",
        "read", "baseline", "correl.", "delta", "joined", "delta"
    );
    row(
        "list_work_items (REST)",
        old_full,
        correlated_full,
        joined_full,
    );
    row(
        "list_work_items (lean)",
        old_lean,
        correlated_lean,
        joined_lean,
    );

    // What a caller actually waits for. Both repo functions run a second
    // statement for `total`/`omitted`, which this sprint did not touch — so
    // the honest user-visible delta is the statement delta spread over a
    // total that already included that count. Subtracting it out gives the
    // implied before, which is the number the sprint record should quote
    // rather than the raw statement percentage.
    println!("\ncall-path absolutes (each includes an untouched count statement):");
    println!("  list_work_items          {fn_full:>7}µs");
    println!("  list_work_items_lean     {fn_lean:>7}µs");
    println!("  planning_rollup          {rollup:>7}µs");
    let count_full = fn_full.saturating_sub(joined_full);
    let count_lean = fn_lean.saturating_sub(joined_lean);
    let implied_full = old_full + count_full;
    let implied_lean = old_lean + count_lean;
    println!("\nimplied call-path delta (count statement held constant):");
    println!(
        "  list_work_items          {implied_full:>7}µs -> {fn_full:>7}µs  {:+.1}%",
        pct(implied_full, fn_full)
    );
    println!(
        "  list_work_items_lean     {implied_lean:>7}µs -> {fn_lean:>7}µs  {:+.1}%",
        pct(implied_lean, fn_lean)
    );

    // Not a perf assertion with a threshold — those go red on a loaded CI box
    // and teach everyone to ignore them. The guard is only that nothing has
    // gone catastrophically wrong; the real output is the table above, which
    // is what the sprint record quotes.
    assert!(
        joined_full < old_full * 10 + 50_000,
        "full read regressed by an order of magnitude: {old_full}µs -> {joined_full}µs"
    );
    assert!(
        joined_lean < old_lean * 10 + 50_000,
        "lean read regressed by an order of magnitude: {old_lean}µs -> {joined_lean}µs"
    );
}
