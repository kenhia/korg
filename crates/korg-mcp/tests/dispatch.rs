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

use korg_core::relationships;
use korg_core::repo::{self, NewReport};
use korg_test_support::{fresh_korg, new};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::{BTreeMap, BTreeSet};
use time::macros::date;
use time::{Duration, OffsetDateTime};

mod common;
use common::{args, body, server};

/// The "today" the report fixtures write against.
const TODAY: &str = "2026-07-11";

/// Build one database holding an instance of everything the tool surface can
/// address, and return the argument object for each tool.
///
/// Destructive tools (`delete_comment`, `unrelate`, `delete_link`) each get
/// their **own** entity, so that the order the tools happen to be dispatched
/// in cannot matter. A fixture table whose correctness depends on iteration
/// order is a trap for whoever adds the 45th tool.
async fn fixtures(pool: &PgPool) -> BTreeMap<&'static str, Value> {
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

    // Three links: one to patch, one to mark read, one to delete.
    let link = repo::create_link(pool, new::link("https://example.invalid/one"))
        .await
        .expect("link");
    let readable = repo::create_link(pool, new::link("https://example.invalid/two"))
        .await
        .expect("readable link");
    let doomed_link = repo::create_link(pool, new::link("https://example.invalid/three"))
        .await
        .expect("doomed link");

    // Two more areas: one to rename, one to delete. Both stay unreferenced —
    // delete_area refuses while work items are filed under it (#889).
    repo::create_area(pool, "korg", "renameable", None)
        .await
        .expect("renameable area");
    repo::create_area(pool, "korg", "doomed", None)
        .await
        .expect("doomed area");

    // Two comments: one to patch, one to delete.
    let comment = repo::add_comment(pool, wi.node_id, "a comment")
        .await
        .expect("comment");
    let doomed_comment = repo::add_comment(pool, wi.node_id, "to be deleted")
        .await
        .expect("doomed comment");

    // A relationship to delete, and a label from the registry so `relate`'s
    // fixture cannot drift out of the vocabulary.
    let doomed_rel = repo::relate(pool, wi.node_id, card.node_id, "related-to", None, None)
        .await
        .expect("relationship");

    let proposal = repo::create_proposal(pool, new::proposal("a proposal"))
        .await
        .expect("proposal");

    // A program with one slice, so `get_program`'s rollup has something to roll
    // up and `update_program` has a target (#968).
    let program = repo::create_program(
        pool,
        korg_core::repo::NewProgram {
            slices: vec![proposal.row.node_id],
            ..new::program("a program")
        },
    )
    .await
    .expect("program");

    // A schedule that is already due, so `get_schedule`/`update_schedule` have a
    // target and `materialize_schedule` has something it will actually fire.
    let schedule = repo::create_schedule(
        pool,
        new::schedule(
            "a drill - {MONTH} {YEAR}",
            "quarterly",
            Some(OffsetDateTime::now_utc() - Duration::days(200)),
        ),
    )
    .await
    .expect("schedule");

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

    // An attachment owned by the subject work item (#582/#1119). Recorded
    // straight through the repo rather than uploaded: this fence is about
    // dispatch, and korg-mcp has no store — the bytes half of the feature is
    // REST's, and `korg-api/tests/images.rs` is where it is exercised.
    let attachment = repo::create_attachment(
        pool,
        repo::NewAttachment {
            owner_node_id: Some(wi.node_id),
            ..new::attachment("screenshot.png")
        },
    )
    .await
    .expect("attachment");

    BTreeMap::from([
        // --- work items ---
        (
            "create_work_item",
            json!({"title": "created by the dispatch fence", "content": ""}),
        ),
        ("list_work_items", json!({})),
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
        ("delete_link", json!({"node_id": doomed_link.node_id})),
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
            json!({"title": "proposed by the fence", "summary": "s", "project": "korg"}),
        ),
        ("list_proposals", json!({})),
        ("get_proposal", json!({"node_id": proposal.row.node_id})),
        (
            "update_proposal",
            json!({"node_id": proposal.row.node_id, "status": "active"}),
        ),
        // --- programs and the awaiting-Ken marker (#968, #969) ---
        (
            "create_program",
            json!({"title": "created by the fence", "aim": "a"}),
        ),
        ("list_programs", json!({})),
        ("get_program", json!({"node_id": program.row.node_id})),
        (
            "update_program",
            json!({"node_id": program.row.node_id, "status": "holding"}),
        ),
        (
            "set_awaiting",
            json!({"node_id": wi.node_id, "awaiting": true, "note": "raised by the fence"}),
        ),
        ("list_awaiting", json!({})),
        // --- schedules and report-source staleness (#581, #950) ---
        (
            "create_schedule",
            json!({"title": "created by the fence", "cadence": "weekly", "project": "korg"}),
        ),
        ("list_schedules", json!({})),
        ("get_schedule", json!({"node_id": schedule.node_id})),
        (
            "update_schedule",
            json!({"node_id": schedule.node_id, "status": "active"}),
        ),
        ("materialize_schedule", json!({"node_id": schedule.node_id})),
        ("list_report_sources", json!({})),
        (
            "set_report_source",
            json!({"source": "kmon", "cadence_days": 1}),
        ),
        // --- the board rollup (#970) ---
        ("get_board", json!({})),
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
        // --- attachments (#582/#1119) ---
        //
        // `get_attachment` is addressed by its display id here, and
        // `list_attachments` by the owner's node id — between them the two
        // spellings the selector accepts both get dispatched.
        ("get_attachment", json!({"img_id": attachment.img_id})),
        ("list_attachments", json!({"node_id": wi.node_id})),
        // --- projects and areas ---
        ("list_projects", json!({})),
        ("get_project", json!({"name": "korg"})),
        ("create_project", json!({"name": "created-project"})),
        (
            "update_project",
            json!({"name": "korg", "status": "archived"}),
        ),
        ("list_areas", json!({"project": "korg"})),
        (
            "create_area",
            json!({"project": "korg", "name": "created-area"}),
        ),
        (
            "update_area",
            json!({"project": "korg", "name": "renameable", "new_name": "renamed"}),
        ),
        ("delete_area", json!({"project": "korg", "name": "doomed"})),
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

/// The tool names inside the parenthesised list that follows `prefix` in the
/// server instructions — the normative claim, read as data.
fn named_in_instructions(prefix: &str) -> Vec<String> {
    let text = korg_mcp::server_instructions();
    let at = text
        .find(prefix)
        .unwrap_or_else(|| panic!("server_instructions no longer says {prefix:?}"));
    let rest = &text[at + prefix.len()..];
    let open = rest.find('(').expect("a parenthesised list follows");
    let close = rest.find(')').expect("the list closes");
    rest[open + 1..close]
        .split(',')
        .map(|n| n.trim().to_string())
        .collect()
}

/// Collection reads return the shape the instructions promise (WI #579).
///
/// The `instructions` string is the only overview an MCP client sees before it
/// calls anything, and sprint 020 caught it claiming an envelope five reads
/// never return — found the hard way, by a test asserting `body["items"]`
/// against a bare array. Sprint 020 corrected the prose; this is the fence that
/// keeps it corrected, because prose about shapes is exactly the kind of fact
/// that rots silently.
///
/// Ken's decision (2026-07-24, WI #579): the unpaginated reads STAY bare —
/// `total`/`limit`/`offset` are noise on a read that never pages — so what
/// needed defending was the claim, not the shape.
///
/// Both halves matter. Completeness: every advertised collection read is
/// classified, so a new one cannot ship undocumented. Correctness: each
/// classification matches what the tool actually returns over the wire.
#[tokio::test]
async fn collection_reads_return_the_shape_the_instructions_promise() {
    let paginated = named_in_instructions("Paginated collection reads");
    let bare = named_in_instructions("the unpaginated ones");
    // A third shape since #828, and two reads in it since #852: `list_projects`
    // and `list_proposals` return {items, omitted} because they filter rows by
    // default, and a filtered list that looks bare is how "no such project" gets
    // concluded from "you didn't ask for archived ones".
    let filtered = named_in_instructions("the filtered ones");

    // The `list_*` tools are exactly the collection reads the instructions
    // summarise, now that the `survey_work_items` alias is gone (#871).
    // `neighbors` carries its own shape and is named in docs/api.md's shape
    // table instead.
    let classified: BTreeSet<&str> = paginated
        .iter()
        .chain(bare.iter())
        .chain(filtered.iter())
        .map(String::as_str)
        .collect();
    let advertised: BTreeSet<String> = korg_mcp::tools::tools()
        .iter()
        .map(|t| t.name.to_string())
        .filter(|n| n.starts_with("list_"))
        .collect();
    let unclassified: Vec<&String> = advertised
        .iter()
        .filter(|n| !classified.contains(n.as_str()))
        .collect();
    assert!(
        unclassified.is_empty(),
        "these collection reads are advertised but the server instructions do not \
         say what shape they return: {unclassified:?}"
    );
    let phantom: Vec<&&str> = classified
        .iter()
        .filter(|n| !advertised.contains(**n))
        .collect();
    assert!(
        phantom.is_empty(),
        "the server instructions name collection reads that are not advertised: {phantom:?}"
    );

    let (_pg, pool) = fresh_korg().await;
    let fixtures = fixtures(&pool).await;
    let server = server(pool);

    for name in &paginated {
        let value = body(
            &server
                .call(name, args(fixtures[name.as_str()].clone()))
                .await
                .unwrap(),
        );
        let has_envelope = ["items", "total", "limit", "offset"]
            .iter()
            .all(|k| value.get(k).is_some());
        assert!(
            has_envelope,
            "`{name}` is documented as paginated but returned {value}"
        );
        // `list_work_items` is the paginated read that ALSO narrows by
        // default (#851, #861), and the instructions say so in as many words.
        if name == "list_work_items" {
            let omitted = value.get("omitted");
            assert!(
                omitted.is_some_and(|o| o.get("closed").is_some() && o.get("archived").is_some()),
                "the instructions say `{name}` carries `omitted` {{closed, archived}}, \
                 but it returned {value} — a sweep cannot tell a narrowed count from a \
                 complete one without it"
            );
        }
    }
    for name in &bare {
        let value = body(
            &server
                .call(name, args(fixtures[name.as_str()].clone()))
                .await
                .unwrap(),
        );
        assert!(
            value.is_array(),
            "`{name}` is documented as a bare array but returned {value}"
        );
    }
    for name in &filtered {
        let value = body(
            &server
                .call(name, args(fixtures[name.as_str()].clone()))
                .await
                .unwrap(),
        );
        assert!(
            value.get("items").is_some() && value.get("omitted").is_some(),
            "`{name}` is documented as {{items, omitted}} but returned {value}"
        );
    }
}
