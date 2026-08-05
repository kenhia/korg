//! Sprint 038 — the T3 behaviour gaps, fenced.
//!
//! Every test here is a probe sprint 037 ran against production, turned into an
//! assertion. Four of them describe a bug that existed; the fifth (#886)
//! describes a bug that did *not*, and exists so the same false positive cannot
//! be filed a third time.

use korg_core::daily_plan::{self, LifecycleContext, PlanningError};
use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::repo::{
    archived_default, create_card, create_link, create_proposal, create_work_item,
    list_work_items_lean, mark_link_read, set_link_disposition, update_card, update_link,
    update_project_by_name, update_proposal, update_work_item, CardPatch, LinkPatch, NewCard,
    NewLink, NewProposal, NewWorkItem, ProjectPatch, ProposalPatch, WorkItemPatch,
};
use korg_test_support::{fresh_korg, new, test_project};
use sqlx::PgPool;
use time::macros::date;
use time::{Date, OffsetDateTime};

/// The error code a surface would report for a failure, which is the half of an
/// error an agent branches on.
fn code(e: &anyhow::Error) -> ErrorCode {
    e.code()
}

async fn make_project(pool: &PgPool, name: &str, status: &str) {
    sqlx::query("INSERT INTO project (name, status) VALUES ($1, $2)")
        .bind(name)
        .bind(status)
        .execute(pool)
        .await
        .expect("seed project");
}

// ---------------------------------------------------------------------------
// #883 — `total` on a page past the end
// ---------------------------------------------------------------------------

/// The probe: `list_work_items {project:"korg"}` → `total: 14`; the same query
/// at `offset: 100` → `total: 0`. The count rode on the returned rows
/// (`count(*) OVER()`), so an empty page carried no count at all and the
/// envelope fell back to zero — announcing an empty corpus at exactly the
/// moment a pager overshoots and is trusting `total` most.
///
/// Asserted across the whole boundary rather than at one offset: the last full
/// page, the first empty one, and far past the end all have to agree, because
/// the defect was invisible until the page went empty.
#[tokio::test]
async fn total_is_the_corpus_on_every_page_including_past_the_end() {
    let (_c, pool) = fresh_korg().await;
    for i in 0..5 {
        create_work_item(&pool, new::work_item(&format!("item {i}")))
            .await
            .expect("create wi");
    }

    for (limit, offset, expected_items) in [(2, 0, 2), (2, 4, 1), (2, 5, 0), (2, 100, 0), (2, 6, 0)]
    {
        let page = list_work_items_lean(&pool, None, None, archived_default(), limit, offset)
            .await
            .expect("list");
        assert_eq!(
            page.items.len(),
            expected_items,
            "limit {limit} offset {offset}: item count"
        );
        assert_eq!(
            page.total, 5,
            "limit {limit} offset {offset}: `total` is the filtered corpus, not the page — \
             a client computing `remaining = total - offset` reads garbage otherwise"
        );
        assert_eq!(page.limit, limit, "the echoed knobs describe the request");
        assert_eq!(page.offset, offset);
    }
}

