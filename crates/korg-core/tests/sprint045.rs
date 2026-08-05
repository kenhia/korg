//! Sprint 045 (#970, korg:973) — the board rollup read.
//!
//! One call returning what a dashboard renders, so kfdc and korg-dash stop
//! crawling. The 2026-07-31 backlog review assembled a fraction of this with 17
//! `get_proposal` calls plus a script.
//!
//! What this suite pins, and why each is a rule rather than a habit:
//!
//! 1. **One aggregate, two panels** (D-2, the #976 answer). Fire Missions'
//!    progress and Operations' per-slice rollups are the same counts one level
//!    apart, so a board slice and `get_program`'s slice must be *byte-identical*
//!    — the moment they are computed twice they can disagree.
//! 2. **A slice is not a queue row.** A program includes proposals whatever
//!    their state; the board fetches them for Operations and must not let a
//!    `done` or archived one leak into `active`/`queue`.
//! 3. **No counters block** (D-3). Every header figure is derivable from the
//!    lists the board already returns, and each derivation must agree with the
//!    corpus read that owns it.
//! 4. **`summary` yes, `notes` no** (D-5), and the board's programs cannot
//!    disagree with `list_programs`.

use korg_core::repo::{
    archived_default, board_rollup, create_program, create_project, create_proposal,
    create_work_item, get_program_detail, list_awaiting, list_programs, list_proposals_lean,
    node_id_for_wi, planning_rollup, relate, set_awaiting, update_proposal, update_work_item,
    upsert_report, NewProgram, NewWorkItem, ProposalPatch, WorkItemPatch, BOARD_REPORT_CAP,
};
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use rust_decimal::Decimal;
use sqlx::PgPool;
use time::{Date, Month};

/// A proposal in `project` covering `wi_numbers`, returning its node_id.
async fn proposal_covering(
    pool: &PgPool,
    project: &str,
    title: &str,
    summary: &str,
    rank: i64,
    covers: Vec<i64>,
) -> i64 {
    create_proposal(
        pool,
        korg_core::repo::NewProposal {
            summary: summary.into(),
            rank: Decimal::from(rank),
            covers,
            ..new::proposal_in(project, title)
        },
    )
    .await
    .unwrap()
    .row
    .node_id
}

/// A work item in `project` with a given status, returning its wi_number.
async fn wi_in(pool: &PgPool, project: &str, title: &str, status: &str) -> i64 {
    create_work_item(
        pool,
        NewWorkItem {
            project: Some(project.into()),
            wi_status: status.into(),
            ..new::work_item(title)
        },
    )
    .await
    .unwrap()
    .wi_number
}

