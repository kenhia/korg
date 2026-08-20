//! Sprint 070 (#1467/#1468, korg:1469) — the HTTP half of korg being
//! addressable and embeddable.
//!
//! Two surfaces an operator and a browser meet rather than an agent:
//! `/n/:node_id`, the redirect a consumer holding only a locator follows, and
//! the `frame-ancestors` policy that decides whether kfdc's pane may paint
//! korg at all.

use axum::http::StatusCode;
use serde_json::json;

mod common;
use common::{app, app_configured, raw, req};

const KFDC: &str = "https://kubsdb.encke-wahoo.ts.net:8100";

async fn get(router: &axum::Router, path: &str) -> (StatusCode, axum::http::HeaderMap) {
    let (status, headers, _) = raw(router, "GET", path, None, Vec::new()).await;
    (status, headers)
}

fn header(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .map(|v| v.to_str().expect("header is text").to_owned())
}

// --- #1467: /n/:node_id ------------------------------------------------------

/// The resolver sends a locator to the node's own page.
///
/// A 307 rather than a 200 with a rendered node: the address bar ends on the
/// canonical per-kind URL, so the link a person copies *after* following one of
/// these is the real one, and korg grows no second renderer to argue with
/// #621's per-kind decision.
#[tokio::test]
async fn n_redirects_to_the_nodes_own_page() {
    let (_c, router) = app().await;

    let (st, wi) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title": "addressable", "content": "x"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let wi_number = wi["wi_number"].as_i64().unwrap();

    let (st, headers) = get(&router, &format!("/n/{wi_number}")).await;
    assert_eq!(st, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        header(&headers, "location").as_deref(),
        Some(format!("/work-items/{wi_number}").as_str())
    );

    // A different kind resolves to a different page off the same route — the
    // whole reason this endpoint exists rather than a consumer-side rule.
    let (st, proposal) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({
            "title": "addressable proposal",
            "summary": "s",
            "project": common::PROJECT,
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let node_id = proposal["node_id"].as_i64().unwrap();

    let (st, headers) = get(&router, &format!("/n/{node_id}")).await;
    assert_eq!(st, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        header(&headers, "location").as_deref(),
        Some(format!("/planning/{node_id}").as_str())
    );
}

/// A stale locator gets a 404, not a redirect into a page that will say the
/// same thing one round trip later.
#[tokio::test]
async fn n_404s_on_an_unknown_id() {
    let (_c, router) = app().await;
    let (st, _) = get(&router, "/n/999999").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

/// `/n/:node_id` must win over the SPA fallback and must not shadow `/api`.
///
/// The router mounts the bundle as a `fallback_service`, so a route that failed
/// to register would not 404 — it would quietly serve the app shell with a 200,
/// and a consumer following the redirect would get an HTML page instead of a
/// `Location`. That is the failure mode worth a test.
#[tokio::test]
async fn n_is_a_real_route_not_the_spa_fallback() {
    let (_c, router) = app().await;
    let (st, headers) = get(&router, "/n/999999").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_ne!(
        header(&headers, "content-type").as_deref(),
        Some("text/html"),
        "/n/:node_id answered with a document — the route is not registered and \
         the fallback took the request"
    );
}

// --- #1468: frame-ancestors --------------------------------------------------

/// **Default closed.** An unconfigured korg permits no embedding at all.
///
/// `'none'` is served rather than the header being omitted, because an absent
/// `frame-ancestors` means *anyone* may frame korg — silence and "nobody" are
/// opposite answers here, so the default has to be spelled out.
#[tokio::test]
async fn embedding_is_refused_by_default() {
    let (_c, router) = app().await;
    let (_st, headers) = get(&router, "/api/health").await;
    assert_eq!(
        header(&headers, "content-security-policy").as_deref(),
        Some("frame-ancestors 'none'")
    );
}

/// A configured origin appears in the policy. kfdc's `:8100` is the first
/// entry; korg and kfdc share a host since kfdc sprint 009 but not a port, so
/// they are different origins and the header is what closes the gap.
#[tokio::test]
async fn a_configured_origin_may_embed() {
    let (_c, _pool, router) = app_configured(|c| c.with_frame_ancestors(KFDC)).await;
    let (_st, headers) = get(&router, "/api/health").await;
    assert_eq!(
        header(&headers, "content-security-policy").as_deref(),
        Some(format!("frame-ancestors {KFDC}").as_str())
    );
}

/// It is a list, not a constant (#1468's own instruction): kfdc is the first
/// embedder and korg-dash, kdeskdash and korg-vs's webview are each an entry
/// rather than a rebuild. Comma-separated in, space-separated out — the
/// operator's spelling matches `KORG_CORS_ORIGINS` beside it, and the header's
/// spelling is CSP's.
#[tokio::test]
async fn the_allowlist_takes_several_origins() {
    let (_c, _pool, router) =
        app_configured(|c| c.with_frame_ancestors(&format!("{KFDC} , https://kai:5674 ,"))).await;
    let (_st, headers) = get(&router, "/api/health").await;
    assert_eq!(
        header(&headers, "content-security-policy").as_deref(),
        Some(format!("frame-ancestors {KFDC} https://kai:5674").as_str()),
        "entries are trimmed and blanks dropped — a trailing comma in a .env \
         line must not become an empty CSP source"
    );
}

/// The policy rides **every** response, not just the app shell.
///
/// The pane loads a document and everything that document pulls in; a policy
/// covering only the first is a policy with a hole in it. Asserted across an
/// API read, a 404 and a redirect because the layer sits outside the router and
/// an error path that skipped it would be invisible until a browser refused a
/// frame nobody could explain.
#[tokio::test]
async fn the_policy_rides_every_response() {
    let (_c, _pool, router) = app_configured(|c| c.with_frame_ancestors(KFDC)).await;
    for path in ["/api/health", "/api/work-items", "/n/999999", "/n/1"] {
        let (_st, headers) = get(&router, path).await;
        assert_eq!(
            header(&headers, "content-security-policy").as_deref(),
            Some(format!("frame-ancestors {KFDC}").as_str()),
            "{path} answered without the embed policy"
        );
    }
}

/// korg sends **no** `X-Frame-Options`.
///
/// The old header cannot express an allowlist — it has `DENY`, `SAMEORIGIN` and
/// a deprecated `ALLOW-FROM` — and where the two disagree browsers honour the
/// stricter. So an `X-Frame-Options: SAMEORIGIN` added later, for the best of
/// reasons, would silently undo the allowlist and leave kfdc's pane blank with
/// a correct-looking CSP header in the response.
#[tokio::test]
async fn no_x_frame_options_header() {
    let (_c, _pool, router) = app_configured(|c| c.with_frame_ancestors(KFDC)).await;
    for path in ["/api/health", "/n/999999"] {
        let (_st, headers) = get(&router, path).await;
        assert_eq!(
            header(&headers, "x-frame-options"),
            None,
            "{path} set X-Frame-Options, which overrides frame-ancestors where \
             the two disagree"
        );
    }
}

/// Embedding is not authentication. The allowlist decides who may *paint* korg;
/// it changes nothing about who may read or write it.
///
/// Asserted rather than only documented, because the plausible mistake is the
/// other direction: somebody reads the allowlist as an access-control list and
/// later "tightens" korg by trusting it. A default-closed korg still answers
/// every API call it answered before.
#[tokio::test]
async fn the_allowlist_grants_no_access() {
    let (_c, router) = app().await;
    let (st, _) = req(
        &router,
        "POST",
        "/api/work-items",
        Some(json!({"title": "written with embedding refused", "content": "x"})),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "a korg that permits no embedding must still serve its API — the CSP \
         header is about framing, never about permission"
    );
}
