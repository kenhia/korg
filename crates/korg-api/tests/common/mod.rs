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
use korg_test_support::{fresh_korg, test_project};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use time::macros::datetime;
use tower::ServiceExt;

/// A router over a fresh korg database, with the clock pinned so date-dependent
/// endpoints (reports) assert against a fixed "today".
pub async fn app() -> (impl Sized, axum::Router) {
    let (pg, _pool, router) = app_with_pool().await;
    (pg, router)
}

/// The name every suite here files a proposal under. Re-exported from
/// `korg-test-support` so a JSON payload can spell it without a second import.
pub const PROJECT: &str = korg_test_support::TEST_PROJECT;

/// The same, plus the pool — for suites that must seed something REST has no
/// write route for (reports are MCP-only).
///
/// Seeds [`PROJECT`]: sprint 043 made a project mandatory on every proposal,
/// and a REST suite that has to POST `/api/projects` first before it can test
/// anything else is testing the wrong thing.
pub async fn app_with_pool() -> (impl Sized, PgPool, axum::Router) {
    let (guard, pool, router, _root) = app_with_images().await;
    (guard, pool, router)
}

/// A throwaway image-store root that removes itself when the suite drops it.
///
/// Bound into the harness guard rather than left to the caller: an image test
/// that forgot to clean up would leave decoded screenshots in `/tmp` on every
/// run, and the failure would be invisible until a disk filled.
pub struct ImageRoot(pub std::path::PathBuf);

impl Drop for ImageRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The full harness, including where the image store put its blobs — so a test
/// can assert against the *disk*, not only against what korg says about it.
/// That distinction is the whole point of the store being a separate crate.
pub async fn app_with_images() -> (impl Sized, PgPool, axum::Router, std::path::PathBuf) {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let (pg, pool) = fresh_korg().await;
    test_project(&pool).await;

    let root = std::env::temp_dir().join(format!(
        "korg-api-img-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let images = korg_img::Store::new(&root);
    images.ensure_root().expect("create test image store");

    let router = build_router(AppState {
        pool: Arc::new(pool.clone()),
        config: Arc::new(
            korg_core::config::KorgConfig::fixed("UTC", datetime!(2026-07-11 12:00 UTC)).unwrap(),
        ),
        images: Arc::new(images),
    });
    ((pg, ImageRoot(root.clone())), pool, router, root)
}

/// A `multipart/form-data` body with one file part — what a `curl -F` upload
/// and a browser's `FormData` both send.
pub fn multipart(field: &str, filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    const BOUNDARY: &str = "korgtestboundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{field}\"; \
             filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

/// Issue a request with a raw body and content type, returning the raw
/// response. Images are not JSON in either direction, so the JSON-shaped
/// helpers below cannot serve them.
pub async fn raw(
    router: &axum::Router,
    method: &str,
    path: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let resp = router
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .expect("request");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, headers, bytes.to_vec())
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
