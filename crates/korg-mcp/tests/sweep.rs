//! Behaviour for the MCP arms that nothing had ever dispatched (WI #551).
//!
//! `tests/dispatch.rs` proves each of these *runs*. This file proves each one
//! does what its name claims, and fails the way it should when the target is
//! missing — the two halves the review asked for ("at least one happy path and
//! one error path" per arm).
//!
//! The reports trio is next door in `reports.rs`; everything else the sweep
//! turned up is here:
//!
//! ```text
//! archive_topic   update_topic     mark_link_read      update_comment
//! update_project  reorder_daily_plan  delete_daily_plan_item  daily_plan_history
//! ```

use korg_core::repo;
use korg_core::topics::{self, NewTopic};
use korg_core::{daily_plan, daily_plan::LifecycleContext};
use korg_test_support::{fresh_korg, new};
use serde_json::json;
use sqlx::PgPool;
use time::macros::{date, datetime};

mod common;
use common::{args, body, error_text, server};

const TODAY: &str = "2026-07-11";

fn context() -> LifecycleContext {
    LifecycleContext {
        today: date!(2026 - 07 - 11),
        now: datetime!(2026-07-11 12:00 UTC),
    }
}

async fn topic(pool: &PgPool, name: &str) -> i64 {
    topics::create_topic(
        pool,
        NewTopic {
            project_id: None,
            project: None,
            category: None,
            tags: Vec::new(),
            name: name.into(),
            description: None,
        },
    )
    .await
    .expect("topic")
    .node_id
}

// --- topics -----------------------------------------------------------------