/// `total` counts what the filters left, and `omitted` counts what they hid —
/// two different numbers over the same corpus, and folding them into one query
/// (which is how #883 was fixed) is exactly the change that could confuse them.
#[tokio::test]
async fn total_and_omitted_stay_distinct_after_the_fold() {
    let (_c, pool) = fresh_korg().await;
    for (title, status) in [
        ("open one", "open"),
        ("resolved one", "resolved"),
        ("closed one", "closed"),
        ("closed two", "closed"),
    ] {
        create_work_item(
            &pool,
            NewWorkItem {
                wi_status: status.into(),
                ..new::work_item(title)
            },
        )
        .await
        .expect("create wi");
    }
    let doomed = create_work_item(&pool, new::work_item("archived one"))
        .await
        .expect("create wi");
    update_work_item(
        &pool,
        doomed.wi_number,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("archive");

    let default = list_work_items_lean(&pool, None, None, archived_default(), 200, 0)
        .await
        .expect("list");
    assert_eq!(default.total, 2, "open + resolved, live only");
    assert_eq!(default.omitted.closed, 2);
    assert_eq!(default.omitted.archived, 1);

    // Past the end, the same three numbers: the empty page was where they used
    // to disagree, `omitted` staying right while `total` went to zero.
    let past = list_work_items_lean(&pool, None, None, archived_default(), 200, 50)
        .await
        .expect("list");
    assert!(past.items.is_empty());
    assert_eq!(past.total, 2);
    assert_eq!(past.omitted.closed, 2);
    assert_eq!(past.omitted.archived, 1);

    let everything = list_work_items_lean(&pool, Some("nope"), None, archived_default(), 200, 0)
        .await
        .expect("list");
    assert_eq!(
        everything.total, 0,
        "an empty *corpus* still reports zero — the fix must not floor `total` at something"
    );
}

// ---------------------------------------------------------------------------
// #884 — archived projects as write targets
// ---------------------------------------------------------------------------

/// The probe: `create_work_item {project:"kris"}` — archived — succeeded and
/// filed the item there. The MCP instructions call the active roster "the only
/// valid targets for new work" and the project param promises an unresolvable
/// name fails "rather than mis-filing"; a known-but-archived name resolved and
/// mis-filed silently, which is the exact failure that promise exists to
/// prevent.
///
/// Every create that takes a project is asserted, because the fix lives in the
/// shared resolver and the value of putting it there is that no writer can miss
/// it.
#[tokio::test]
async fn archived_projects_are_refused_by_every_create() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;
    make_project(&pool, "live", "active").await;
    make_project(&pool, "retired", "archived").await;

    let wi = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("retired".into()),
            ..new::work_item("mis-filed")
        },
    )
    .await;
    let err = wi.expect_err("an archived project must not take new work");
    assert_eq!(code(&err), ErrorCode::InvalidInput);
    let message = err.to_string();
    assert!(
        message.contains("retired") && message.contains("archived"),
        "the refusal must name the project and its status, so a stale name reads \
         as retired rather than misspelled: {message}"
    );

    for (what, result) in [
        (
            "create_card",
            create_card(
                &pool,
                NewCard {
                    project: Some("retired".into()),
                    ..new::card("card")
                },
            )
            .await
            .map(|_| ()),
        ),
        (
            "create_link",
            create_link(
                &pool,
                NewLink {
                    project: Some("retired".into()),
                    ..new::link("https://example.invalid/x")
                },
            )
            .await
            .map(|_| ()),
        ),
        (
            "propose_sprint",
            create_proposal(
                &pool,
                NewProposal {
                    project: Some("retired".into()),
                    summary: "s".into(),
                    ..new::proposal("proposal")
                },
            )
            .await
            .map(|_| ()),
        ),
    ] {
        let err = result.expect_err(&format!("{what} must refuse an archived project"));
        assert_eq!(code(&err), ErrorCode::InvalidInput, "{what}");
    }

    // The active sibling still works — the check narrows to status, not to
    // "projects are hard now".
    create_work_item(
        &pool,
        NewWorkItem {
            project: Some("live".into()),
            ..new::work_item("filed correctly")
        },
    )
    .await
    .expect("an active project still takes work");
}

/// `project_id` must not route around what `project` refuses, and a *move* into
/// an archived project is the same mis-file as a create there. Moving out stays
/// legal: that is the remedy, not the offence.
#[tokio::test]
async fn archived_projects_are_refused_by_id_and_by_move() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "live", "active").await;
    make_project(&pool, "retired", "archived").await;
    let retired_id: i64 = sqlx::query_scalar("SELECT id FROM project WHERE name = 'retired'")
        .fetch_one(&pool)
        .await
        .expect("id");

    let by_id = create_work_item(
        &pool,
        NewWorkItem {
            project_id: Some(retired_id),
            ..new::work_item("by id")
        },
    )
    .await;
    assert_eq!(
        code(&by_id.expect_err("project_id must not be the back door")),
        ErrorCode::InvalidInput
    );

    let wi = create_work_item(
        &pool,
        NewWorkItem {
            project: Some("live".into()),
            ..new::work_item("movable")
        },
    )
    .await
    .expect("create");

    let moved = update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            project: Some(Some("retired".into())),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        code(&moved.expect_err("a move into an archived project is the same mis-file")),
        ErrorCode::InvalidInput
    );

    let card = create_card(
        &pool,
        NewCard {
            project: Some("live".into()),
            ..new::card("movable card")
        },
    )
    .await
    .expect("create card");
    let card_moved = update_card(
        &pool,
        card.node_id,
        CardPatch {
            project: Some(Some("retired".into())),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        code(&card_moved.expect_err("cards move under the same rule")),
        ErrorCode::InvalidInput
    );

    // Unassigning, and moving to an active project, both still work.
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            project: Some(None),
            ..Default::default()
        },
    )
    .await
    .expect("unassigning is not a move into anything");
}

