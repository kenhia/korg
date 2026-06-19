//! M4 acceptance — korg-mcp server dispatch over a real korg database.
//!
//! Builds the server against a fresh testcontainers korg DB and exercises the
//! tool surface end-to-end: tool listing, work-item/link creation, generalized
//! relate + neighbors (cross-kind), and the seeded slot template.

use korg_mcp::tools::{tools, KorgServer};
use rmcp::model::{CallToolResult, JsonObject};
use serde_json::{json, Value};
use sqlx::PgPool;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;

async fn fresh_korg() -> (impl Sized, PgPool) {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("port");
    let url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = korg_core::connect(&url).await.expect("connect+migrate");
    (container, pool)
}

fn args(v: Value) -> Option<JsonObject> {
    match v {
        Value::Object(m) => Some(m),
        _ => panic!("args must be object"),
    }
}

/// Extract the JSON body of a successful tool result.
fn body(result: &CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "tool returned an error: {result:?}");
    let text = result.content[0].as_text().expect("text content").text.clone();
    serde_json::from_str(&text).expect("result body is json")
}

#[tokio::test]
async fn mcp_surface_end_to_end() {
    let (_c, pool) = fresh_korg().await;
    let server = KorgServer::new(pool);

    // Tool descriptors are stable.
    assert_eq!(tools().len(), 16, "expected 16 tools");

    // Create a work item.
    let wi = body(
        &server
            .call(
                "create_work_item",
                args(json!({"title":"Ship korg-mcp","content":"wire tools","wi_tshirt":"M","tags":["mcp"]})),
            )
            .await
            .expect("create_work_item"),
    );
    let wi_node = wi["node_id"].as_i64().unwrap();
    assert_eq!(wi["wi_number"].as_i64(), Some(1));

    // List shows it.
    let items = body(&server.call("list_work_items", args(json!({}))).await.unwrap());
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["title"], "Ship korg-mcp");

    // Capture a reading-list URL.
    let link = body(
        &server
            .call(
                "create_link",
                args(json!({"url":"https://modelcontextprotocol.io","title":"MCP"})),
            )
            .await
            .unwrap(),
    );
    let link_node = link["node_id"].as_i64().unwrap();

    // Cross-kind relationship work item <-> link.
    server
        .call(
            "relate",
            args(json!({"left":wi_node,"right":link_node,"label":"references"})),
        )
        .await
        .unwrap();

    let ns = body(&server.call("neighbors", args(json!({"node_id":wi_node}))).await.unwrap());
    let ns = ns.as_array().unwrap();
    assert_eq!(ns.len(), 1);
    assert_eq!(ns[0]["node_id"].as_i64(), Some(link_node));
    assert_eq!(ns[0]["kind"], "link");
    assert_eq!(ns[0]["label"], "references");

    // Reading list reflects the link.
    let links = body(&server.call("list_links", args(json!({}))).await.unwrap());
    assert_eq!(links.as_array().unwrap().len(), 1);

    // Seeded weekly template (16 rows) is visible.
    let tmpl = body(&server.call("list_slot_templates", args(json!({}))).await.unwrap());
    assert_eq!(tmpl.as_array().unwrap().len(), 16);

    // Generate a week of slots and read them back.
    let gen = body(
        &server
            .call("generate_slots", args(json!({"start":"2024-01-01","days":7})))
            .await
            .unwrap(),
    );
    assert_eq!(gen["created"].as_i64(), Some(16));
    let week = body(
        &server
            .call("list_slots", args(json!({"from":"2024-01-01","to":"2024-01-07"})))
            .await
            .unwrap(),
    );
    assert_eq!(week.as_array().unwrap().len(), 16);

    // Unknown tool is a clean error.
    assert!(server.call("nope", args(json!({}))).await.is_err());
}