async fn set_status(pool: &PgPool, node_id: i64, status: &str) {
    update_proposal(
        pool,
        node_id,
        ProposalPatch {
            status: Some(status.into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

// --- 1. one aggregate, two panels (D-2) -------------------------------------

/// The progress track Fire Missions renders: the four status counts, summing to
/// `covered_count`. `WI_STATUSES` is exactly these four, so a sum that misses is
/// a projection bug, not a rounding one.
#[tokio::test]
async fn a_proposals_progress_counts_its_covered_items_by_status() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let items = vec![
        wi_in(&pool, TEST_PROJECT, "still open", "open").await,
        wi_in(&pool, TEST_PROJECT, "also open", "open").await,
        wi_in(&pool, TEST_PROJECT, "implemented", "resolved").await,
        wi_in(&pool, TEST_PROJECT, "agent satisfied", "done").await,
        wi_in(&pool, TEST_PROJECT, "ken closed it", "closed").await,
    ];
    let id = proposal_covering(&pool, TEST_PROJECT, "firing", "the mission", 1, items).await;
    set_status(&pool, id, "active").await;

    let board = board_rollup(&pool).await.unwrap();
    let mission = &board.active[0];
    assert_eq!(mission.covered_count, 5);
    assert_eq!((mission.open, mission.resolved), (2, 1));
    assert_eq!((mission.done, mission.closed), (1, 1));
    assert_eq!(
        mission.open + mission.resolved + mission.done + mission.closed,
        mission.covered_count,
        "the four counts partition the covered set — WI_STATUSES is exactly these"
    );
}

/// The decision #976 asked for, made executable: the board's slice rollup and
/// `get_program`'s must be the **same value**, because they are the same
/// aggregate one level apart. Compute them twice and they can drift; this fails
/// the moment someone does.
#[tokio::test]
async fn a_board_slice_is_identical_to_the_one_get_program_returns() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    create_project(&pool, "elsewhere").await.unwrap();

    let here = proposal_covering(
        &pool,
        TEST_PROJECT,
        "engine slice",
        "s",
        1,
        vec![
            wi_in(&pool, TEST_PROJECT, "open one", "open").await,
            wi_in(&pool, TEST_PROJECT, "done one", "done").await,
        ],
    )
    .await;
    let there = proposal_covering(
        &pool,
        "elsewhere",
        "ui slice",
        "s",
        2,
        vec![wi_in(&pool, "elsewhere", "resolved one", "resolved").await],
    )
    .await;

    let program = create_program(
        &pool,
        NewProgram {
            slices: vec![there, here],
            ..new::program("spans two repos")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;

    let board = board_rollup(&pool).await.unwrap();
    let detail = get_program_detail(&pool, program).await.unwrap().unwrap();
    assert_eq!(board.programs.len(), 1);
    assert_eq!(
        board.programs[0].slices, detail.slices,
        "one aggregate, two panels — a board slice and a program-detail slice \
         are the same rollup and must not be computed twice"
    );
    assert_eq!(
        board.programs[0].slices[0].title, "ui slice",
        "the caller's slice order, from the edge rank — not node_id order"
    );
}

// --- 2. a slice is not a queue row ------------------------------------------

/// The Operations panel renders finished programs (the concept's
/// `OP INFRA-CLEANUP` has nothing but done slices), so the board fetches a
/// program's proposals whatever their state — and must not let that fetch put a
/// `done` proposal back on the queue it left.
#[tokio::test]
async fn a_done_slice_is_rolled_up_but_never_enters_the_queue() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let shipped = proposal_covering(
        &pool,
        TEST_PROJECT,
        "already shipped",
        "s",
        1,
        vec![wi_in(&pool, TEST_PROJECT, "closed out", "closed").await],
    )
    .await;
    set_status(&pool, shipped, "done").await;
    create_program(
        &pool,
        NewProgram {
            slices: vec![shipped],
            ..new::program("a finished operation")
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(
        board.programs[0].slices.len(),
        1,
        "rolled up for Operations"
    );
    assert_eq!(board.programs[0].slices[0].closed, 1);
    assert!(board.active.is_empty() && board.queue.is_empty());
    assert_eq!(
        board.proposals_omitted.done, 1,
        "hidden from the queue, and counted as hidden"
    );
}

/// The same guard one door over: an archived proposal a live program still
/// includes is rolled up, and stays out of the queue on its way past.
#[tokio::test]
async fn an_archived_slice_stays_out_of_the_queue() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let parked = proposal_covering(&pool, TEST_PROJECT, "parked", "s", 1, vec![]).await;
    update_proposal(
        &pool,
        parked,
        ProposalPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    create_program(
        &pool,
        NewProgram {
            slices: vec![parked],
            ..new::program("still running")
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.programs[0].slices.len(), 1);
    assert!(
        board.queue.is_empty(),
        "it is `proposed`, but archived — the queue is live rows only"
    );
    assert_eq!(board.proposals_omitted.archived, 1);
}

// --- 3. no counters block: every header figure is derivable (D-3) -----------

/// The board deliberately ships no `counts` object. This asserts the derivations
/// the header statline uses, each against the corpus read that owns that number
/// — which is the property a counters block would put at risk.
#[tokio::test]
async fn every_header_figure_is_derivable_and_agrees_with_its_own_read() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    create_project(&pool, "elsewhere").await.unwrap();

    let firing = proposal_covering(&pool, TEST_PROJECT, "firing", "s", 1, vec![]).await;
    set_status(&pool, firing, "active").await;
    proposal_covering(&pool, TEST_PROJECT, "next up", "s", 2, vec![]).await;
    proposal_covering(&pool, "elsewhere", "also queued", "s", 3, vec![]).await;
    let shipped = proposal_covering(&pool, TEST_PROJECT, "shipped", "s", 4, vec![]).await;
    set_status(&pool, shipped, "done").await;

    let wi = wi_in(&pool, TEST_PROJECT, "your call", "resolved").await;
    let node = node_id_for_wi(&pool, wi).await.unwrap().unwrap();
    set_awaiting(&pool, node, true, Some("proceed or kill?"))
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let queue = list_proposals_lean(&pool, None, None, archived_default())
        .await
        .unwrap();

    // live proposals = active + queue
    assert_eq!(board.active.len() + board.queue.len(), queue.items.len());
    assert_eq!(board.active.len(), 1);
    // shipped = proposals_omitted.done
    assert_eq!(board.proposals_omitted.done, queue.omitted.done);
    assert_eq!(board.proposals_omitted.done, 1);
    // awaiting Ken = awaiting.len()
    assert_eq!(
        board.awaiting.len(),
        list_awaiting(&pool).await.unwrap().len()
    );
    assert_eq!(
        board.awaiting[0].awaiting_note.as_deref(),
        Some("proceed or kill?")
    );
    // projects = depth filtered by status — the figure D-3 made derivable by
    // putting `status` on the row instead of adding a counter for it.
    let active_projects = board.depth.iter().filter(|d| d.status == "active").count();
    assert_eq!(active_projects, 2, "korg + elsewhere");
    assert_eq!(board.depth, planning_rollup(&pool).await.unwrap());
}

/// `depth` is `planning_rollup` verbatim, which means *every* project — a rail
/// entry that vanishes at zero is a rail you cannot click — now carrying the
/// status that tells the board which of them are live.
#[tokio::test]
async fn depth_carries_every_project_with_its_status() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    create_project(&pool, "quiet").await.unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let quiet = board.depth.iter().find(|d| d.project == "quiet").unwrap();
    assert_eq!(
        (quiet.proposals, quiet.wi_in_proposal, quiet.wi_total),
        (0, 0, 0),
        "present with three zeroes, not absent"
    );
    assert_eq!(quiet.status, "active");
}

// --- 4. the row contract, and what cannot disagree --------------------------

/// D-5: the summary is the mission's subtitle and is on the row; the analysis is
/// unbounded and is not. The board is a dashboard refresh, not an archive read.
#[tokio::test]
async fn the_board_carries_the_summary_and_leaves_notes_behind() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let id = proposal_covering(
        &pool,
        TEST_PROJECT,
        "firing",
        "the routing contract, under 500 chars",
        1,
        vec![],
    )
    .await;
    update_proposal(
        &pool,
        id,
        ProposalPatch {
            status: Some("active".into()),
            notes: Some(Some("the unbounded analysis".repeat(200))),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let json = serde_json::to_value(&board).unwrap();
    assert_eq!(
        board.active[0].summary,
        "the routing contract, under 500 chars"
    );
    assert!(
        !json.to_string().contains("the unbounded analysis"),
        "`notes` must stay behind get_proposal — it is unbounded"
    );
}

/// The board's Operations panel and `/api/programs` are the same rows by
/// construction: the slice pass is keyed off the program ids the list returned.
#[tokio::test]
async fn the_boards_programs_cannot_disagree_with_list_programs() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let slice = proposal_covering(&pool, TEST_PROJECT, "a slice", "s", 1, vec![]).await;
    create_program(
        &pool,
        NewProgram {
            slices: vec![slice],
            ..new::program("in motion")
        },
    )
    .await
    .unwrap();
    let finished = create_program(&pool, new::program("finished"))
        .await
        .unwrap()
        .row
        .node_id;
    korg_core::repo::update_program(
        &pool,
        finished,
        korg_core::repo::ProgramPatch {
            status: Some("done".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    let list = list_programs(&pool, None, archived_default())
        .await
        .unwrap();
    assert_eq!(
        board
            .programs
            .iter()
            .map(|p| p.program.node_id)
            .collect::<Vec<_>>(),
        list.items.iter().map(|p| p.node_id).collect::<Vec<_>>(),
    );
    assert_eq!(board.programs_omitted.done, 1);
    assert_eq!(board.programs_omitted.done, list.omitted.done);
}

/// `active` and `queue` are split by status and each carries the queue's own
/// stable order — pinned first, then rank, then node_id (F-19).
#[tokio::test]
async fn active_and_queue_are_split_by_status_and_ordered_pinned_then_rank() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let firing = proposal_covering(&pool, TEST_PROJECT, "firing", "s", 9, vec![]).await;
    set_status(&pool, firing, "active").await;
    proposal_covering(&pool, TEST_PROJECT, "rank three", "s", 3, vec![]).await;
    proposal_covering(&pool, TEST_PROJECT, "rank one", "s", 1, vec![]).await;
    let pinned = proposal_covering(&pool, TEST_PROJECT, "pinned, rank five", "s", 5, vec![]).await;
    update_proposal(
        &pool,
        pinned,
        ProposalPatch {
            pinned: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(
        board.active.iter().map(|p| &*p.title).collect::<Vec<_>>(),
        ["firing"],
        "a rank-9 active proposal is a Fire Mission, not the bottom of On Deck"
    );
    assert_eq!(
        board.queue.iter().map(|p| &*p.title).collect::<Vec<_>>(),
        ["pinned, rank five", "rank one", "rank three"],
    );
}

/// Sensor Net's source, capped. `report_date` is the only date in korg that
/// records when something *happened*, which is why it is the whole of the
/// board's event story (D-7).
#[tokio::test]
async fn reports_are_the_newest_few_newest_first() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    for day in 1..=(BOARD_REPORT_CAP as u8 + 2) {
        upsert_report(
            &pool,
            new::report(
                "kmon",
                Date::from_calendar_date(2026, Month::August, day).unwrap(),
            ),
        )
        .await
        .unwrap();
    }

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.reports.len(), BOARD_REPORT_CAP as usize);
    assert_eq!(
        board.reports[0].report_date,
        Date::from_calendar_date(2026, Month::August, BOARD_REPORT_CAP as u8 + 2).unwrap(),
        "newest first"
    );
}

/// `generated` is Postgres's clock, not the process's — the same one every
/// timestamp on the board came from, so an age computed against it is right.
/// A row written before the read must not be stamped after it.
#[tokio::test]
async fn generated_is_the_same_clock_the_rows_came_from() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    let wi = wi_in(&pool, TEST_PROJECT, "your call", "open").await;
    let node = node_id_for_wi(&pool, wi).await.unwrap().unwrap();
    let marked = set_awaiting(&pool, node, true, Some("decide"))
        .await
        .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert!(
        board.generated >= marked.awaiting_since.unwrap(),
        "the board cannot be assembled before the ask it is showing"
    );
}

/// The whole point, end to end: a corpus that would have taken a crawl, read
/// once, with every panel populated.
#[tokio::test]
async fn one_call_fills_every_panel() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    create_project(&pool, "elsewhere").await.unwrap();

    let firing = proposal_covering(
        &pool,
        TEST_PROJECT,
        "firing",
        "s",
        1,
        vec![wi_in(&pool, TEST_PROJECT, "in flight", "open").await],
    )
    .await;
    set_status(&pool, firing, "active").await;
    let queued = proposal_covering(&pool, "elsewhere", "on deck", "s", 2, vec![]).await;
    create_program(
        &pool,
        NewProgram {
            slices: vec![firing, queued],
            ..new::program("operation")
        },
    )
    .await
    .unwrap();

    let blocked = wi_in(&pool, TEST_PROJECT, "your ops action", "open").await;
    let node = node_id_for_wi(&pool, blocked).await.unwrap().unwrap();
    set_awaiting(&pool, node, true, Some("rotate the password"))
        .await
        .unwrap();
    upsert_report(
        &pool,
        new::report(
            "kmon",
            Date::from_calendar_date(2026, Month::August, 5).unwrap(),
        ),
    )
    .await
    .unwrap();

    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(board.active.len(), 1, "Fire Missions");
    assert_eq!(board.queue.len(), 1, "On Deck");
    assert_eq!(board.programs.len(), 1, "Operations");
    assert_eq!(board.programs[0].slices.len(), 2);
    assert_eq!(board.programs[0].program.span, ["elsewhere", TEST_PROJECT]);
    assert_eq!(board.awaiting.len(), 1, "Commander's Call");
    assert_eq!(board.reports.len(), 1, "Sensor Net");
    assert_eq!(board.depth.len(), 2, "queue depth, one row per project");
}

/// The board is *fed* by the same edges everything else reads: a `covers` edge
/// written by `relate` after the fact shows up in the next read's progress,
/// because the rollup is computed, never cached.
#[tokio::test]
async fn progress_follows_the_edges_and_is_never_cached() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    let id = proposal_covering(&pool, TEST_PROJECT, "firing", "s", 1, vec![]).await;
    set_status(&pool, id, "active").await;
    assert_eq!(
        board_rollup(&pool).await.unwrap().active[0].covered_count,
        0
    );

    let wi = wi_in(&pool, TEST_PROJECT, "added later", "open").await;
    let node = node_id_for_wi(&pool, wi).await.unwrap().unwrap();
    relate(&pool, id, node, "covers", None, None).await.unwrap();
    let board = board_rollup(&pool).await.unwrap();
    assert_eq!(
        (board.active[0].covered_count, board.active[0].open),
        (1, 1)
    );

    update_work_item(
        &pool,
        wi,
        WorkItemPatch {
            wi_status: Some("resolved".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let board = board_rollup(&pool).await.unwrap();
    assert_eq!((board.active[0].open, board.active[0].resolved), (0, 1));
}
