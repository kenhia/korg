//! The completeness fence: every advertised tool is actually *dispatched*
//! (WI #551).
//!
//! Sprint 016 shipped `every_advertised_tool_has_a_handler`, which grepped
//! `tools.rs` for each tool's string literal. That proved an arm existed. It
//! would have passed against an arm whose body was `todo!()`, and it did pass
//! for the whole time ten tools had never been called by anything — including
//! `create_report`, whose argument parsing sprint 016 rewrote with no test in
//! the repo able to notice a regression.
//!
//! This replaces it. [`fixtures`] maps every tool name to a valid argument
//! object against one seeded database, and the test below asserts two things:
//!
//! 1. the fixture set **equals** the advertised set — a new tool with no
//!    fixture fails here, which is the entire point; and
//! 2. every one of them dispatches to a non-error result.
//!
//! What this is not: a behavioural test. It proves each arm runs and returns
//! something; `server.rs` and `reports.rs` prove the interesting ones are
//! right. Adding a tool to this file is the floor, not the job.

use korg_core::repo::{self, NewReport};
use korg_core::topics::{self, NewTopic};
use korg_core::{daily_plan, relationships};
use korg_test_support::{fresh_korg, new};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use time::macros::date;
use time::Date;

mod common;
use common::{args, server};

/// The pinned "today" — `common::server` fixes the clock here, and the daily
/// plan refuses to write to dates in the past, so every plan date below is on
/// or after it.
const TODAY: &str = "2026-07-11";

