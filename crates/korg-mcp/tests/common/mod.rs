//! Scaffolding shared by the korg-mcp integration suites.
//!
//! The database half comes from `korg-test-support`; the MCP half lives here,
//! because a shared crate that knew about `KorgServer` would have to depend on
//! the crate it is a dev-dependency of.
//!
//! `server.rs` and `dispatch.rs` must call tools through the *same* wrapper —
//! the whole value of the completeness fence in `dispatch.rs` is that "the
//! dispatch test exercised it" means the same thing as "a behavioural test
//! exercised it".

#![allow(dead_code)]

use korg_mcp::tools::KorgServer;
use rmcp::model::{CallToolResult, JsonObject};
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;
use time::macros::datetime;

/// A server over `pool`, with the clock pinned so date-dependent tools
/// (daily plan, reports) assert against a fixed "today".
pub fn server(pool: PgPool) -> KorgServer {
    KorgServer::new(
        pool,
        Arc::new(
            korg_core::config::KorgConfig::fixed("UTC", datetime!(2026-07-11 12:00 UTC)).unwrap(),
        ),
    )
}

/// Tool arguments, as MCP delivers them: a JSON object, not a bare value.
pub fn args(v: Value) -> Option<JsonObject> {
    match v {
        Value::Object(m) => Some(m),
        _ => panic!("args must be object"),
    }
}

/// The JSON body of a successful tool result. Panics with the whole result if
/// the tool errored, so a failure names what went wrong rather than an index
/// out of bounds.
pub fn body(result: &CallToolResult) -> Value {
    assert_ne!(
        result.is_error,
        Some(true),
        "tool returned an error: {result:?}"
    );
    let text = result.content[0]
        .as_text()
        .expect("text content")
        .text
        .clone();
    serde_json::from_str(&text).expect("result body is json")
}

/// The error message of a tool result that *should* have failed. The inverse of
/// [`body`], for the error-path assertions.
pub fn error_text(result: &CallToolResult) -> String {
    assert_eq!(
        result.is_error,
        Some(true),
        "tool unexpectedly succeeded: {result:?}"
    );
    result.content[0]
        .as_text()
        .expect("text content")
        .text
        .clone()
}
