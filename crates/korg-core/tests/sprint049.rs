//! Sprint 049 — the UI-polish bundle's core-side halves (proposal korg:1042).
//!
//! Two of the sprint's five items reach past `web/`:
//!
//! * **#982** — `get_node_preview` had no `program` arm, so find-by-ID answered
//!   `979` with the `{kind} #{id}` fallback. The fence below was meant to be the
//!   part that mattered: *every* kind resolves, so the next kind added has a
//!   test waiting for it rather than a silent fallback.
//!
//!   It did not hold. The fence enumerated its own seven subjects, sprint 051
//!   added an eighth kind, and `schedule` went three sprints with no arm and no
//!   failing test. Sprint 054 rebuilt it on `vocab::NODE_KINDS`; the history is
//!   in `tests/sprint054.rs`, and it is worth reading before writing the next
//!   fence of this shape.
//! * **#966** — `get_work_item` is reached by `node_id` as well as `wi_number`.
//!   The two are the same number since the 0009 identity migration, so an agent
//!   holding either was holding the right one.

use korg_core::ops::WorkItemSelector;
use korg_core::repo::{
    create_card, create_handoff, create_link, create_program, create_project, create_proposal,
    create_schedule, create_work_item, get_node_preview, get_work_item_detail, upsert_report,
    NewHandoff, NewProgram,
};
use korg_core::vocab;
use korg_test_support::{fresh_korg, new, test_project, TEST_PROJECT};
use sqlx::PgPool;

async fn proposal_in(pool: &PgPool, project: &str, title: &str) -> i64 {
    create_proposal(pool, new::proposal_in(project, title))
        .await
        .unwrap()
        .row
        .node_id
}

// --- #982: find-by-ID resolves every kind -----------------------------------

/// The reported bug, exactly: a program previewed as `program #<id>` with no
/// title, no status and no span.
#[tokio::test]
async fn node_preview_renders_a_program() {
    let (_c, pool) = fresh_korg().await;
    for p in ["korg", "kfdc"] {
        create_project(&pool, p).await.unwrap();
    }
    let slices = vec![
        proposal_in(&pool, "korg", "korg slice").await,
        proposal_in(&pool, "kfdc", "kfdc slice").await,
    ];
    let created = create_program(
        &pool,
        NewProgram {
            slices,
            notes: Some("# Plan\nthe long form".into()),
            ..new::program("kfdc Phase 0 — the board substrate")
        },
    )
    .await
    .unwrap();

    let preview = get_node_preview(&pool, created.row.node_id)
        .await
        .unwrap()
        .expect("preview exists");

    assert_eq!(preview.kind, "program");
    assert_eq!(
        preview.title, "kfdc Phase 0 — the board substrate",
        "the real title, not the `program #<id>` fallback"
    );
    assert!(
        preview.badges.iter().any(|b| b == "active"),
        "status is a badge: {:?}",
        preview.badges
    );
    // D-6: a program has no project of its own, so the span is the honest
    // answer to "which repos does this touch".
    assert!(preview.project.is_none(), "a program carries no project");
    assert!(
        preview
            .fields
            .iter()
            .any(|f| f.label == "Span" && f.value == "kfdc, korg"),
        "the derived span is a field: {:?}",
        preview.fields
    );
    assert!(
        preview
            .fields
            .iter()
            .any(|f| f.label == "Slices" && f.value == "2"),
        "{:?}",
        preview.fields
    );
    // Same rule as a proposal: the long form is the body, the routing contract
    // stays a field — never both.
    assert_eq!(preview.body.as_deref(), Some("# Plan\nthe long form"));
    assert_eq!(preview.body_label.as_deref(), Some("Notes"));
    assert!(preview.fields.iter().any(|f| f.label == "Aim"));
}

/// With no notes the aim is the body, so the panel is never blank below the
/// header — the mirror of the proposal arm's summary fallback.
#[tokio::test]
async fn a_program_without_notes_previews_its_aim_as_the_body() {
    let (_c, pool) = fresh_korg().await;
    let created = create_program(&pool, new::program("not started"))
        .await
        .unwrap();

    let preview = get_node_preview(&pool, created.row.node_id)
        .await
        .unwrap()
        .expect("preview");
    assert_eq!(preview.body.as_deref(), Some("not started aim"));
    assert_eq!(preview.body_label.as_deref(), Some("Aim"));
    assert!(
        !preview.fields.iter().any(|f| f.label == "Aim"),
        "the aim is the body OR a field, never duplicated"
    );
    assert!(
        !preview.fields.iter().any(|f| f.label == "Span"),
        "no slices, so no span line rather than an empty one"
    );
}