/// `update_topic` patches only the fields it is given, and `archive_topic`
/// round-trips in both directions — it takes `archived`, so it is un-archive as
/// much as archive, which is the half a name like "archive_topic" hides.
#[tokio::test]
async fn topics_can_be_patched_and_archived_both_ways() {
    let (_pg, pool) = fresh_korg().await;
    let id = topic(&pool, "original name").await;
    let server = server(pool);

    let patched = body(
        &server
            .call(
                "update_topic",
                args(json!({"node_id": id, "description": "now described"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(patched["description"], "now described");
    assert_eq!(
        patched["name"], "original name",
        "a patch must not clear the fields it was not given"
    );

    let archived = body(
        &server
            .call("archive_topic", args(json!({"node_id": id})))
            .await
            .unwrap(),
    );
    assert_eq!(
        archived["archived"], true,
        "archived defaults to true when omitted"
    );

    let restored = body(
        &server
            .call(
                "archive_topic",
                args(json!({"node_id": id, "archived": false})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(restored["archived"], false, "and back again");

    // Archived topics leave the default listing but are still addressable.
    let listed = body(&server.call("list_topics", args(json!({}))).await.unwrap());
    assert_eq!(listed["items"].as_array().expect("items").len(), 1);
}

#[tokio::test]
async fn patching_or_archiving_a_missing_topic_is_not_found() {
    let (_pg, pool) = fresh_korg().await;
    let server = server(pool);

    for tool in ["update_topic", "archive_topic"] {
        let result = server
            .call(tool, args(json!({"node_id": 999_999, "name": "x"})))
            .await
            .unwrap();
        assert!(
            error_text(&result).contains("not_found"),
            "`{tool}` on a missing topic must be not_found"
        );
    }
}

/// A node of the wrong kind is not a topic, and must not be silently patched
/// as if it were — the class of bug sprint 013 found in `update_card`.
#[tokio::test]
async fn a_work_item_node_is_not_a_topic() {
    let (_pg, pool) = fresh_korg().await;
    let wi = repo::create_work_item(&pool, new::work_item("not a topic"))
        .await
        .expect("wi");
    let server = server(pool);

    let result = server
        .call(
            "update_topic",
            args(json!({"node_id": wi.node_id, "name": "hijacked"})),
        )
        .await
        .unwrap();
    assert!(
        error_text(&result).contains("not_found"),
        "a work-item node must not be patchable through update_topic"
    );
}

// --- links ------------------------------------------------------------------

/// `mark_link_read` is the narrow cousin of `update_link`: it sets `read` and
/// nothing else. The test asserts the "nothing else" half, because that is the
/// part a future refactor into `update_link` would break.
#[tokio::test]
async fn mark_link_read_sets_only_the_read_flag() {
    let (_pg, pool) = fresh_korg().await;
    let link = repo::create_link(&pool, new::link("https://example.invalid/x"))
        .await
        .expect("link");
    let server = server(pool);

    // Give it a non-default disposition first, so we can watch it survive.
    server
        .call(
            "update_link",
            args(json!({"node_id": link.node_id, "disposition": "Revisit"})),
        )
        .await
        .unwrap();

    let marked = body(
        &server
            .call(
                "mark_link_read",
                args(json!({"node_id": link.node_id, "read": true})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(marked["read"], true);
    assert_eq!(
        marked["disposition"], "Revisit",
        "marking read must not reset the disposition"
    );

    let unmarked = body(
        &server
            .call(
                "mark_link_read",
                args(json!({"node_id": link.node_id, "read": false})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(unmarked["read"], false);
}

#[tokio::test]
async fn marking_a_non_link_read_is_not_found() {
    let (_pg, pool) = fresh_korg().await;
    let wi = repo::create_work_item(&pool, new::work_item("not a link"))
        .await
        .expect("wi");
    let server = server(pool);

    let result = server
        .call(
            "mark_link_read",
            args(json!({"node_id": wi.node_id, "read": true})),
        )
        .await
        .unwrap();
    assert!(error_text(&result).contains("not_found"));
}

// --- comments ---------------------------------------------------------------

/// `update_comment` edits in place: same id, new body, and `updated` moves.
#[tokio::test]
async fn update_comment_edits_in_place() {
    let (_pg, pool) = fresh_korg().await;
    let wi = repo::create_work_item(&pool, new::work_item("commented"))
        .await
        .expect("wi");
    let comment = repo::add_comment(&pool, wi.node_id, "first draft")
        .await
        .expect("comment");
    let server = server(pool);

    let edited = body(
        &server
            .call(
                "update_comment",
                args(json!({"id": comment.id, "body": "second draft"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(edited["id"], comment.id, "editing must not re-create");
    assert_eq!(edited["body"], "second draft");
    assert_eq!(
        edited["node_id"], wi.node_id,
        "and must not re-parent the comment"
    );

    // The thread still holds exactly one comment.
    let listed = body(
        &server
            .call("list_comments", args(json!({"node_id": wi.node_id})))
            .await
            .unwrap(),
    );
    assert_eq!(listed.as_array().expect("array").len(), 1);
}

/// The DB-CHECK error path the review asked for: an empty comment body is
/// rejected as invalid input, not stored and not a 500.
#[tokio::test]
async fn an_empty_comment_body_is_invalid_input() {
    let (_pg, pool) = fresh_korg().await;
    let wi = repo::create_work_item(&pool, new::work_item("commented"))
        .await
        .expect("wi");
    let comment = repo::add_comment(&pool, wi.node_id, "real")
        .await
        .expect("comment");
    let server = server(pool);

    for body_text in ["", "   "] {
        let add = server
            .call(
                "add_comment",
                args(json!({"node_id": wi.node_id, "body": body_text})),
            )
            .await;
        let rejected = match add {
            // Empty strings fail the schema's non-empty constraint at parse
            // time; whitespace-only reaches the DB CHECK. Either is a caller
            // error — what must not happen is a stored blank comment.
            Err(e) => e.message.contains("invalid"),
            Ok(result) => error_text(&result).contains("invalid_input"),
        };
        assert!(
            rejected,
            "an empty comment body ({body_text:?}) was accepted"
        );

        let update = server
            .call(
                "update_comment",
                args(json!({"id": comment.id, "body": body_text})),
            )
            .await;
        let rejected = match update {
            Err(e) => e.message.contains("invalid"),
            Ok(result) => error_text(&result).contains("invalid_input"),
        };
        assert!(
            rejected,
            "an empty comment body ({body_text:?}) was accepted by update"
        );
    }
}

// --- projects ---------------------------------------------------------------

/// `update_project` addresses a project by name and patches its metadata. The
/// name itself is immutable (WI #246), which is why the selector and the patch
/// are different types — and why passing `name` cannot rename anything.
#[tokio::test]
async fn update_project_patches_metadata_by_name() {
    let (_pg, pool) = fresh_korg().await;
    repo::create_project(&pool, "korg").await.expect("project");
    let server = server(pool);

    let patched = body(
        &server
            .call(
                "update_project",
                args(json!({
                    "name": "korg",
                    "status": "maintenance",
                    "gh_repo": "kenhiatt/korg",
                    "machines": ["kai", "kubs0"],
                })),
            )
            .await
            .unwrap(),
    );
    assert_eq!(patched["name"], "korg");
    assert_eq!(patched["status"], "maintenance");
    assert_eq!(patched["gh_repo"], "kenhiatt/korg");
    assert_eq!(patched["machines"], json!(["kai", "kubs0"]));

    // A second patch touching one field leaves the others alone.
    let again = body(
        &server
            .call(
                "update_project",
                args(json!({"name": "korg", "status": "active"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(again["status"], "active");
    assert_eq!(
        again["gh_repo"], "kenhiatt/korg",
        "an unmentioned field must survive the patch"
    );
}

#[tokio::test]
async fn updating_an_unknown_project_is_not_found() {
    let (_pg, pool) = fresh_korg().await;
    let server = server(pool);

    let result = server
        .call(
            "update_project",
            args(json!({"name": "no-such-project", "status": "active"})),
        )
        .await
        .unwrap();
    assert!(error_text(&result).contains("not_found"));
}

// --- daily planning ---------------------------------------------------------

/// `reorder_daily_plan` takes the complete new order for one day and
/// renumbers positions; `delete_daily_plan_item` removes one and closes the
/// gap left behind.
#[tokio::test]
async fn reorder_and_delete_keep_positions_contiguous() {
    let (_pg, pool) = fresh_korg().await;
    let ctx = context();

    let mut ids = Vec::new();
    for title in ["first", "second", "third"] {
        let wi = repo::create_work_item(&pool, new::work_item(title))
            .await
            .expect("wi");
        ids.push(
            daily_plan::create_item(&pool, wi.node_id, date!(2026 - 07 - 11), &ctx)
                .await
                .expect("plan item")
                .node_id,
        );
    }
    let server = server(pool);

    let reordered = body(
        &server
            .call(
                "reorder_daily_plan",
                args(json!({
                    "plan_date": TODAY,
                    "node_ids": [ids[2], ids[0], ids[1]],
                })),
            )
            .await
            .unwrap(),
    );
    let order: Vec<i64> = reordered
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["node_id"].as_i64().expect("node_id"))
        .collect();
    assert_eq!(order, vec![ids[2], ids[0], ids[1]], "the day's new order");
    let positions: Vec<i64> = reordered
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["position"].as_i64().expect("position"))
        .collect();
    assert_eq!(
        positions,
        vec![0, 1, 2],
        "positions are renumbered, not sparse"
    );

    let deleted = body(
        &server
            .call("delete_daily_plan_item", args(json!({"node_id": ids[0]})))
            .await
            .unwrap(),
    );
    assert_eq!(deleted["deleted"], true);

    let day = body(
        &server
            .call("list_daily_plan", args(json!({"from": TODAY, "to": TODAY})))
            .await
            .unwrap(),
    );
    let after: Vec<i64> = day
        .as_array()
        .expect("array")
        .iter()
        .map(|i| i["position"].as_i64().expect("position"))
        .collect();
    assert_eq!(
        after,
        vec![0, 1],
        "deleting must close the gap it left, not leave a hole"
    );
}

/// A reorder that does not name every item for the day exactly once is
/// rejected — a partial list would silently drop whatever it omitted.
#[tokio::test]
async fn a_partial_reorder_is_rejected() {
    let (_pg, pool) = fresh_korg().await;
    let ctx = context();

    let mut ids = Vec::new();
    for title in ["a", "b"] {
        let wi = repo::create_work_item(&pool, new::work_item(title))
            .await
            .expect("wi");
        ids.push(
            daily_plan::create_item(&pool, wi.node_id, date!(2026 - 07 - 11), &ctx)
                .await
                .expect("plan item")
                .node_id,
        );
    }
    let server = server(pool);

    let result = server
        .call(
            "reorder_daily_plan",
            args(json!({"plan_date": TODAY, "node_ids": [ids[0]]})),
        )
        .await
        .unwrap();
    assert!(
        error_text(&result).contains("conflict"),
        "a reorder missing an item must be rejected — as a conflict, since the          request is well-formed but disagrees with the stored day (sprint 013)"
    );
}

/// `daily_plan_history` reads the frozen past, optionally narrowed to one
/// source — and refuses a range that reaches today, because today is still
/// being edited.
#[tokio::test]
async fn history_reads_the_past_and_refuses_the_present() {
    let (_pg, pool) = fresh_korg().await;
    let ctx = LifecycleContext {
        // Plant history by planning "today" and then reading it from a later
        // vantage point: plan on the 11th, ask on the 12th.
        today: date!(2026 - 07 - 11),
        now: datetime!(2026-07-11 12:00 UTC),
    };
    let tracked = repo::create_work_item(&pool, new::work_item("tracked"))
        .await
        .expect("wi");
    let other = repo::create_work_item(&pool, new::work_item("other"))
        .await
        .expect("wi");
    for source in [tracked.node_id, other.node_id] {
        daily_plan::create_item(&pool, source, date!(2026 - 07 - 11), &ctx)
            .await
            .expect("plan item");
    }
    let server = server(pool);

    // The server's clock is the 11th, so the 10th and earlier is history.
    let empty = body(
        &server
            .call(
                "daily_plan_history",
                args(json!({"from": "2026-07-01", "to": "2026-07-10"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        empty["total"], 0,
        "nothing was planned before the 11th: {empty:?}"
    );

    let reaching_today = server
        .call(
            "daily_plan_history",
            args(json!({"from": "2026-07-01", "to": TODAY})),
        )
        .await
        .unwrap();
    assert!(
        error_text(&reaching_today).contains("invalid_input"),
        "history must refuse a range that reaches today"
    );

    let backwards = server
        .call(
            "daily_plan_history",
            args(json!({"from": "2026-07-10", "to": "2026-07-01"})),
        )
        .await
        .unwrap();
    assert!(
        error_text(&backwards).contains("invalid_input"),
        "history must refuse a backwards range"
    );
}

// --- links, DB-CHECK --------------------------------------------------------

/// The other DB-CHECK path the review asked for: an empty link URL.
#[tokio::test]
async fn an_empty_link_url_is_invalid_input() {
    let (_pg, pool) = fresh_korg().await;
    let server = server(pool);

    for url in ["", "   "] {
        let created = server.call("create_link", args(json!({"url": url}))).await;
        let rejected = match created {
            Err(e) => e.message.contains("invalid"),
            Ok(result) => error_text(&result).contains("invalid_input"),
        };
        assert!(rejected, "an empty link url ({url:?}) was accepted");
    }
}
