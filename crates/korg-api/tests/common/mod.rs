//! Scaffolding shared by the korg-api integration suites.
//!
//! The database half comes from `korg-test-support`; what lives here is the
//! part that is specific to *this* crate's surface — building the router and
//! driving it over Tower — which cannot move into the shared crate without
//! making it depend on the crate it is a dev-dependency of.

#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use korg_api::{build_router, AppState};
use korg_test_support::fresh_korg;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use time::macros::datetime;
use tower::ServiceExt;

/// A router over a fresh korg database, with the clock pinned so date-dependent
/// endpoints (daily plan, reports) assert against a fixed "today".
pub async fn app() -> (impl Sized, axum::Router) {
    let (pg, _pool, router) = app_with_pool().await;
    (pg, router)
}

/// The same, plus the pool — for suites that must seed something REST has no
/// write route for (reports are MCP-only).
pub async fn app_with_pool() -> (impl Sized, PgPool, axum::Router) {
    let (pg, pool) = fresh_korg().await;
    let router = build_router(AppState {
        pool: Arc::new(pool.clone()),
        config: Arc::new(
            korg_core::config::KorgConfig::fixed("UTC", datetime!(2026-07-11 12:00 UTC)).unwrap(),
        ),
    });
    (pg, pool, router)
}

/// Issue one request and return `(status, parsed body)`. An empty body parses
/// as `Value::Null` rather than panicking, so 204s are assertable.
pub async fn req(
    router: &axum::Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&v).unwrap())
        }
        None => Body::empty(),
    };
    let resp = router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .expect("request");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json)
}
