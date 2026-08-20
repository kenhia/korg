//! Sprint 070 (#1467/#1468, korg:1469) — korg addressable and embeddable.
//!
//! Slice 1 of program korg:1471. This file is the addressable half: every node
//! kind has a page, korg says where it is, and a comment is reachable inside
//! the node it hangs off.
//!
//! The gap these tests close was invisible on the board because two consumers
//! were *correctly* routing around it — kfdc #993's Net Log degrades to plain
//! text rather than fake a URL, and korg #981's Awaiting lane linked only the
//! kinds that had one. Nothing was broken; several things were quietly smaller
//! than they should be. So the assertions here are about coverage and about
//! korg being the one that answers, not about a failing behaviour.

use korg_core::repo::{
    add_comment, create_attachment, create_card, create_handoff, create_link, create_program,
    create_proposal, create_schedule, create_work_item, get_node_preview, node_path, node_route,
    search, upsert_report, SearchQuery,
};
use korg_core::vocab::NODE_KINDS;
use korg_test_support::{fresh_korg, new, test_project};
use sqlx::PgPool;
use std::collections::BTreeMap;

/// One node of every kind in [`NODE_KINDS`], keyed by kind.
///
/// Built by creating them all rather than by inserting `node` rows directly: a
/// route is only worth anything if the *real* creation path produces a node the
/// route resolves, and a hand-inserted row would pass this suite while the
/// feature stayed broken for everything a person can actually make.
async fn one_of_each(pool: &PgPool) -> BTreeMap<&'static str, i64> {
    let mut ids = BTreeMap::new();

    let wi = create_work_item(pool, new::work_item("routable"))
        .await
        .unwrap()
        .node_id;
    ids.insert("workitem", wi);
    ids.insert(
        "card",
        create_card(pool, new::card("routable card"))
            .await
            .unwrap()
            .node_id,
    );
    ids.insert(
        "link",
        create_link(pool, new::link("https://example.invalid/routable"))
            .await
            .unwrap()
            .node_id,
    );
    ids.insert(
        "sprint_proposal",
        create_proposal(pool, new::proposal("routable proposal"))
            .await
            .unwrap()
            .row
            .node_id,
    );
    ids.insert(
        "report",
        upsert_report(
            pool,
            new::report("kfo", time::macros::date!(2026 - 08 - 19)),
        )
        .await
        .unwrap()
        .node_id,
    );
    // Hung off the work item rather than created standalone: a handoff must
    // belong to something (that is the rule `allow_standalone` opts out of), and
    // an owned one is the shape a route actually has to serve.
    ids.insert(
        "handoff",
        create_handoff(
            pool,
            korg_core::repo::NewHandoff {
                related_node_ids: vec![wi],
                ..new::handoff("routable handoff")
            },
        )
        .await
        .unwrap()
        .handoff
        .node_id,
    );
    ids.insert(
        "program",
        create_program(pool, new::program("routable program"))
            .await
            .unwrap()
            .row
            .node_id,
    );
    ids.insert(
        "schedule",
        create_schedule(pool, new::schedule("routable schedule", "monthly", None))
            .await
            .unwrap()
            .node_id,
    );
    ids.insert(
        "attachment",
        create_attachment(pool, new::attachment("routable.png"))
            .await
            .unwrap()
            .node_id,
    );

    assert_eq!(
        ids.len(),
        NODE_KINDS.len(),
        "this fixture must build one node of every kind — a new kind needs a \
         line here before the coverage assertions below mean anything"
    );
    ids
}

/// **The headline claim of #1467**: every node kind korg holds is reachable at
/// a URL, resolved from nothing but its id.
///
/// `node_route` is the server half of `/n/:node_id`, which is the call a
/// consumer holding a locator cannot make for itself — `korg:1395` and `WI-836`
/// carry an id and no kind, so nothing outside korg can choose between
/// `/planning/1395` and `/work-items/1395`.
#[tokio::test]
async fn every_node_kind_resolves_to_a_page() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let ids = one_of_each(&pool).await;

    for (kind, node_id) in &ids {
        let route = node_route(&pool, *node_id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{kind} #{node_id} resolved to no page"));
        assert_eq!(
            Some(route.clone()),
            node_path(kind, *node_id),
            "{kind}'s resolved route must be the one NODE_ROUTES declares — a \
             second derivation is the drift the table exists to prevent"
        );
        assert!(route.ends_with(&format!("/{node_id}")), "{kind} → {route}");
    }
}

