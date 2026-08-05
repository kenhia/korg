//! Sprint 036 — the collection read contract.
//!
//! Two halves, both of them contracts rather than features:
//!
//! - **#860** — `sprint_proposal.summary` is a ≤500-character routing contract
//!   and `notes` carries the analysis. Migration 0021 moved 96 of 119 production
//!   summaries; what is tested here is that the move **preserves every
//!   character**, which is the one outcome the WI forbids losing.
//! - **#861** — `list_work_items_lean` excludes terminal and archived rows by
//!   default and reports both in `omitted`, as a cascade so nothing is
//!   double-counted.

use korg_core::repo::{
    create_proposal, create_work_item, get_proposal, list_work_items_lean, update_proposal,
    update_work_item, NewProposal, NewWorkItem, ProposalPatch, WorkItemPatch, PROPOSAL_SUMMARY_MAX,
};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use rust_decimal::Decimal;
use sqlx::PgPool;

fn proposal(title: &str, summary: &str) -> NewProposal {
    NewProposal {
        project_id: None,
        project: Some(TEST_PROJECT.into()),
        category: None,
        tags: vec![],
        title: title.into(),
        summary: summary.into(),
        notes: None,
        rank: Decimal::ZERO,
        pinned: false,
        covers: vec![],
    }
}

// ---------------------------------------------------------------------------
// #860 — migration 0021
// ---------------------------------------------------------------------------

/// Seed a proposal past the constraint, the way production data got there:
/// straight into the tables, before 0021 ran.
///
/// It still carries a project — 0022 made that a CHECK on `node`, and the
/// production rows this stands in for all had one by then (0016 §5).
async fn seed_raw(pool: &PgPool, title: &str, summary: &str) -> i64 {
    let project: i64 = sqlx::query_scalar(
        "INSERT INTO project (name) VALUES ('seeded') ON CONFLICT (name) DO UPDATE \
         SET name = project.name RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let node_id: i64 = sqlx::query_scalar(
        "INSERT INTO node (kind, project_id) VALUES ('sprint_proposal', $1) RETURNING id",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sprint_proposal (node_id, title, summary, rank) VALUES ($1,$2,$3,0)")
        .bind(node_id)
        .bind(title)
        .bind(summary)
        .execute(pool)
        .await
        .unwrap();
    node_id
}

/// WI #860 — migration 0021, applied to a corpus shaped like production's.
///
/// The file is deliberately re-appliable (`IF NOT EXISTS` on the column, a
/// guard on the constraint, an UPDATE whose WHERE clause is false once it has
/// run), which is what lets this test seed an over-cap corpus on an
/// already-migrated database and run **the real file** against it rather than a
/// paraphrase of it — the same mechanism `sprint010::migration_0018_…` uses.
#[tokio::test]
async fn migration_0021_moves_the_analysis_and_keeps_every_character() {
    const SQL: &str = include_str!("../migrations/0021_proposal_notes_and_routing_summary.sql");

    let (_c, pool) = fresh_korg().await;

    // The constraint is already there (0021 ran with the rest); drop it so the
    // seed can hold the shapes production held before the migration.
    sqlx::raw_sql(
        "ALTER TABLE sprint_proposal DROP CONSTRAINT sprint_proposal_summary_routing_line",
    )
    .execute(&pool)
    .await
    .unwrap();

    // 1. A lede plus analysis — the shape every real proposal has.
    let lede = "Generalize 816's proven three-tier contract beyond projects.";
    let structured = format!("{lede}\n\n{}", "Analysis. ".repeat(80));
    let a = seed_raw(&pool, "structured", &structured).await;

    // 2. One paragraph, no blank line, well past the cap: the derivation has to
    //    cut it, and must cut at a word boundary.
    let flat = "word ".repeat(200);
    let b = seed_raw(&pool, "flat", &flat).await;

    // 3. Already inside the cap — must not be touched at all.
    let short = "Short enough to be a routing contract already.";
    let c = seed_raw(&pool, "short", short).await;

    sqlx::raw_sql(SQL).execute(&pool).await.unwrap();

    let row = |node_id: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT summary, notes FROM sprint_proposal WHERE node_id = $1",
            )
            .bind(node_id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };

    let (summary, notes) = row(a).await;
    assert_eq!(
        summary,
        format!("{lede} […]"),
        "a structured summary derives to its first paragraph plus the marker"
    );
    assert_eq!(
        notes.as_deref(),
        Some(structured.as_str()),
        "every character of the original is in `notes`, verbatim"
    );

    let (summary, notes) = row(b).await;
    assert!(
        summary.chars().count() <= 500,
        "derived summary is {} chars",
        summary.chars().count()
    );
    assert!(summary.ends_with(" […]"));
    let prefix = summary.strip_suffix(" […]").unwrap();
    assert!(
        flat.starts_with(prefix),
        "the derived summary must be a prefix of the original, not a paraphrase"
    );
    assert!(
        flat[prefix.len()..].starts_with(char::is_whitespace),
        "the cut lands on a word boundary, not mid-word: …{:?}",
        &flat[prefix.len().saturating_sub(12)..prefix.len() + 4]
    );
    assert_eq!(notes.as_deref(), Some(flat.as_str()));

    let (summary, notes) = row(c).await;
    assert_eq!(
        summary, short,
        "a summary already inside the cap is untouched"
    );
    assert_eq!(
        notes, None,
        "and gets no `notes` — the column means \"there is more\", not \"there was\""
    );

    // The constraint came back with the file, and it bites.
    let over = "x".repeat(501);
    let err = seed_raw_expecting_failure(&pool, &over).await;
    assert!(
        err.contains("sprint_proposal_summary_routing_line"),
        "the CHECK must be back in place after a re-apply; got {err}"
    );
}

