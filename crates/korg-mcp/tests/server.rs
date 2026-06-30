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
    assert_eq!(tools().len(), 25, "expected 25 tools");

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

/// WI #92 — agents must be able to update a work item over MCP: set status
/// (resolve), edit fields, clear a nullable field, and archive.
#[tokio::test]
async fn update_work_item_partial_and_nullable() {
    let (_c, pool) = fresh_korg().await;
    let server = KorgServer::new(pool);

    // Create with a details value and an "open" status.
    let wi = body(
        &server
            .call(
                "create_work_item",
                args(json!({"title":"Explore trt-llm","content":"investigate","details":"notes here","wi_status":"open","wi_tshirt":"S"})),
            )
            .await
            .unwrap(),
    );
    let n = wi["wi_number"].as_i64().unwrap();

    // Resolve it and edit the title in one partial update; omit everything else.
    let res = body(
        &server
            .call(
                "update_work_item",
                args(json!({"wi_number":n,"wi_status":"resolved","title":"Explore trt-llm (done)"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(res["ok"], json!(true));

    let got = body(&server.call("get_work_item", args(json!({"wi_number":n}))).await.unwrap());
    assert_eq!(got["wi_status"], "resolved", "status flipped");
    assert_eq!(got["title"], "Explore trt-llm (done)", "title edited");
    assert_eq!(got["content"], "investigate", "untouched field preserved");
    assert_eq!(got["details"], "notes here", "omitted nullable left unchanged");

    // Clear a nullable field by passing null.
    server
        .call("update_work_item", args(json!({"wi_number":n,"details":null})))
        .await
        .unwrap();
    let cleared = body(&server.call("get_work_item", args(json!({"wi_number":n}))).await.unwrap());
    assert!(cleared["details"].is_null(), "null clears the field");

    // Archive it.
    server
        .call("update_work_item", args(json!({"wi_number":n,"archived":true})))
        .await
        .unwrap();
    let archived = body(&server.call("get_work_item", args(json!({"wi_number":n}))).await.unwrap());
    assert_eq!(archived["archived"], json!(true), "archived flag set");
}

/// Sprint 002 — MCP coverage gaps: project/area creation, card update, edge
/// removal (unrelate), and card comments must all be reachable over MCP.
#[tokio::test]
async fn mcp_coverage_gaps_end_to_end() {
    let (_c, pool) = fresh_korg().await;
    let server = KorgServer::new(pool);

    // create_project is idempotent and returns an id.
    let p1 = body(&server.call("create_project", args(json!({"name":"acme"}))).await.unwrap());
    let pid = p1["id"].as_i64().unwrap();
    let p2 = body(&server.call("create_project", args(json!({"name":"acme"}))).await.unwrap());
    assert_eq!(p2["id"].as_i64(), Some(pid), "create_project is idempotent");

    let projects = body(&server.call("list_projects", args(json!({}))).await.unwrap());
    assert!(projects.as_array().unwrap().iter().any(|p| p["name"] == "acme"));

    // create_area under the project, then list_areas surfaces it.
    let area = body(
        &server
            .call("create_area", args(json!({"project":"acme","name":"backend","description":"svc"})))
            .await
            .unwrap(),
    );
    let area_id = area["id"].as_i64().unwrap();
    let areas = body(&server.call("list_areas", args(json!({"project":"acme"}))).await.unwrap());
    let areas = areas.as_array().unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(areas[0]["name"], "backend");
    assert_eq!(areas[0]["id"].as_i64(), Some(area_id));

    // update_card: create a card, then move its status and edit the title.
    let card = body(
        &server
            .call("create_card", args(json!({"title":"draft","status":"Backlog","project_id":pid})))
            .await
            .unwrap(),
    );
    let card_node = card["node_id"].as_i64().unwrap();
    let upd = body(
        &server
            .call("update_card", args(json!({"node_id":card_node,"status":"Active","title":"shipped"})))
            .await
            .unwrap(),
    );
    assert_eq!(upd["ok"], json!(true));
    let cards = body(&server.call("list_cards", args(json!({}))).await.unwrap());
    let c = &cards.as_array().unwrap()[0];
    assert_eq!(c["status"], "Active", "card status moved");
    assert_eq!(c["title"], "shipped", "card title edited");

    // comments: add two, list them, delete one.
    let cm = body(
        &server
            .call("add_comment", args(json!({"card_node_id":card_node,"body":"first"})))
            .await
            .unwrap(),
    );
    let cm_id = cm["id"].as_i64().unwrap();
    server
        .call("add_comment", args(json!({"card_node_id":card_node,"body":"second"})))
        .await
        .unwrap();
    let comments = body(&server.call("list_comments", args(json!({"card_node_id":card_node}))).await.unwrap());
    assert_eq!(comments.as_array().unwrap().len(), 2);
    server.call("delete_comment", args(json!({"id":cm_id}))).await.unwrap();
    let after = body(&server.call("list_comments", args(json!({"card_node_id":card_node}))).await.unwrap());
    assert_eq!(after.as_array().unwrap().len(), 1, "one comment deleted");
    assert_eq!(after[0]["body"], "second");

    // relate -> unrelate round-trip. Make a second card to link to.
    let card2 = body(&server.call("create_card", args(json!({"title":"other"}))).await.unwrap());
    let card2_node = card2["node_id"].as_i64().unwrap();
    let rel = body(
        &server
            .call("relate", args(json!({"left":card_node,"right":card2_node,"label":"blocks"})))
            .await
            .unwrap(),
    );
    let rel_id = rel["id"].as_i64().unwrap();
    let ns = body(&server.call("neighbors", args(json!({"node_id":card_node}))).await.unwrap());
    let ns = ns.as_array().unwrap();
    assert_eq!(ns.len(), 1);
    assert_eq!(ns[0]["rel_id"].as_i64(), Some(rel_id), "neighbors exposes rel_id for unrelate");

    server.call("unrelate", args(json!({"id":rel_id}))).await.unwrap();
    let ns_after = body(&server.call("neighbors", args(json!({"node_id":card_node}))).await.unwrap());
    assert_eq!(ns_after.as_array().unwrap().len(), 0, "edge removed by unrelate");
}

/// WI #85 — `list_work_items` must accept an optional `project` filter so MCP
/// clients can scope to one project instead of pulling every work item.
#[tokio::test]
async fn list_work_items_filters_by_project() {
    let (_c, pool) = fresh_korg().await;
    let alpha = korg_core::repo::create_project(&pool, "alpha").await.unwrap();
    let beta = korg_core::repo::create_project(&pool, "beta").await.unwrap();
    let server = KorgServer::new(pool);

    for (title, pid) in [("A1", alpha), ("A2", alpha), ("B1", beta)] {
        server
            .call(
                "create_work_item",
                args(json!({"title":title,"content":"x","project_id":pid})),
            )
            .await
            .unwrap();
    }

    // Unfiltered: every work item.
    let all = body(&server.call("list_work_items", args(json!({}))).await.unwrap());
    assert_eq!(all.as_array().unwrap().len(), 3, "unfiltered returns all");

    // Filtered by project name: only alpha's two items.
    let only_alpha = body(
        &server
            .call("list_work_items", args(json!({"project":"alpha"})))
            .await
            .unwrap(),
    );
    let arr = only_alpha.as_array().unwrap();
    assert_eq!(arr.len(), 2, "project filter must scope results to that project");
    assert!(
        arr.iter().all(|w| w["project"] == "alpha"),
        "every returned item must belong to alpha"
    );

    // Unknown project: empty, not an error.
    let none = body(
        &server
            .call("list_work_items", args(json!({"project":"ghost"})))
            .await
            .unwrap(),
    );
    assert_eq!(none.as_array().unwrap().len(), 0, "unknown project yields none");
}
