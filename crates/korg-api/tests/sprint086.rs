//! Sprint 086 — `GET /api/connectors/projects`, the v1 project listing.
//!
//! WI 2874, slice 1 of program korg:2916. kctrldeck's Code tab consumes this
//! to render every live korg project as a launchable row, so the assertions
//! here are written from the consumer's side: can it open the folder without
//! knowing anything about korg?
//!
//! The unit-level path conversions live beside the code in
//! `korg-api/src/connectors.rs`. What this suite adds is the part only a real
//! dispatched response can prove — the envelope, the filters, and the fact
//! that a cleo project comes out as a Windows path at all, which is the whole
//! reason slice 1 exists.

use axum::http::StatusCode;
use serde_json::Value;
use sqlx::PgPool;

mod common;
use common::{app_with_pool, req};

const PATH: &str = "/api/connectors/projects";

async fn project(pool: &PgPool, name: &str, src_path: Option<&str>, machines: &[&str]) {
    sqlx::query(
        "INSERT INTO project (name, status, src_path, machines) VALUES ($1, 'active', $2, $3)",
    )
    .bind(name)
    .bind(src_path)
    .bind(machines.iter().map(|m| m.to_string()).collect::<Vec<_>>())
    .execute(pool)
    .await
    .unwrap_or_else(|e| panic!("seed {name}: {e}"));
}

fn row<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["projects"]
        .as_array()
        .expect("projects is a list")
        .iter()
        .find(|p| p["name"] == name)
        .unwrap_or_else(|| panic!("{name} is not in the listing: {body}"))
}

/// The envelope a consumer branches on before it reads a single project.
#[tokio::test]
async fn the_listing_names_its_protocol_and_version() {
    let (_c, _pool, router) = app_with_pool().await;
    let (status, body) = req(&router, "GET", PATH, None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["connector"], "korg",
        "the shape is source-neutral — a consumer needs to be told which source answered"
    );
    assert_eq!(
        body["version"], 1,
        "the version is the contract mechanism for this endpoint; within it, changes \
         are additive only"
    );
    assert_eq!(
        body["generated"], "2026-07-11T12:00:00Z",
        "`generated` must come from korg's configured clock, not the wall clock — a \
         consumer displays this as the age of its last-good copy, and a surface the \
         pinned clock cannot reach is a surface no test can assert on"
    );
    assert!(body["projects"].is_array());
}

/// A POSIX project: `~/` is expanded, because the consumer is not required to
/// know korg's storage convention or which user the fleet runs as.
#[tokio::test]
async fn a_posix_project_is_emitted_as_an_absolute_native_path() {
    let (_c, pool, router) = app_with_pool().await;
    // Not `korg` itself: the harness seeds that name as its default project.
    project(&pool, "klams", Some("~/src/ai/klams"), &["kai"]).await;

    let (_, body) = req(&router, "GET", PATH, None).await;
    let locations = row(&body, "klams")["locations"]
        .as_array()
        .expect("locations is a list");

    assert_eq!(locations.len(), 1, "korg v1 emits exactly one location");
    assert_eq!(locations[0]["host"], "kai");
    assert_eq!(
        locations[0]["path"], "/home/ken/src/ai/klams",
        "absolute: a consumer cannot expand `~/` without knowing the host's user"
    );
    assert_eq!(locations[0]["kind"], "posix");
}

/// The case slice 1 exists for. A cleo clone is stored as a drive root and
/// handed to the consumer in the spelling Windows actually takes.
#[tokio::test]
async fn a_cleo_project_is_emitted_as_a_native_windows_path() {
    let (_c, pool, router) = app_with_pool().await;
    project(
        &pool,
        "kctrldeck",
        Some("/d/ClaudeWorks/kctrldeck"),
        &["cleo"],
    )
    .await;

    let (_, body) = req(&router, "GET", PATH, None).await;
    let location = &row(&body, "kctrldeck")["locations"][0];

    assert_eq!(location["host"], "cleo");
    assert_eq!(
        location["path"], r"D:\ClaudeWorks\kctrldeck",
        "the stored drive-root form is korg's; the consumer gets the host's own spelling"
    );
    assert_eq!(
        location["kind"], "windows",
        "a consumer builds a `file:///d%3A/...` URI from this and cannot reliably \
         infer the form from the string"
    );
}

