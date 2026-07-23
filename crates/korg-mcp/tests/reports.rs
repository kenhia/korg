//! The MCP reports trio, over a real database (WI #574).
//!
//! `create_report` is MCP-only — there is no REST write route — and until this
//! file nothing in the repo had ever dispatched it. Sprint 016 found that out
//! by changing how it deserializes `report_date` (hand-parsed `String` to
//! `#[serde(with)]` on the shared `NewReport`) and discovering that no test
//! would have caught a regression. It left a serde unit fence for the parsing
//! half; this covers the other half — the path from an argument object through
//! `upsert_report` to the `{node_id, replaced, findings_linked}` response.
//!
//! The drop-and-replace semantics (D-7) get the most attention here because
//! they are the part that rots silently: a same-day re-run that accumulated
//! findings instead of replacing them would look fine in every list view and be
//! wrong only in the edge set.

use korg_core::repo;
use korg_test_support::{fresh_korg, new};
use serde_json::json;

mod common;
use common::{args, body, server};

/// `create_report` for `(source, date)` with the given findings.
fn report_args(source: &str, date: &str, findings: &[i64]) -> serde_json::Value {
    json!({
        "source": source,
        "report_date": date,
        "status": "ok",
        "summary": format!("{source} on {date}"),
        "body": "# report\n\nbody text",
        "finding_work_items": findings,
    })
}