/// Un-archiving must stay reachable. `update_project` deliberately does not go
/// through the resolver that refuses archived targets — if it did, a project
/// could be archived but never restored.
#[tokio::test]
async fn an_archived_project_can_still_be_reactivated() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "retired", "archived").await;

    let restored = update_project_by_name(
        &pool,
        "retired",
        &ProjectPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .expect("an archived project must still be editable");
    assert_eq!(restored.status, "active");

    create_work_item(
        &pool,
        NewWorkItem {
            project: Some("retired".into()),
            ..new::work_item("after restore")
        },
    )
    .await
    .expect("and takes work again once active");
}

// ---------------------------------------------------------------------------
// #885 — `updated` on ordinary edits
// ---------------------------------------------------------------------------

/// The probes: WI #872's title, status, details and sprint edits all returned
/// `updated == created`, frozen across four separate calls; card #877's
/// status+rank move, frozen; proposal #868's status transition, frozen. Only
/// `archived` moved the timestamp, because `archived` is the one field on the
/// `node` row that the touch trigger watches.
///
/// So: one assertion per kind per *field class*, since the class is what the
/// bug was keyed on — a field on the satellite table versus a field on `node`.
#[tokio::test]
async fn updated_advances_on_ordinary_edits_of_every_kind() {
    let (_c, pool) = fresh_korg().await;
    test_project(&pool).await;

    async fn node_updated(pool: &PgPool, node_id: i64) -> OffsetDateTime {
        sqlx::query_scalar("SELECT updated FROM node WHERE id = $1")
            .bind(node_id)
            .fetch_one(pool)
            .await
            .expect("read updated")
    }

    let wi = create_work_item(&pool, new::work_item("subject"))
        .await
        .expect("create wi");
    let created = node_updated(&pool, wi.node_id).await;

    // Satellite-table fields, one at a time: each was independently frozen, and
    // asserting them together would let one moving cover for six that don't.
    let wi_edits: Vec<(&str, WorkItemPatch)> = vec![
        (
            "title",
            WorkItemPatch {
                title: Some("retitled".into()),
                ..Default::default()
            },
        ),
        (
            "wi_status",
            WorkItemPatch {
                wi_status: Some("resolved".into()),
                ..Default::default()
            },
        ),
        (
            "content",
            WorkItemPatch {
                content: Some("body".into()),
                ..Default::default()
            },
        ),
        (
            "details",
            WorkItemPatch {
                details: Some(Some("more".into())),
                ..Default::default()
            },
        ),
        (
            "wi_tshirt",
            WorkItemPatch {
                wi_tshirt: Some("M".into()),
                ..Default::default()
            },
        ),
        (
            "sprint",
            WorkItemPatch {
                sprint: Some(Some("038".into())),
                ..Default::default()
            },
        ),
    ];
    let mut previous = created;
    for (field, patch) in wi_edits {
        update_work_item(&pool, wi.wi_number, patch)
            .await
            .expect("update wi");
        let now = node_updated(&pool, wi.node_id).await;
        assert!(
            now > previous,
            "work item `{field}`: updated must advance on an ordinary edit \
             (was {previous}, now {now}) — an agent sorting by recency sees \
             creation order otherwise"
        );
        previous = now;
    }
    // The node-column field that always worked, asserted so the fix cannot have
    // traded one for the other.
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            archived: Some(true),
            ..Default::default()
        },
    )
    .await
    .expect("archive");
    assert!(node_updated(&pool, wi.node_id).await > previous, "archived");

    let card = create_card(&pool, new::card("subject card"))
        .await
        .expect("create card");
    let card_created = node_updated(&pool, card.node_id).await;
    update_card(
        &pool,
        card.node_id,
        CardPatch {
            status: Some("Active".into()),
            rank: Some(rust_decimal::Decimal::new(15, 1)),
            ..Default::default()
        },
    )
    .await
    .expect("update card");
    assert!(
        node_updated(&pool, card.node_id).await > card_created,
        "card status+rank move: the exact probe that came back frozen"
    );

    let proposal = create_proposal(
        &pool,
        NewProposal {
            summary: "s".into(),
            ..new::proposal("subject proposal")
        },
    )
    .await
    .expect("create proposal");
    let proposal_created = node_updated(&pool, proposal.row.node_id).await;
    update_proposal(
        &pool,
        proposal.row.node_id,
        ProposalPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .expect("update proposal");
    assert!(
        node_updated(&pool, proposal.row.node_id).await > proposal_created,
        "proposal proposed→active: starting a sprint must move the timestamp"
    );

    // Links were never probed by T3 and had the identical defect — the same
    // satellite-table mechanism, so the same fix, so the same fence.
    let link = create_link(&pool, new::link("https://example.invalid/read"))
        .await
        .expect("create link");
    let link_created = node_updated(&pool, link.node_id).await;
    update_link(
        &pool,
        link.node_id,
        LinkPatch {
            disposition: Some("Done".into()),
            ..Default::default()
        },
    )
    .await
    .expect("update link");
    let after_patch = node_updated(&pool, link.node_id).await;
    assert!(
        after_patch > link_created,
        "link disposition via update_link"
    );

    set_link_disposition(&pool, link.node_id, "Revisit")
        .await
        .expect("set disposition");
    let after_set = node_updated(&pool, link.node_id).await;
    assert!(after_set > after_patch, "set_link_disposition");

    mark_link_read(&pool, link.node_id, true)
        .await
        .expect("mark read");
    assert!(
        node_updated(&pool, link.node_id).await > after_set,
        "mark_link_read"
    );
}

