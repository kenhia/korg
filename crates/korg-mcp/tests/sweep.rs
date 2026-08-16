//! Behaviour for the MCP arms that nothing had ever dispatched (WI #551).
//!
//! `tests/dispatch.rs` proves each of these *runs*. This file proves each one
//! does what its name claims, and fails the way it should when the target is
//! missing — the two halves the review asked for ("at least one happy path and
//! one error path" per arm).
//!
//! The reports trio is next door in `reports.rs`; everything else the sweep
//! turned up is here (its topic and daily-plan arms left with the slots
//! feature in sprint 050, WI #965):
//!
//! ```text
//! mark_link_read  update_comment  update_project
//! ```

use korg_core::repo;
use korg_test_support::{fresh_korg, new};
use serde_json::json;

mod common;
use common::{args, body, error_text, server};

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
                    "status": "archived",
                    "gh_repo": "kenhiatt/korg",
                    "machines": ["kai", "kubs0"],
                })),
            )
            .await
            .unwrap(),
    );
    assert_eq!(patched["name"], "korg");
    assert_eq!(patched["status"], "archived");
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

// --- the board rollup (#970) ------------------------------------------------

/// `get_board` over MCP: one call, every panel, and no arguments to get wrong.
///
/// The claim under test is the one an agent reads in the tool description — that
/// this replaces walking the queue proposal by proposal. So this asserts the
/// *keys* a consumer types against, and that a schema with no properties is
/// exactly what the tool advertises.
#[tokio::test]
async fn get_board_returns_every_panel_and_takes_no_arguments() {
    let (_pg, pool) = fresh_korg().await;
    repo::create_project(&pool, "korg").await.unwrap();
    let wi = repo::create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            project: Some("korg".into()),
            ..new::work_item("in flight")
        },
    )
    .await
    .unwrap();
    let proposal = repo::create_proposal(
        &pool,
        korg_core::repo::NewProposal {
            summary: "the mission".into(),
            covers: vec![wi.wi_number],
            ..new::proposal_in("korg", "firing")
        },
    )
    .await
    .unwrap()
    .row
    .node_id;
    repo::update_proposal(
        &pool,
        proposal,
        korg_core::repo::ProposalPatch {
            status: Some("active".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    repo::set_awaiting(&pool, wi.node_id, true, Some("your call"))
        .await
        .unwrap();

    let server = server(pool);
    let board = body(&server.call("get_board", args(json!({}))).await.unwrap());

    for key in [
        "generated",
        "active",
        "queue",
        "proposals_omitted",
        "programs",
        "programs_omitted",
        "awaiting",
        "depth",
        "reports",
    ] {
        assert!(board.get(key).is_some(), "the board is missing `{key}`");
    }
    assert_eq!(board["active"][0]["title"], "firing");
    assert_eq!(board["active"][0]["covered_count"], 1);
    assert_eq!(board["active"][0]["open"], 1);
    assert_eq!(board["awaiting"][0]["awaiting_note"], "your call");
    assert!(
        board.get("counts").is_none(),
        "D-3: no counters block — every header figure is derivable from these lists"
    );

    // D-1, as the schema an agent actually reads: nothing to pass, nothing to
    // get wrong. A filter here would produce a board of one repo, which is the
    // opposite of the question it answers.
    let schema = korg_mcp::tools::tools()
        .into_iter()
        .find(|t| t.name == "get_board")
        .expect("get_board is advertised")
        .input_schema
        .clone();
    assert_eq!(
        schema.get("properties"),
        Some(&json!({})),
        "get_board takes no arguments"
    );
}

// --- the flow series (#1318) --------------------------------------------------

/// `work_item_flow` over MCP: the envelope names its own contract, the rows
/// carry the churn-vs-durable split, and the advertised schema documents the
/// default window — the number kfdc must NOT hardcode.
#[tokio::test]
async fn work_item_flow_serves_the_series_and_names_its_contract() {
    let (_pg, pool) = fresh_korg().await;
    repo::create_project(&pool, "korg").await.unwrap();
    repo::create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            project: Some("korg".into()),
            ..new::work_item("arrived today")
        },
    )
    .await
    .unwrap();

    let server = server(pool);
    let flow = body(
        &server
            .call("work_item_flow", args(json!({})))
            .await
            .unwrap(),
    );

    for key in [
        "days",
        "horizon",
        "timezone",
        "durable_after_days",
        "generated",
    ] {
        assert!(flow.get(key).is_some(), "the flow is missing `{key}`");
    }
    let days = flow["days"].as_array().expect("days");
    assert_eq!(days.len(), korg_core::repo::FLOW_DAYS_DEFAULT as usize);
    let today = days.last().expect("today's row");
    for key in [
        "day",
        "added",
        "closed",
        "backlog",
        "added_durable",
        "closed_durable",
    ] {
        assert!(today.get(key).is_some(), "a flow day is missing `{key}`");
    }
    assert_eq!(today["added"], 1);
    assert_eq!(today["backlog"], 1);
    assert_eq!(flow["timezone"], "UTC");

    // A narrower window narrows the series; the envelope stays.
    let narrow = body(
        &server
            .call("work_item_flow", args(json!({"days": 2})))
            .await
            .unwrap(),
    );
    assert_eq!(narrow["days"].as_array().expect("days").len(), 2);

    // The schema is where an agent learns the default — it must carry it.
    let schema = korg_mcp::tools::tools()
        .into_iter()
        .find(|t| t.name == "work_item_flow")
        .expect("work_item_flow is advertised")
        .input_schema
        .clone();
    assert_eq!(
        schema["properties"]["days"]["default"],
        json!(korg_core::repo::FLOW_DAYS_DEFAULT),
        "the advertised default and the served default are one constant"
    );
}