/// A same-(source, date) re-run keeps the node id and **replaces** the finding
/// edges rather than accumulating them.
///
/// Keeping the node id is what lets comments and relationships on a report
/// survive the day's re-run; replacing the edges is what stops a report that
/// re-ran five times from claiming five copies of every finding.
#[tokio::test]
async fn a_same_day_rerun_keeps_the_node_and_replaces_the_findings() {
    let (_pg, pool) = fresh_korg().await;

    let a = repo::create_work_item(&pool, new::work_item("finding A"))
        .await
        .expect("wi a");
    let b = repo::create_work_item(&pool, new::work_item("finding B"))
        .await
        .expect("wi b");
    let c = repo::create_work_item(&pool, new::work_item("finding C"))
        .await
        .expect("wi c");

    let server = server(pool);

    let first = body(
        &server
            .call(
                "create_report",
                args(report_args(
                    "kmon",
                    "2026-07-11",
                    &[a.wi_number, b.wi_number],
                )),
            )
            .await
            .unwrap(),
    );
    assert_eq!(first["replaced"], false, "the first write replaces nothing");
    assert_eq!(
        first["findings_linked"],
        json!([a.wi_number, b.wi_number]),
        "both findings linked"
    );
    let node_id = first["node_id"].as_i64().expect("node_id");

    // Re-run the same day with a *different* finding set.
    let second = body(
        &server
            .call(
                "create_report",
                args(report_args("kmon", "2026-07-11", &[c.wi_number])),
            )
            .await
            .unwrap(),
    );
    assert_eq!(second["replaced"], true, "the re-run replaced the report");
    assert_eq!(
        second["node_id"], node_id,
        "the node id must be KEPT — comments and relationships hang off it"
    );
    assert_eq!(second["findings_linked"], json!([c.wi_number]));

    // The authority is the edge set, not the response echo.
    let full = body(
        &server
            .call("get_report", args(json!({"node_id": node_id})))
            .await
            .unwrap(),
    );
    let linked: Vec<i64> = full["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .map(|f| f["wi_number"].as_i64().expect("wi_number"))
        .collect();
    assert_eq!(
        linked,
        vec![c.wi_number],
        "findings accumulated instead of being replaced"
    );

    // And exactly one report exists for the day, not two.
    let list = body(
        &server
            .call("list_reports", args(json!({"source": "kmon"})))
            .await
            .unwrap(),
    );
    assert_eq!(
        list.as_array()
            .expect("list_reports returns an array")
            .len(),
        1,
        "the re-run created a second row: {list:?}"
    );
}

/// `findings_linked` echoes only the work-item numbers that resolved. Numbers
/// that do not exist are dropped silently — an agent citing a stale wi_number
/// should still get its report filed.
#[tokio::test]
async fn unresolvable_finding_numbers_are_dropped_from_the_echo() {
    let (_pg, pool) = fresh_korg().await;

    let real = repo::create_work_item(&pool, new::work_item("a real finding"))
        .await
        .expect("wi");
    let server = server(pool);

    let created = body(
        &server
            .call(
                "create_report",
                args(report_args(
                    "kmon",
                    "2026-07-11",
                    &[real.wi_number, 999_999, 999_998],
                )),
            )
            .await
            .unwrap(),
    );

    assert_eq!(
        created["findings_linked"],
        json!([real.wi_number]),
        "findings_linked must echo only what resolved"
    );
}

/// A malformed `report_date` is `invalid_params` — a caller error — not a 500.
///
/// This is the dispatch-side half of sprint 016's serde unit fence: that test
/// proves the format string parses `2026-07-11`; this one proves a bad value
/// surfaces as a protocol error rather than a panic somewhere downstream.
#[tokio::test]
async fn a_malformed_report_date_is_a_caller_error() {
    let (_pg, pool) = fresh_korg().await;
    let server = server(pool);

    for bad in ["11/07/2026", "2026-7-11", "yesterday", ""] {
        let err = server
            .call("create_report", args(report_args("kmon", bad, &[])))
            .await
            .expect_err("a malformed report_date must not succeed");
        assert!(
            err.message.contains("invalid arguments"),
            "`{bad}` produced an unhelpful error: {err:?}"
        );
    }
}

/// `list_reports` filters by source, orders newest-first, and honours `limit`.
#[tokio::test]
async fn list_reports_filters_orders_and_limits() {
    let (_pg, pool) = fresh_korg().await;
    let server = server(pool);

    for (source, date) in [
        ("kmon", "2026-07-09"),
        ("kmon", "2026-07-10"),
        ("kmon", "2026-07-11"),
        ("other", "2026-07-11"),
    ] {
        server
            .call("create_report", args(report_args(source, date, &[])))
            .await
            .unwrap();
    }

    // NOTE: `list_reports` returns a **bare array**, not the
    // `{items, total, limit, offset}` envelope that `list_work_items`,
    // `list_cards`, `list_links` and `list_topics` return. That is what the
    // code does on every surface (MCP, `GET /api/reports`, and `api.ts`'s
    // `ReportRow[]`), so it is what these tests assert — but the MCP server
    // instructions tell agents that *collection reads* return the envelope,
    // full stop, which is not true here (nor for proposals, projects, areas or
    // comments). Recorded in the sprint README; not silently changed under a
    // coverage sweep, because fixing it is a contract change with a UI blast
    // radius, not a test.
    let all = body(&server.call("list_reports", args(json!({}))).await.unwrap());
    assert_eq!(all.as_array().expect("array").len(), 4);

    let mine = body(
        &server
            .call("list_reports", args(json!({"source": "kmon"})))
            .await
            .unwrap(),
    );
    let items = mine.as_array().expect("array");
    assert_eq!(items.len(), 3, "source filter");
    let dates: Vec<&str> = items
        .iter()
        .map(|r| r["report_date"].as_str().expect("report_date"))
        .collect();
    assert_eq!(
        dates,
        vec!["2026-07-11", "2026-07-10", "2026-07-09"],
        "newest first"
    );

    let capped = body(
        &server
            .call("list_reports", args(json!({"source": "kmon", "limit": 2})))
            .await
            .unwrap(),
    );
    assert_eq!(capped.as_array().expect("array").len(), 2, "limit");
}

/// `get_report` returns the body and the linked findings; a missing node id is
/// `not_found`, not an empty success.
#[tokio::test]
async fn get_report_returns_the_body_and_findings_and_404s_honestly() {
    let (_pg, pool) = fresh_korg().await;

    let finding = repo::create_work_item(&pool, new::work_item("the finding"))
        .await
        .expect("wi");
    let server = server(pool);

    let created = body(
        &server
            .call(
                "create_report",
                args(report_args("kmon", "2026-07-11", &[finding.wi_number])),
            )
            .await
            .unwrap(),
    );
    let node_id = created["node_id"].as_i64().expect("node_id");

    let full = body(
        &server
            .call("get_report", args(json!({"node_id": node_id})))
            .await
            .unwrap(),
    );
    assert_eq!(full["source"], "kmon");
    assert_eq!(full["report_date"], "2026-07-11");
    assert!(
        full["body"]
            .as_str()
            .is_some_and(|b| b.contains("body text")),
        "the full read must carry the markdown body: {full:?}"
    );
    let findings = full["findings"].as_array().expect("findings");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["wi_number"], finding.wi_number);
    assert_eq!(
        findings[0]["title"], "the finding",
        "findings carry enough to render without a second round-trip"
    );

    let missing = server
        .call("get_report", args(json!({"node_id": 999_999})))
        .await
        .unwrap();
    assert_eq!(
        missing.is_error,
        Some(true),
        "a missing report must be an error result, not an empty success"
    );
    let text = missing.content[0].as_text().expect("text").text.clone();
    assert!(
        text.contains("not_found"),
        "a missing report must carry the not_found code: {text}"
    );
}
