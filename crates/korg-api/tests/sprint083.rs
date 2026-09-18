//! Sprint 083 — the published read-shape contract is reachable over HTTP.
//!
//! `korg-mcp`'s `read_shapes` suite proves the committed document describes what
//! the reads return. That is the half that keeps the file honest. This is the
//! half that gets it to a consumer: kmon and kyac do not clone korg, so a
//! committed file they cannot fetch would close nothing (WI 2041).

use axum::http::StatusCode;

mod common;
use common::{app, raw};

const PATH: &str = "/api/contract/read-shapes";

fn committed() -> String {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contract/read-shapes.json");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Byte-identical, not merely equivalent JSON.
///
/// A consumer is entitled to hash what it fetched and compare it with what it
/// vendored — that is the cheapest possible staleness check, and it is only
/// available if the endpoint serves the file rather than a re-serialisation of
/// it. Re-parsing to a `Value` on the way out would reorder keys and break that
/// for no gain.
#[tokio::test]
async fn the_endpoint_serves_the_committed_contract_byte_for_byte() {
    let (_c, router) = app().await;
    let (status, headers, bytes) = raw(&router, "GET", PATH, None, Vec::new()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default(),
        "application/json",
        "a consumer's HTTP client decides whether to parse on this header"
    );
    assert_eq!(
        String::from_utf8(bytes).expect("utf-8"),
        committed(),
        "the endpoint and the committed contract have diverged — since the endpoint embeds \
         the file at build time, this means the running binary was built from a different \
         tree than the one under test"
    );
}

/// The endpoint needs no database.
///
/// Worth asserting rather than assuming: a consumer's CI wants this document
/// when korg is up but may be checked against a korg whose pool is unhealthy,
/// and the contract is a property of the *build*, not of the data. The route
/// takes no `State`, so nothing here can start depending on one without this
/// failing to compile.
#[tokio::test]
async fn the_contract_is_readable_and_names_the_reads_it_describes() {
    let (_c, router) = app().await;
    let (status, _headers, bytes) = raw(&router, "GET", PATH, None, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);

    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("served body is json");
    let reads = doc["reads"].as_array().expect("a `reads` array");
    assert!(
        reads.len() >= 15,
        "korg has 15 collection reads; the served contract describes {}",
        reads.len()
    );

    // The two facts the item was filed about, asserted through the transport a
    // consumer actually uses rather than only in korg's own suite.
    let work_items = reads
        .iter()
        .find(|r| r["read"] == "list_work_items")
        .expect("list_work_items is described");
    assert_eq!(
        work_items["shape"], "paginated",
        "the envelope form that crashed kmon for 11 days"
    );
    assert!(
        work_items["only_on_full_read"]
            .as_array()
            .expect("only_on_full_read")
            .iter()
            .any(|k| k == "tags"),
        "the lean-row `tags` absence that silently emptied kyac's sink for two months"
    );
}
