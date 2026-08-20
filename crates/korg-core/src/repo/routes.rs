//! Resolving a node id to the page that renders it (WI #1467, sprint 070).
//!
//! The table itself lives in [`crate::vocab::NODE_ROUTES`], beside the node
//! kinds it must cover; this module is the database half — the one lookup a
//! caller holding nothing but an id cannot do for itself.
//!
//! That is the whole shape of `/n/:node_id`. A consumer with a locator
//! (`korg:1395`, `WI-836`) has an id and no kind, so it cannot pick a route;
//! korg has both, so it answers with a redirect and the consumer keeps no
//! kind → path table of its own (GP-13).

use anyhow::Result;
use sqlx::PgPool;

use crate::vocab::node_path;

/// The canonical path for the node with this id, or `None` when no node has it.
///
/// Two different "no" answers deliberately collapse into one here: an id with
/// no node, and a node whose kind has no route. The second is unreachable —
/// `every_node_kind_has_a_route` fences it — and a caller could do nothing
/// different with the distinction anyway, since both mean "there is no page to
/// send you to".
pub async fn node_route(pool: &PgPool, node_id: i64) -> Result<Option<String>> {
    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM node WHERE id = $1")
        .bind(node_id)
        .fetch_optional(pool)
        .await?;
    Ok(kind.and_then(|k| node_path(&k, node_id)))
}