async fn seed_raw_expecting_failure(pool: &PgPool, summary: &str) -> String {
    let project: i64 = sqlx::query_scalar(
        "INSERT INTO project (name) VALUES ('seeded') ON CONFLICT (name) DO UPDATE \
         SET name = project.name RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let node_id: i64 = sqlx::query_scalar(
        "INSERT INTO node (kind, project_id) VALUES ('sprint_proposal', $1) RETURNING id",
    )
    .bind(project)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO sprint_proposal (node_id, title, summary, rank) VALUES ($1,'x',$2,0)")
        .bind(node_id)
        .bind(summary)
        .execute(pool)
        .await
        .expect_err("the CHECK constraint must reject an over-cap summary")
        .to_string()
}

// ---------------------------------------------------------------------------
// #860 — the write surface
// ---------------------------------------------------------------------------

/// The cap is enforced in `korg-core` as well as by the constraint, so the
/// caller gets `invalid_input` naming the field and the overage rather than a
/// raw constraint violation surfacing as `internal` — the same treatment
/// `PROJECT_DESCRIPTION_MAX` got in #828.
#[tokio::test]
async fn an_over_long_summary_is_invalid_input_on_both_write_paths() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let over = "x".repeat(PROPOSAL_SUMMARY_MAX + 1);

    let err = create_proposal(&pool, proposal("too long", &over))
        .await
        .expect_err("propose_sprint must reject an over-cap summary")
        .to_string();
    assert!(
        err.contains("500") && err.contains("notes"),
        "the error has to name the cap and the remedy; got {err}"
    );

    let p = create_proposal(&pool, proposal("fine", "short"))
        .await
        .unwrap()
        .row;
    let err = update_proposal(
        &pool,
        p.node_id,
        ProposalPatch {
            summary: Some(over),
            ..Default::default()
        },
    )
    .await
    .expect_err("update_proposal must reject one too")
    .to_string();
    assert!(err.contains("500"), "got {err}");

    // Exactly at the cap is legal — an off-by-one here would be invisible.
    let exact = "y".repeat(PROPOSAL_SUMMARY_MAX);
    update_proposal(
        &pool,
        p.node_id,
        ProposalPatch {
            summary: Some(exact.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        get_proposal(&pool, p.node_id)
            .await
            .unwrap()
            .unwrap()
            .summary,
        exact
    );
}

/// `notes` is unbounded, survives a round trip, and clears on an explicit null
/// (the double-option spelling every other nullable patch field uses).
#[tokio::test]
async fn notes_round_trips_and_clears() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let analysis = "measured, at length. ".repeat(500);

    let created = create_proposal(
        &pool,
        NewProposal {
            notes: Some(analysis.clone()),
            ..proposal("with notes", "the contract")
        },
    )
    .await
    .unwrap()
    .row;
    assert_eq!(created.notes.as_deref(), Some(analysis.as_str()));

    // Absent leaves it alone: a status transition must not wipe the analysis.
    let after_status = update_proposal(
        &pool,
        created.node_id,
        ProposalPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(after_status.notes.as_deref(), Some(analysis.as_str()));

    let cleared = update_proposal(
        &pool,
        created.node_id,
        ProposalPatch {
            notes: Some(None),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(cleared.notes, None);
}

// ---------------------------------------------------------------------------
// #861 — the lean list
// ---------------------------------------------------------------------------

async fn wi(pool: &PgPool, title: &str, status: &str) -> i64 {
    let row = create_work_item(
        pool,
        NewWorkItem {
            content: "x".into(),
            wi_status: status.into(),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap();
    row.wi_number
}

/// WI #861 — the default hides terminal *and* archived rows, and `omitted`
/// accounts for both without counting anything twice.
///
/// The cascade is the subtle part: an archived closed item is hidden by the
/// archived filter first, so it belongs to `omitted.archived` and must NOT also
/// appear in `omitted.closed`. Getting that wrong makes the two counts sum to
/// more than the corpus, which is a different kind of lie from the one this
/// envelope was added to stop.
#[tokio::test]
async fn the_lean_list_excludes_terminal_and_archived_and_reports_both() {
    let (_c, pool) = fresh_korg().await;
    wi(&pool, "open one", "open").await;
    wi(&pool, "resolved one", "resolved").await;
    wi(&pool, "done one", "done").await;
    wi(&pool, "closed one", "closed").await;
    let archived_open = wi(&pool, "archived open", "open").await;
    let archived_closed = wi(&pool, "archived closed", "closed").await;
    for n in [archived_open, archived_closed] {
        update_work_item(
            &pool,
            n,
            WorkItemPatch {
                archived: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let list = |wi_status: Option<&'static str>, archived| {
        let pool = pool.clone();
        async move {
            list_work_items_lean(&pool, None, wi_status, archived, 200, 0)
                .await
                .unwrap()
        }
    };

    let default = list(None, Some(false)).await;
    assert_eq!(
        default.total, 3,
        "open + resolved + done, live only — not the closed one and neither archived row"
    );
    assert_eq!(default.omitted.closed, 1, "the live closed row");
    assert_eq!(
        default.omitted.archived, 2,
        "both archived rows, whatever their status"
    );
    assert!(
        default.items.iter().all(|i| i.wi_status != "closed"),
        "no closed row may appear under the default"
    );

    let all = list(Some("all"), Some(false)).await;
    assert_eq!(all.total, 4, "`all` adds the closed row back");
    assert_eq!(
        all.omitted.closed, 0,
        "a status the caller asked for is not omitted"
    );
    assert_eq!(all.omitted.archived, 2);

    let closed_only = list(Some("closed"), Some(false)).await;
    assert_eq!(closed_only.total, 1);
    assert_eq!(closed_only.omitted.closed, 0);

    let everything = list(Some("all"), None).await;
    assert_eq!(everything.total, 6, "every row, archived included");
    assert_eq!(everything.omitted.closed, 0);
    assert_eq!(
        everything.omitted.archived, 0,
        "a filter that hides nothing must report nothing hidden"
    );

    // The cascade: 3 shown + 1 closed + 2 archived = 6, the whole corpus. If an
    // archived closed row were counted on both sides this would be 7.
    assert_eq!(
        default.total + default.omitted.closed + default.omitted.archived,
        everything.total
    );
}

/// The projection is the whole point: these rows carry no `content`/`details`,
/// which is the 890k characters the old `list_work_items` shipped by default.
#[tokio::test]
async fn the_lean_list_validates_its_status_and_pages() {
    let (_c, pool) = fresh_korg().await;
    for i in 0..5 {
        wi(&pool, &format!("item {i}"), "open").await;
    }

    let page = list_work_items_lean(&pool, None, None, Some(false), 2, 0)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(
        page.total, 5,
        "total is the filtered count, before the page"
    );
    assert_eq!((page.limit, page.offset), (2, 0));

    let err = list_work_items_lean(&pool, None, Some("nonsense"), Some(false), 200, 0)
        .await
        .expect_err("an unknown status must be rejected, not silently ignored")
        .to_string();
    assert!(err.contains("work item status"), "got {err}");
}