// ---------------------------------------------------------------------------
// #886 — the freeze that was never broken
// ---------------------------------------------------------------------------

/// **This one is a refutation.** T3 reported create/delete/reorder all mutating
/// a past day; they do not, and did not. The probes ran at 2026-08-02T01:10Z
/// against production, which runs `KORG_TIMEZONE=America/Los_Angeles` — so the
/// server's local today was still 2026-08-01, and the "past day" being written
/// was the current one.
///
/// What is asserted here is the freeze itself, per operation, against a pinned
/// `today`, so the next session to read the daily-plan module finds the answer
/// in a test rather than re-deriving it from a timezone. The second half
/// asserts the actual fix that shipped: the refusal *names* the server's local
/// date, which is what would have shown the T3 session its clock disagreed.
#[tokio::test]
async fn past_days_are_frozen_and_the_refusal_names_the_servers_today() {
    let (_c, pool) = fresh_korg().await;
    let today = date!(2026 - 08 - 02);
    let yesterday = date!(2026 - 08 - 01);
    let context = LifecycleContext {
        today,
        now: OffsetDateTime::now_utc(),
    };

    let wi = create_work_item(&pool, new::work_item("plannable"))
        .await
        .expect("create wi");

    // Create onto a past day.
    let created = daily_plan::create_item(&pool, wi.node_id, yesterday, &context).await;
    let err = created.expect_err("a past day must not accept new items");
    assert!(matches!(err, PlanningError::TargetPast { .. }));
    assert_eq!(err.code(), ErrorCode::InvalidInput);
    assert_names_the_boundary(&err.to_string(), yesterday, today);

    // Seed one item on the past day the only way the server allows: plan it
    // while that day is current.
    let past_context = LifecycleContext {
        today: yesterday,
        now: OffsetDateTime::now_utc(),
    };
    let seeded = daily_plan::create_item(&pool, wi.node_id, yesterday, &past_context)
        .await
        .expect("planning today is what the freeze exists to permit");

    // Delete from a past day.
    let deleted = daily_plan::delete_item(&pool, seeded.node_id, &context).await;
    let err = deleted.expect_err("past structure is frozen");
    assert!(matches!(err, PlanningError::FrozenPast { .. }));
    assert_eq!(
        err.code(),
        ErrorCode::Conflict,
        "a frozen past is a state conflict, not a malformed request"
    );
    assert_names_the_boundary(&err.to_string(), yesterday, today);

    // Reorder a past day.
    let reordered = daily_plan::reorder_day(&pool, yesterday, &[seeded.node_id], &context).await;
    let err = reordered.expect_err("past order is frozen");
    assert!(matches!(err, PlanningError::FrozenPast { .. }));
    assert_names_the_boundary(&err.to_string(), yesterday, today);

    // Move *to* a past day.
    let moved = daily_plan::move_item(&pool, seeded.node_id, yesterday, 0, &context).await;
    let err = moved.expect_err("a past target is refused");
    assert!(matches!(err, PlanningError::TargetPast { .. }));
    assert_names_the_boundary(&err.to_string(), yesterday, today);

    // History's end is the same boundary, and used to be the one refusal that
    // could not say which date it meant.
    let history = daily_plan::history(&pool, yesterday, today, None, &context).await;
    let err = history.expect_err("history must end before today");
    assert!(matches!(err, PlanningError::HistoryNotPast { .. }));
    assert_eq!(err.code(), ErrorCode::InvalidInput);
    assert_names_the_boundary(&err.to_string(), today, today);

    // And the day itself is still open for business.
    daily_plan::create_item(&pool, wi.node_id, today, &context)
        .await
        .expect("today is not the past");
}