/// An id with no node is `None`, not a path into nowhere. `/n/:node_id` turns
/// this into a 404, which is the honest answer to a stale locator — a redirect
/// to a page that will itself say "no such node" would spend a round trip to
/// arrive at the same place with a worse URL in the bar.
#[tokio::test]
async fn an_unknown_id_resolves_to_nothing() {
    let (_pg, pool) = fresh_korg().await;
    assert_eq!(node_route(&pool, 999_999).await.unwrap(), None);
}

/// korg hands the URL over on the payload a consumer already reads (GP-13).
///
/// `get_node_preview` is the one read that answers "what is this node" without
/// the caller knowing its kind, so it is exactly the read that should also
/// answer "where does it live".
#[tokio::test]
async fn the_node_preview_carries_its_own_url() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let ids = one_of_each(&pool).await;

    for (kind, node_id) in &ids {
        let preview = get_node_preview(&pool, *node_id).await.unwrap().unwrap();
        assert_eq!(
            preview.url,
            node_path(kind, *node_id),
            "the preview of a {kind} must carry the same URL NODE_ROUTES gives it"
        );
    }
}

/// A search hit says where to open it. Without this a consumer holding a hit
/// has `kind` and `node_id` and must own a kind → path table to turn them into
/// a link — a copy of a vocabulary korg grows between deploys (GP-14).
#[tokio::test]
async fn search_hits_carry_their_url() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;
    let ids = one_of_each(&pool).await;

    let hits = search(
        &pool,
        SearchQuery {
            q: "routable".into(),
            scope: Some("all".into()),
            archived: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!hits.items.is_empty(), "the fixture should be findable");

    for hit in &hits.items {
        let url = hit
            .url
            .as_deref()
            .unwrap_or_else(|| panic!("{} carried no url", hit.locator));
        assert!(url.starts_with('/'), "{} → {url}", hit.locator);
        if let Some((kind, _)) = ids.iter().find(|(_, id)| **id == hit.node_id) {
            assert_eq!(
                Some(url.to_string()),
                node_path(kind, hit.node_id),
                "the hit for {} must agree with NODE_ROUTES",
                hit.locator
            );
        }
    }
}

/// A comment hit lands **on the comment**, not merely on the node that holds it.
///
/// This is the half a consumer provably cannot do. A comment hit's `kind` is
/// `comment` and its `node_id` is the owning node — whose kind the response
/// never carried — so `korg:1469#comment-N` is a locator korg emits and, until
/// now, nothing could turn into a URL. Comments are also where the answer
/// usually is (#1177's own measurement), so routing to the top of a long
/// proposal is routing to the wrong place.
#[tokio::test]
async fn a_comment_hit_routes_to_its_anchor() {
    let (_pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let proposal = create_proposal(&pool, new::proposal("anchor host"))
        .await
        .unwrap()
        .row
        .node_id;
    let comment = add_comment(&pool, proposal, "zamboni maintenance is overdue")
        .await
        .unwrap();

    let hits = search(
        &pool,
        SearchQuery {
            q: "zamboni".into(),
            scope: Some("all".into()),
            archived: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let hit = hits
        .items
        .iter()
        .find(|h| h.comment_id == Some(comment.id))
        .expect("the comment should be findable by its own body");
    assert_eq!(hit.kind, "comment");
    assert_eq!(
        hit.url.as_deref(),
        Some(format!("/planning/{proposal}#comment-{}", comment.id).as_str()),
        "a comment's URL is its owner's page plus its own anchor — the locator \
         korg already prints, made clickable"
    );
    assert_eq!(
        hit.locator,
        format!("korg:{proposal}#comment-{}", comment.id),
        "and the URL must be the locator's spelling, not a second convention"
    );
}