/// `machines[0]` is the host, because `src_path` is singular and documented
/// (#675) as the working copy on the DEVELOPMENT machine.
#[tokio::test]
async fn a_multi_machine_project_is_located_on_its_development_machine() {
    let (_c, pool, router) = app_with_pool().await;
    project(
        &pool,
        "agent-skills",
        Some("~/src/ai-agents/agent-skills"),
        &["kubs0", "cleo"],
    )
    .await;

    let (_, body) = req(&router, "GET", PATH, None).await;
    let locations = row(&body, "agent-skills")["locations"]
        .as_array()
        .expect("locations is a list");

    assert_eq!(locations.len(), 1);
    assert_eq!(
        locations[0]["host"], "kubs0",
        "one path, several hosts that might hold it — korg only knows the first is \
         what `src_path` describes"
    );
    assert_eq!(locations[0]["path"], "/home/ken/src/ai-agents/agent-skills");
}

/// What the listing leaves out, and why each omission is silent rather than an
/// error: a project korg cannot locate is one whose metadata nobody filled in,
/// and failing the listing over it would take the panel down for a blank field.
#[tokio::test]
async fn projects_korg_cannot_locate_are_omitted() {
    let (_c, pool, router) = app_with_pool().await;
    project(&pool, "has-both", Some("~/src/has-both"), &["kai"]).await;
    project(&pool, "no-path", None, &["kai"]).await;
    project(&pool, "no-machine", Some("~/src/no-machine"), &[]).await;

    let (_, body) = req(&router, "GET", PATH, None).await;
    let names: Vec<&str> = body["projects"]
        .as_array()
        .expect("projects is a list")
        .iter()
        .map(|p| p["name"].as_str().expect("name"))
        .collect();

    assert!(names.contains(&"has-both"));
    assert!(
        !names.contains(&"no-path"),
        "no `src_path` means there is no path to emit"
    );
    assert!(
        !names.contains(&"no-machine"),
        "no `machines` means there is no host to attach a path to"
    );
}

/// Archiving in korg is what takes a project off the panel — the whole point
/// of the automation, and the thing a hand-kept favorites list cannot do.
#[tokio::test]
async fn an_archived_project_leaves_the_listing() {
    let (_c, pool, router) = app_with_pool().await;
    project(&pool, "retired", Some("~/src/retired"), &["kai"]).await;

    let (_, before) = req(&router, "GET", PATH, None).await;
    assert_eq!(row(&before, "retired")["locations"][0]["host"], "kai");

    sqlx::query("UPDATE project SET status = 'archived' WHERE name = 'retired'")
        .execute(&pool)
        .await
        .expect("archive");

    let (_, after) = req(&router, "GET", PATH, None).await;
    let names: Vec<&str> = after["projects"]
        .as_array()
        .expect("projects is a list")
        .iter()
        .map(|p| p["name"].as_str().expect("name"))
        .collect();
    assert!(
        !names.contains(&"retired"),
        "a project archived in korg must disappear on the next refresh, with nothing \
         hand-kept: {after}"
    );
}

/// `starred` and `category` ride along so a consumer can sort and group
/// without a second call — and `category` is OMITTED, not null, when korg does
/// not know it, so a grouping consumer has no `null` bucket.
#[tokio::test]
async fn sorting_and_grouping_need_no_second_call() {
    let (_c, pool, router) = app_with_pool().await;
    project(&pool, "hot", Some("~/src/hot"), &["kai"]).await;
    sqlx::query("UPDATE project SET starred = true, category = 'Tools' WHERE name = 'hot'")
        .execute(&pool)
        .await
        .expect("star and categorise");
    project(&pool, "plain", Some("~/src/plain"), &["kai"]).await;

    let (_, body) = req(&router, "GET", PATH, None).await;

    let hot = row(&body, "hot");
    assert_eq!(hot["starred"], true);
    assert_eq!(hot["category"], "Tools");

    let plain = row(&body, "plain");
    assert_eq!(plain["starred"], false);
    assert!(
        plain.get("category").is_none(),
        "an uncategorised project must omit the key rather than send null — a consumer \
         grouping by category wants no group, not a group called `null`: {plain}"
    );
}