/// Build one database holding an instance of everything the tool surface can
/// address, and return the argument object for each tool.
///
/// Destructive tools (`delete_comment`, `unrelate`, `delete_daily_plan_item`)
/// each get their **own** entity, and each daily-plan tool its own date, so
/// that the order the tools happen to be dispatched in cannot matter. A fixture
/// table whose correctness depends on iteration order is a trap for whoever
/// adds the 45th tool.
async fn fixtures(pool: &PgPool) -> BTreeMap<&'static str, Value> {
    let ctx = daily_plan::LifecycleContext {
        today: date!(2026 - 07 - 11),
        now: time::macros::datetime!(2026-07-11 12:00 UTC),
    };

    repo::create_project(pool, "korg").await.expect("project");
    repo::create_area(pool, "korg", "core", None)
        .await
        .expect("area");

    let wi = repo::create_work_item(pool, new::work_item("subject work item"))
        .await
        .expect("wi");
    let finding = repo::create_work_item(pool, new::work_item("a finding"))
        .await
        .expect("finding wi");
    let card = repo::create_card(pool, new::card("subject card"))
        .await
        .expect("card");

    // Two links: one to patch, one to mark read.
    let link = repo::create_link(pool, new::link("https://example.invalid/one"))
        .await
        .expect("link");
    let readable = repo::create_link(pool, new::link("https://example.invalid/two"))
        .await
        .expect("readable link");

    // Two comments: one to patch, one to delete.
    let comment = repo::add_comment(pool, wi.node_id, "a comment")
        .await
        .expect("comment");
    let doomed_comment = repo::add_comment(pool, wi.node_id, "to be deleted")
        .await
        .expect("doomed comment");

    // Two topics: one to patch, one to archive.
    let topic = topics::create_topic(
        pool,
        NewTopic {
            project_id: None,
            project: None,
            category: None,
            tags: Vec::new(),
            name: "a topic".into(),
            description: None,
        },
    )
    .await
    .expect("topic");
    let doomed_topic = topics::create_topic(
        pool,
        NewTopic {
            project_id: None,
            project: None,
            category: None,
            tags: Vec::new(),
            name: "topic to archive".into(),
            description: None,
        },
    )
    .await
    .expect("archivable topic");

    // A relationship to delete, and a label from the registry so `relate`'s
    // fixture cannot drift out of the vocabulary.
    let doomed_rel = repo::relate(pool, wi.node_id, card.node_id, "related-to", None)
        .await
        .expect("relationship");

    let proposal = repo::create_proposal(pool, new::proposal("a proposal"))
        .await
        .expect("proposal");

    let report = repo::upsert_report(
        pool,
        NewReport {
            findings: vec![finding.wi_number],
            ..new::report("kmon", date!(2026 - 07 - 10))
        },
    )
    .await
    .expect("report");

    // A handoff attached to the subject work item — one to read, one to update.
    let mut new_handoff = new::handoff("a handoff");
    new_handoff.related_node_ids = vec![wi.node_id];
    let handoff = repo::create_handoff(pool, new_handoff)
        .await
        .expect("handoff");

    // One daily-plan item per tool that mutates one, each on its own day, so
    // `reorder` sees exactly the day it owns and `delete` cannot strand `move`.
    async fn plan(
        pool: &PgPool,
        date: Date,
        source: i64,
        ctx: &daily_plan::LifecycleContext,
    ) -> i64 {
        daily_plan::create_item(pool, source, date, ctx)
            .await
            .expect("plan item")
            .node_id
    }
    let completable = plan(pool, date!(2026 - 07 - 11), wi.node_id, &ctx).await;
    let doomed_item = plan(pool, date!(2026 - 07 - 13), card.node_id, &ctx).await;
    let movable = plan(pool, date!(2026 - 07 - 14), finding.node_id, &ctx).await;
    let ordered_a = plan(pool, date!(2026 - 07 - 16), wi.node_id, &ctx).await;
    let ordered_b = plan(pool, date!(2026 - 07 - 16), card.node_id, &ctx).await;

    BTreeMap::from([
        // --- work items ---
        (
            "create_work_item",
            json!({"title": "created by the dispatch fence", "content": ""}),
        ),
        ("list_work_items", json!({})),
        ("survey_work_items", json!({})),
        ("get_work_item", json!({"wi_number": wi.wi_number})),
        (
            "update_work_item",
            json!({"wi_number": wi.wi_number, "title": "retitled"}),
        ),
        // --- cards ---
        ("create_card", json!({"title": "created card"})),
        (
            "update_card",
            json!({"node_id": card.node_id, "title": "retitled card"}),
        ),
        ("list_cards", json!({})),
        // --- comments ---
        ("list_comments", json!({"node_id": wi.node_id})),
        (
            "add_comment",
            json!({"node_id": wi.node_id, "body": "added"}),
        ),
        (
            "update_comment",
            json!({"id": comment.id, "body": "edited"}),
        ),
        ("delete_comment", json!({"id": doomed_comment.id})),
        // --- reading-list links ---
        (
            "create_link",
            json!({"url": "https://example.invalid/created"}),
        ),
        ("list_links", json!({})),
        (
            "update_link",
            json!({"node_id": link.node_id, "disposition": "Done"}),
        ),
        (
            "mark_link_read",
            json!({"node_id": readable.node_id, "read": true}),
        ),
        // --- relationships ---
        (
            "relate",
            json!({"left": proposal.row.node_id, "right": wi.node_id, "label": "covers"}),
        ),
        ("neighbors", json!({"node_id": wi.node_id})),
        ("unrelate", json!({"id": doomed_rel})),
        // --- topics ---
        ("create_topic", json!({"name": "created topic"})),
        ("get_topic", json!({"node_id": topic.node_id})),
        ("list_topics", json!({})),
        ("search_topics", json!({"q": "topic"})),
        (
            "update_topic",
            json!({"node_id": topic.node_id, "name": "renamed topic"}),
        ),
        (
            "archive_topic",
            json!({"node_id": doomed_topic.node_id, "archived": true}),
        ),
        // --- daily planning ---
        (
            "list_daily_plan",
            json!({"from": TODAY, "to": "2026-07-20"}),
        ),
        (
            "create_daily_plan_item",
            json!({"source_node_id": finding.node_id, "plan_date": "2026-07-17"}),
        ),
        (
            "set_daily_plan_completion",
            json!({"node_id": completable, "completed": true}),
        ),
        ("delete_daily_plan_item", json!({"node_id": doomed_item})),
        (
            "reorder_daily_plan",
            json!({"plan_date": "2026-07-16", "node_ids": [ordered_b, ordered_a]}),
        ),
        (
            "move_daily_plan_item",
            json!({"node_id": movable, "target_date": "2026-07-15"}),
        ),
        // History is strictly the past: the range must end before today.
        (
            "daily_plan_history",
            json!({"from": "2026-07-01", "to": "2026-07-10"}),
        ),
        // --- reports ---
        (
            "create_report",
            json!({
                "source": "dispatch-fence",
                "report_date": TODAY,
                "status": "ok",
                "summary": "created by the dispatch fence",
                "body": "",
            }),
        ),
        ("list_reports", json!({})),
        ("get_report", json!({"node_id": report.node_id})),
        // --- sprint proposals ---
        (
            "propose_sprint",
            json!({"title": "proposed by the fence", "summary": "s"}),
        ),
        ("list_proposals", json!({})),
        ("get_proposal", json!({"node_id": proposal.row.node_id})),
        (
            "update_proposal",
            json!({"node_id": proposal.row.node_id, "status": "active"}),
        ),
        // --- handoffs ---
        (
            "create_handoff",
            json!({
                "title": "handed off by the fence",
                "summary": "s",
                "body": "b",
                "related_node_ids": [wi.node_id],
            }),
        ),
        ("get_handoff", json!({"node_id": handoff.handoff.node_id})),
        (
            "update_handoff",
            json!({"node_id": handoff.handoff.node_id, "body": "revised by the fence"}),
        ),
        // --- projects and areas ---
        ("list_projects", json!({})),
        ("create_project", json!({"name": "created-project"})),
        (
            "update_project",
            json!({"name": "korg", "status": "maintenance"}),
        ),
        ("list_areas", json!({"project": "korg"})),
        (
            "create_area",
            json!({"project": "korg", "name": "created-area"}),
        ),
    ])
}