/// The fence #982 actually asked for: "which other kinds hit this fallback?".
///
/// **This test used to answer that question with a hardcoded list of seven ids,
/// and it was wrong within eleven days.** Sprint 051 added `schedule` (0025) and
/// no `get_node_preview` arm; the list above stayed at seven, so a schedule
/// previewed as `schedule #1042` with no fields for three sprints while this
/// test — and the doc comment claiming it covered "every kind the `node.kind`
/// check constraint admits" — reported green. A fence that enumerates its own
/// subjects fences nothing (WI #870's audit, sprint 054).
///
/// It now iterates [`vocab::NODE_KINDS`], which `node_kinds_match_the_check
/// _constraint` pins to the database. The chain is: a new kind in a migration
/// fails the schema test until it is in `NODE_KINDS`, which fails the `_ =>` arm
/// below until this test can build one, which fails the title assertion until
/// `get_node_preview` renders it. Three failures, no silent fallback.
#[tokio::test]
async fn every_node_kind_resolves_to_a_real_title() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    // One node of each kind, built through the real write paths so the detail
    // rows exist the way production's do. Driven by the vocabulary rather than
    // by this list, so the `_` arm is what a ninth kind hits first.
    let mut ids: Vec<(&str, i64)> = Vec::new();
    for kind in vocab::NODE_KINDS {
        let id = match kind {
            "workitem" => {
                create_work_item(&pool, new::work_item("a work item"))
                    .await
                    .unwrap()
                    .node_id
            }
            "sprint_proposal" => proposal_in(&pool, TEST_PROJECT, "a proposal").await,
            "program" => {
                let slice = proposal_in(&pool, TEST_PROJECT, "a slice").await;
                create_program(
                    &pool,
                    NewProgram {
                        slices: vec![slice],
                        ..new::program("a program")
                    },
                )
                .await
                .unwrap()
                .row
                .node_id
            }
            "handoff" => {
                let subject = create_work_item(&pool, new::work_item("a subject"))
                    .await
                    .unwrap()
                    .node_id;
                create_handoff(
                    &pool,
                    NewHandoff {
                        related_node_ids: vec![subject],
                        ..new::handoff("a handoff")
                    },
                )
                .await
                .unwrap()
                .handoff
                .node_id
            }
            "card" => {
                create_card(&pool, new::card("a card"))
                    .await
                    .unwrap()
                    .node_id
            }
            "link" => {
                create_link(&pool, new::link("https://example.invalid/a"))
                    .await
                    .unwrap()
                    .node_id
            }
            "report" => {
                let today = time::OffsetDateTime::now_utc().date();
                upsert_report(&pool, new::report("a-source", today))
                    .await
                    .unwrap()
                    .node_id
            }
            "schedule" => {
                create_schedule(&pool, new::schedule("a drill", "quarterly", None))
                    .await
                    .unwrap()
                    .node_id
            }
            "attachment" => {
                // Unowned on purpose: a preview must render an image nothing
                // has claimed yet, which is the state a paste lands in and the
                // one a reader is most likely to be looking at from find-by-ID.
                korg_core::repo::create_attachment(&pool, new::attachment("screenshot.png"))
                    .await
                    .unwrap()
                    .node_id
            }
            other => panic!(
                "`{other}` is in vocab::NODE_KINDS but this fence cannot build one. \
                 Add a write path here, then make sure get_node_preview renders it — \
                 that second step is the one sprint 051 skipped for `schedule`."
            ),
        };
        ids.push((kind, id));
    }

    for (_kind, id) in ids {
        let preview = get_node_preview(&pool, id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("no preview for node {id}"));
        assert_ne!(
            preview.title,
            format!("{} #{id}", preview.kind),
            "kind `{}` still falls through to the generic title — it needs an \
             arm in get_node_preview (WI #982)",
            preview.kind
        );
        assert!(
            !preview.title.trim().is_empty(),
            "kind `{}` previews with a blank title",
            preview.kind
        );
    }
}

// --- #966: get_work_item by node_id or wi_number -----------------------------

/// Fable's call, which was right in every way that matters: for a work item the
/// node id and the wi_number are the same number, so `{node_id: 965}` named the
/// item it meant. It used to be `missing field wi_number`.
#[tokio::test]
async fn get_work_item_accepts_either_id() {
    let (_c, pool) = fresh_korg().await;
    let item = create_work_item(&pool, new::work_item("reachable by either id"))
        .await
        .unwrap();
    assert_eq!(
        item.node_id, item.wi_number,
        "the 0009 identity migration is the premise of this whole item"
    );

    let by_number = WorkItemSelector {
        wi_number: Some(item.wi_number),
        node_id: None,
    }
    .resolve()
    .unwrap();
    let by_node = WorkItemSelector {
        wi_number: None,
        node_id: Some(item.node_id),
    }
    .resolve()
    .unwrap();
    assert_eq!(by_number, by_node);

    let detail = get_work_item_detail(&pool, by_node)
        .await
        .unwrap()
        .expect("found by node_id");
    assert_eq!(detail.item.title, "reachable by either id");
}

/// Neither half is `invalid_input` naming both spellings — not a panic, and not
/// a silent default to some id.
#[tokio::test]
async fn get_work_item_requires_one_of_the_two() {
    let err = WorkItemSelector {
        wi_number: None,
        node_id: None,
    }
    .resolve()
    .unwrap_err()
    .to_string();
    assert!(err.contains("wi_number"), "{err}");
    assert!(err.contains("node_id"), "{err}");
}

/// Both halves is a refusal, matching the project/area selectors (#575): korg
/// does not guess which of two explicitly-passed ids the caller meant, even
/// when they agree.
#[tokio::test]
async fn get_work_item_refuses_both_ids() {
    let err = WorkItemSelector {
        wi_number: Some(965),
        node_id: Some(965),
    }
    .resolve()
    .unwrap_err()
    .to_string();
    assert!(err.contains("not both"), "{err}");
}