/// A date refusal has to carry both dates: the one asked for and the one the
/// server measured against. Without the second, a caller on a different clock
/// can only guess — which is the whole of WI #886.
fn assert_names_the_boundary(message: &str, requested: Date, today: Date) {
    let requested = requested.to_string();
    let today = today.to_string();
    assert!(
        message.contains(&today),
        "the refusal must name the server's local today ({today}): {message}"
    );
    assert!(
        message.contains(&requested) || requested == today,
        "the refusal must name the date it refused ({requested}): {message}"
    );
}

// ---------------------------------------------------------------------------
// #887 — src_path
// ---------------------------------------------------------------------------

/// The probe: `update_project {name:"korg", src_path:"~/src/tools/korg (dev
/// copy)"}` → `{code:"internal", message:"… violates check constraint
/// \"project_src_path_canonical\""}`. The constraint did its job — nothing was
/// stored — but a user-correctable input error surfaced at the tier reserved
/// for korg's own faults, with leaked Postgres text as its only documentation.
#[tokio::test]
async fn a_non_canonical_src_path_is_invalid_input_not_internal() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject", "active").await;

    let patch = |value: &str| ProjectPatch {
        src_path: Some(Some(value.into())),
        ..Default::default()
    };

    for (value, why) in [
        ("~/src/tools/korg (dev copy)", "parentheses"),
        ("~/src/tools/korg (kai; was ~/src/korg)", "prose"),
        ("~/src/my korg", "whitespace"),
        ("/home/ken/src/tools/korg", "absolute"),
        ("src/tools/korg", "no ~/ prefix"),
        ("~/src/tools/korg/", "trailing slash"),
    ] {
        let err = update_project_by_name(&pool, "subject", &patch(value))
            .await
            .expect_err(&format!("{why} must be refused: {value}"));
        assert_eq!(
            code(&err),
            ErrorCode::InvalidInput,
            "{why}: a correctable input error must not read as korg's fault"
        );
        let message = err.to_string();
        assert!(
            !message.contains("check constraint") && !message.contains("relation"),
            "{why}: raw Postgres text is not documentation: {message}"
        );
        assert!(
            message.contains(value) && message.contains("~/src/tools/korg"),
            "{why}: the error should echo the value and show the form it wants: {message}"
        );
    }

    // The canonical form still stores, and clearing still clears — the check
    // must not have turned a correctable field into an unusable one.
    let ok = update_project_by_name(&pool, "subject", &patch("~/src/tools/korg"))
        .await
        .expect("a canonical path stores");
    assert_eq!(ok.src_path.as_deref(), Some("~/src/tools/korg"));

    let cleared = update_project_by_name(
        &pool,
        "subject",
        &ProjectPatch {
            src_path: Some(None),
            ..Default::default()
        },
    )
    .await
    .expect("clearing stays legal — NULL means 'not recorded'");
    assert_eq!(cleared.src_path, None);
}