/// Every advertised tool has a fixture, and every fixture dispatches.
///
/// The two halves are asserted separately on purpose. A missing fixture is an
/// authoring error and names the tool; a failing dispatch is a product bug and
/// names the tool *and* the error. Collapsing them would report the first as
/// the second.
#[tokio::test]
async fn every_advertised_tool_is_dispatched() {
    let (_pg, pool) = fresh_korg().await;
    let fixtures = fixtures(&pool).await;
    let server = server(pool);

    let advertised: BTreeSet<String> = korg_mcp::tools::tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    let covered: BTreeSet<String> = fixtures.keys().map(|k| k.to_string()).collect();

    let missing: Vec<&String> = advertised.difference(&covered).collect();
    assert!(
        missing.is_empty(),
        "these tools are advertised but have no dispatch fixture in \
         crates/korg-mcp/tests/dispatch.rs: {missing:?}\n\
         Add one — a tool nothing calls is a tool nothing tests."
    );

    let stale: Vec<&String> = covered.difference(&advertised).collect();
    assert!(
        stale.is_empty(),
        "these fixtures name tools that are no longer advertised: {stale:?}"
    );

    for (name, arguments) in &fixtures {
        let result = server
            .call(name, args(arguments.clone()))
            .await
            .unwrap_or_else(|e| panic!("`{name}` returned a protocol error: {e:?}"));
        assert_ne!(
            result.is_error,
            Some(true),
            "`{name}` dispatched to an error result: {result:?}"
        );
    }
}

/// An unregistered name is a protocol error, not a silent success. The inverse
/// of the fence above: it fixes the *lower* bound of the dispatch table.
#[tokio::test]
async fn an_unknown_tool_is_a_protocol_error() {
    let (_pg, pool) = fresh_korg().await;
    let server = server(pool);

    let err = server
        .call("no_such_tool", args(json!({})))
        .await
        .expect_err("unknown tool must not succeed");
    assert!(
        err.message.contains("unknown tool"),
        "unhelpful message for an unknown tool: {err:?}"
    );
}

/// `relate` fixtures use a registry label; if the registry were emptied or the
/// label renamed, the fence above would still pass with a label korg no longer
/// understands. This ties the fixture to the vocabulary.
#[test]
fn the_relate_fixture_uses_a_registered_label() {
    assert!(
        relationships::spec("covers").is_some(),
        "the dispatch fixture for `relate` uses an unregistered label"
    );
}
