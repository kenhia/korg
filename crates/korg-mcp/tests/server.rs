//! M4 acceptance — korg-mcp server dispatch over a real korg database.
//!
//! Builds the server against a fresh testcontainers korg DB and exercises the
//! tool surface end-to-end: tool listing, work-item/link creation, and
//! generalized relate + neighbors (cross-kind).
//!
//! Behavioural coverage lives here; `dispatch.rs` holds the completeness fence
//! that every advertised tool is actually dispatched somewhere.

use korg_mcp::tools::tools;
use korg_test_support::fresh_korg;
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

mod common;
use common::{args, body, server};

#[tokio::test]
async fn mcp_surface_end_to_end() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

    // Tool descriptors are stable.
    assert_eq!(tools().len(), 47, "expected 47 tools");

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
    // Since 0009_identity, wi_number IS the node id.
    assert_eq!(wi["wi_number"].as_i64(), Some(wi_node));

    // List shows it.
    let items = body(
        &server
            .call("list_work_items", args(json!({})))
            .await
            .unwrap(),
    );
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    assert_eq!(items["total"], 1);
    assert_eq!(items["items"][0]["title"], "Ship korg-mcp");

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

    // Cross-kind relationship work item <-> link (related-to permits any kinds).
    server
        .call(
            "relate",
            args(json!({"left":wi_node,"right":link_node,"label":"related-to"})),
        )
        .await
        .unwrap();

    let ns = body(
        &server
            .call("neighbors", args(json!({"node_id":wi_node})))
            .await
            .unwrap(),
    );
    let items = ns["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(ns["total"], 1);
    assert_eq!(ns["truncated"], false);
    assert_eq!(items[0]["node_id"].as_i64(), Some(link_node));
    assert_eq!(items[0]["kind"], "link");
    assert_eq!(items[0]["label"], "related-to");

    // Reading list reflects the link.
    let links = body(&server.call("list_links", args(json!({}))).await.unwrap());
    assert_eq!(links["items"].as_array().unwrap().len(), 1);

    // Topics and daily planning round-trip with server-derived display.
    let topic = body(
        &server
            .call("create_topic", args(json!({"name":"Architecture"})))
            .await
            .unwrap(),
    );
    let topic_node = topic["node_id"].as_i64().unwrap();
    let planned = body(
        &server
            .call(
                "create_daily_plan_item",
                args(json!({"source_node_id":topic_node,"plan_date":"2026-07-11"})),
            )
            .await
            .unwrap(),
    );
    let planned_node = planned["node_id"].as_i64().unwrap();
    let completed = body(
        &server
            .call(
                "set_daily_plan_completion",
                args(json!({"node_id":planned_node,"completed":true})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(completed["node_id"].as_i64(), Some(planned_node));
    assert!(
        completed["completed_at"].is_string(),
        "completion acknowledged"
    );
    let moved = body(
        &server
            .call(
                "move_daily_plan_item",
                args(
                    json!({"node_id":planned_node,"target_date":"2026-07-12","target_position":0}),
                ),
            )
            .await
            .unwrap(),
    );
    assert_eq!(moved["copied"], false);
    let day = body(
        &server
            .call(
                "list_daily_plan",
                args(json!({"from":"2026-07-12","to":"2026-07-12"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(day.as_array().unwrap().len(), 1);
    assert_eq!(day[0]["display"], "Architecture");

    // Unknown tool is a clean error.
    assert!(server.call("nope", args(json!({}))).await.is_err());
}

/// WI #92 — agents must be able to update a work item over MCP: set status
/// (resolve), edit fields, clear a nullable field, and archive.
#[tokio::test]
async fn update_work_item_partial_and_nullable() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

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
                args(
                    json!({"wi_number":n,"wi_status":"resolved","title":"Explore trt-llm (done)"}),
                ),
            )
            .await
            .unwrap(),
    );
    // Mutations acknowledge with the post-write row (WI #525), not `{ok:true}`.
    assert_eq!(res["wi_number"], json!(n));
    assert_eq!(res["wi_status"], "resolved");

    let got = body(
        &server
            .call("get_work_item", args(json!({"wi_number":n})))
            .await
            .unwrap(),
    );
    assert_eq!(got["wi_status"], "resolved", "status flipped");
    assert_eq!(got["title"], "Explore trt-llm (done)", "title edited");
    assert_eq!(got["content"], "investigate", "untouched field preserved");
    assert_eq!(
        got["details"], "notes here",
        "omitted nullable left unchanged"
    );

    // Clear a nullable field by passing null.
    server
        .call(
            "update_work_item",
            args(json!({"wi_number":n,"details":null})),
        )
        .await
        .unwrap();
    let cleared = body(
        &server
            .call("get_work_item", args(json!({"wi_number":n})))
            .await
            .unwrap(),
    );
    assert!(cleared["details"].is_null(), "null clears the field");

    // Archive it.
    server
        .call(
            "update_work_item",
            args(json!({"wi_number":n,"archived":true})),
        )
        .await
        .unwrap();
    let archived = body(
        &server
            .call("get_work_item", args(json!({"wi_number":n})))
            .await
            .unwrap(),
    );
    assert_eq!(archived["archived"], json!(true), "archived flag set");
}

/// Sprint 002 — MCP coverage gaps: project/area creation, card update, edge
/// removal (unrelate), and card comments must all be reachable over MCP.
#[tokio::test]
async fn mcp_coverage_gaps_end_to_end() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

    // create_project is idempotent and returns an id.
    let p1 = body(
        &server
            .call("create_project", args(json!({"name":"acme"})))
            .await
            .unwrap(),
    );
    let pid = p1["id"].as_i64().unwrap();
    let p2 = body(
        &server
            .call("create_project", args(json!({"name":"acme"})))
            .await
            .unwrap(),
    );
    assert_eq!(p2["id"].as_i64(), Some(pid), "create_project is idempotent");

    let projects = body(&server.call("list_projects", args(json!({}))).await.unwrap());
    assert!(projects
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "acme"));

    // create_area under the project, then list_areas surfaces it.
    let area = body(
        &server
            .call(
                "create_area",
                args(json!({"project":"acme","name":"backend","description":"svc"})),
            )
            .await
            .unwrap(),
    );
    let area_id = area["id"].as_i64().unwrap();
    let areas = body(
        &server
            .call("list_areas", args(json!({"project":"acme"})))
            .await
            .unwrap(),
    );
    let areas = areas.as_array().unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(areas[0]["name"], "backend");
    assert_eq!(areas[0]["id"].as_i64(), Some(area_id));

    // update_card: create a card, then move its status and edit the title.
    let card = body(
        &server
            .call(
                "create_card",
                args(json!({"title":"draft","status":"Backlog","project_id":pid})),
            )
            .await
            .unwrap(),
    );
    let card_node = card["node_id"].as_i64().unwrap();
    let upd = body(
        &server
            .call(
                "update_card",
                args(json!({"node_id":card_node,"status":"Active","title":"shipped"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(upd["node_id"], json!(card_node));
    assert_eq!(upd["status"], "Active");
    let cards = body(&server.call("list_cards", args(json!({}))).await.unwrap());
    let c = &cards["items"].as_array().unwrap()[0];
    assert_eq!(c["status"], "Active", "card status moved");
    assert_eq!(c["title"], "shipped", "card title edited");

    // comments: add two, list them, delete one (node-scoped `node_id` arg).
    let cm = body(
        &server
            .call(
                "add_comment",
                args(json!({"node_id":card_node,"body":"first"})),
            )
            .await
            .unwrap(),
    );
    let cm_id = cm["id"].as_i64().unwrap();
    server
        .call(
            "add_comment",
            args(json!({"node_id":card_node,"body":"second"})),
        )
        .await
        .unwrap();
    let comments = body(
        &server
            .call("list_comments", args(json!({"node_id":card_node})))
            .await
            .unwrap(),
    );
    assert_eq!(comments.as_array().unwrap().len(), 2);
    server
        .call("delete_comment", args(json!({"id":cm_id})))
        .await
        .unwrap();
    let after = body(
        &server
            .call("list_comments", args(json!({"node_id":card_node})))
            .await
            .unwrap(),
    );
    assert_eq!(after.as_array().unwrap().len(), 1, "one comment deleted");
    assert_eq!(after[0]["body"], "second");

    // Sprint 003: comments are node-scoped — they also attach to a WORK ITEM node.
    let wi = body(
        &server
            .call(
                "create_work_item",
                args(json!({"title":"commented WI","content":"x"})),
            )
            .await
            .unwrap(),
    );
    let wi_node = wi["node_id"].as_i64().unwrap();
    let wi_cm = body(
        &server
            .call(
                "add_comment",
                args(json!({"node_id":wi_node,"body":"note on a work item"})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        wi_cm["node_id"].as_i64(),
        Some(wi_node),
        "comment attached to the WI node"
    );
    let wi_comments = body(
        &server
            .call("list_comments", args(json!({"node_id":wi_node})))
            .await
            .unwrap(),
    );
    assert_eq!(
        wi_comments.as_array().unwrap().len(),
        1,
        "WI comment listed back"
    );
    assert_eq!(wi_comments[0]["body"], "note on a work item");

    // relate -> unrelate round-trip. Make a second card to link to.
    let card2 = body(
        &server
            .call("create_card", args(json!({"title":"other"})))
            .await
            .unwrap(),
    );
    let card2_node = card2["node_id"].as_i64().unwrap();
    let rel = body(
        &server
            .call(
                "relate",
                args(json!({"left":card_node,"right":card2_node,"label":"related-to"})),
            )
            .await
            .unwrap(),
    );
    let rel_id = rel["id"].as_i64().unwrap();
    let ns = body(
        &server
            .call("neighbors", args(json!({"node_id":card_node})))
            .await
            .unwrap(),
    );
    let items = ns["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["rel_id"].as_i64(),
        Some(rel_id),
        "neighbors exposes rel_id for unrelate"
    );

    server
        .call("unrelate", args(json!({"id":rel_id})))
        .await
        .unwrap();
    let ns_after = body(
        &server
            .call("neighbors", args(json!({"node_id":card_node})))
            .await
            .unwrap(),
    );
    assert_eq!(
        ns_after["items"].as_array().unwrap().len(),
        0,
        "edge removed by unrelate"
    );
}

/// WI #85 — `list_work_items` must accept an optional `project` filter so MCP
/// clients can scope to one project instead of pulling every work item.
#[tokio::test]
async fn list_work_items_filters_by_project() {
    let (_c, pool) = fresh_korg().await;
    let alpha = korg_core::repo::create_project(&pool, "alpha")
        .await
        .unwrap();
    let beta = korg_core::repo::create_project(&pool, "beta")
        .await
        .unwrap();
    let server = server(pool);

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
    let all = body(
        &server
            .call("list_work_items", args(json!({})))
            .await
            .unwrap(),
    );
    assert_eq!(
        all["items"].as_array().unwrap().len(),
        3,
        "unfiltered returns all"
    );
    assert_eq!(all["total"], 3);

    // Filtered by project name: only alpha's two items.
    let only_alpha = body(
        &server
            .call("list_work_items", args(json!({"project":"alpha"})))
            .await
            .unwrap(),
    );
    let arr = only_alpha["items"].as_array().unwrap();
    assert_eq!(
        arr.len(),
        2,
        "project filter must scope results to that project"
    );
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
    assert_eq!(
        none["items"].as_array().unwrap().len(),
        0,
        "unknown project yields none"
    );
    assert_eq!(none["total"], 0);
}

/// Sprint 004 — agent planning: `propose_sprint` bundles a proposal + its
/// `covers` edges in one call, `list_proposals` orders pinned-first-then-rank
/// and filters by status, `update_proposal` drives the status lifecycle.
#[tokio::test]
async fn propose_sprint_and_lifecycle() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

    let wi = body(
        &server
            .call(
                "create_work_item",
                args(json!({"title":"fix the thing","content":"x"})),
            )
            .await
            .unwrap(),
    );
    let wi_number = wi["wi_number"].as_i64().unwrap();

    let proposed = body(
        &server
            .call(
                "propose_sprint",
                args(json!({
                    "title":"Sprint: fix things",
                    "summary":"bundle of small fixes",
                    "work_item_numbers":[wi_number, 9999],
                })),
            )
            .await
            .unwrap(),
    );
    let node_id = proposed["node_id"].as_i64().unwrap();
    assert_eq!(
        proposed["covered"].as_array().unwrap().len(),
        1,
        "only the real wi_number resolves; 9999 is dropped"
    );

    let list = body(
        &server
            .call("list_proposals", args(json!({})))
            .await
            .unwrap(),
    );
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["status"], "proposed", "default status");
    assert_eq!(list[0]["pinned"], json!(false));

    // Pin it, then start it.
    server
        .call(
            "update_proposal",
            args(json!({"node_id":node_id,"pinned":true})),
        )
        .await
        .unwrap();
    server
        .call(
            "update_proposal",
            args(json!({"node_id":node_id,"status":"active"})),
        )
        .await
        .unwrap();

    let active = body(
        &server
            .call("list_proposals", args(json!({"status":"active"})))
            .await
            .unwrap(),
    );
    let active = active.as_array().unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(
        active[0]["pinned"],
        json!(true),
        "pin survived the status change"
    );

    let none_proposed = body(
        &server
            .call("list_proposals", args(json!({"status":"proposed"})))
            .await
            .unwrap(),
    );
    assert_eq!(
        none_proposed.as_array().unwrap().len(),
        0,
        "no longer in the proposed bucket"
    );
}

/// Sprint 006 — `survey_work_items`: a slim, paginated projection for
/// cross-project surveys (the `refill-queue` skill's use case) that can't
/// afford `list_work_items`'s full content/details payload at scale.
#[tokio::test]
async fn survey_work_items_paginates_and_filters() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

    for i in 0..5 {
        server
            .call(
                "create_work_item",
                args(json!({"title": format!("item {i}"), "content": "x", "wi_status": "open"})),
            )
            .await
            .unwrap();
    }
    server
        .call(
            "create_work_item",
            args(json!({"title": "closed one", "content": "x", "wi_status": "closed"})),
        )
        .await
        .unwrap();

    // Page 1 of 2, page size 2, status filter excludes the closed item.
    let page1 = body(
        &server
            .call(
                "survey_work_items",
                args(json!({"wi_status": "open", "limit": 2, "offset": 0})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        page1["total"].as_i64(),
        Some(5),
        "total reflects the full filtered count, not just the page"
    );
    assert_eq!(page1["items"].as_array().unwrap().len(), 2);
    let item = &page1["items"][0];
    assert!(
        item.get("content").is_none(),
        "survey is slim -- no content field"
    );
    assert!(
        item.get("details").is_none(),
        "survey is slim -- no details field"
    );
    assert!(item.get("wi_number").is_some());
    assert!(item.get("node_id").is_some());
    assert!(item.get("title").is_some());

    let page2 = body(
        &server
            .call(
                "survey_work_items",
                args(json!({"wi_status": "open", "limit": 2, "offset": 2})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(page2["items"].as_array().unwrap().len(), 2);
    assert_ne!(
        page1["items"][0]["wi_number"], page2["items"][0]["wi_number"],
        "pages don't overlap"
    );

    let all_open = body(
        &server
            .call(
                "survey_work_items",
                args(json!({"wi_status": "open", "limit": 50})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(
        all_open["items"].as_array().unwrap().len(),
        5,
        "closed item excluded by the status filter"
    );
}

/// The MCP half of the §4.2 matrix: not-found reads and missing/cross-kind
/// mutation targets are `isError` results carrying a `code`, not successful
/// `null`s and not `{"ok":true}` (D-5, D-6, F-03, F-04).
#[tokio::test]
async fn typed_errors_carry_codes_and_never_lie_about_success() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

    /// Assert an `isError` result with the given code, returning its message.
    fn err(result: &CallToolResult, code: &str, what: &str) {
        assert_eq!(result.is_error, Some(true), "{what}: expected isError");
        let text = result.content[0]
            .as_text()
            .expect("text content")
            .text
            .clone();
        let body: Value = serde_json::from_str(&text).expect("error body is json");
        assert_eq!(body["code"], code, "{what}: code (body {body:?})");
        assert!(
            body["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{what}: message"
        );
    }

    let wi = body(
        &server
            .call(
                "create_work_item",
                args(json!({"title":"real","content":"c"})),
            )
            .await
            .unwrap(),
    );
    let n = wi["wi_number"].as_i64().unwrap();

    err(
        &server
            .call("update_work_item", args(json!({"wi_number":9999})))
            .await
            .unwrap(),
        "not_found",
        "update_work_item on a missing number",
    );
    err(
        &server
            .call("get_work_item", args(json!({"wi_number":9999})))
            .await
            .unwrap(),
        "not_found",
        "get_work_item on a missing number",
    );
    err(
        &server
            .call("get_report", args(json!({"node_id":9999})))
            .await
            .unwrap(),
        "not_found",
        "get_report on a missing node",
    );
    err(
        &server
            .call("get_topic", args(json!({"node_id":9999})))
            .await
            .unwrap(),
        "not_found",
        "get_topic on a missing node",
    );
    err(
        &server
            .call(
                "relate",
                args(json!({"left":n,"right":9999,"label":"related-to"})),
            )
            .await
            .unwrap(),
        "not_found",
        "relate to a missing node",
    );
    err(
        &server
            .call("add_comment", args(json!({"node_id":9999,"body":"orphan"})))
            .await
            .unwrap(),
        "not_found",
        "comment on a missing node",
    );
    err(
        &server
            .call(
                "update_work_item",
                args(json!({"wi_number":n,"wi_tshirt":"GIGANTIC"})),
            )
            .await
            .unwrap(),
        "invalid_input",
        "unknown t-shirt size",
    );
    err(
        &server
            .call(
                "update_work_item",
                args(json!({"wi_number":n,"wi_type":"taks"})),
            )
            .await
            .unwrap(),
        "invalid_input",
        "unknown wi_type",
    );

    // The cross-kind hazard: a work item's node id is not a card's (F-04).
    err(
        &server
            .call(
                "update_card",
                args(json!({"node_id":n,"archived":true,"title":"hijack"})),
            )
            .await
            .unwrap(),
        "not_found",
        "update_card against a work item",
    );
    let got = body(
        &server
            .call("get_work_item", args(json!({"wi_number":n})))
            .await
            .unwrap(),
    );
    assert_eq!(got["archived"], false, "work item untouched");
    assert_eq!(got["title"], "real", "work item untouched");

    // Deletes say what they did.
    let gone = body(
        &server
            .call("delete_comment", args(json!({"id":9999})))
            .await
            .unwrap(),
    );
    assert_eq!(gone["deleted"], false, "nothing to delete");
    let gone = body(
        &server
            .call("unrelate", args(json!({"id":9999})))
            .await
            .unwrap(),
    );
    assert_eq!(gone["deleted"], false, "no such edge");
}

/// WI #537 — the schema is part of the contract, so drift between what a tool
/// *advertises* and what it *does* is a bug. This is the pre-B4 stopgap: B4
/// generates the schemas and makes it structural.
#[test]
fn advertised_defaults_match_server_behaviour() {
    let tools = tools();
    let schema_of = |name: &str| {
        tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no tool named {name}"))
            .input_schema
            .clone()
    };

    // The shared collection-read params must agree with korg-core's constants
    // everywhere they appear.
    for name in ["list_work_items", "list_cards", "list_links", "list_topics"] {
        let schema = schema_of(name);
        let props = schema["properties"].as_object().expect("properties");
        assert_eq!(
            props["archived"]["default"], false,
            "{name}: archived default must match korg_core::repo::archived_default()"
        );
        assert_eq!(
            props["limit"]["default"],
            korg_core::repo::LIST_LIMIT_DEFAULT,
            "{name}: limit default"
        );
        assert_eq!(
            props["limit"]["maximum"],
            korg_core::repo::LIST_LIMIT_MAX,
            "{name}: limit ceiling"
        );
        assert_eq!(props["offset"]["default"], 0, "{name}: offset default");
    }

    // survey_work_items is the one that lied (F-11): it advertised
    // `default: false` while the server treated omitted as *both*. It must now
    // advertise no default at all.
    let survey = schema_of("survey_work_items");
    assert!(
        survey["properties"]["archived"].get("default").is_none(),
        "survey_work_items must not advertise an archived default it doesn't apply"
    );

    // neighbors' bound must match the core constants too (sprint 014).
    let neighbors = schema_of("neighbors");
    assert_eq!(
        neighbors["properties"]["limit"]["default"],
        korg_core::repo::NEIGHBOR_LIMIT_DEFAULT
    );
    assert_eq!(
        neighbors["properties"]["limit"]["maximum"],
        korg_core::repo::NEIGHBOR_LIMIT_MAX
    );
}

/// Sprint 015 over MCP: envelopes, the archived default and how to escape it,
/// get_proposal, and the transactional update_link.
#[tokio::test]
async fn collection_contracts_and_new_tools_over_mcp() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);

    let mut numbers = vec![];
    for i in 0..3 {
        let wi = body(
            &server
                .call(
                    "create_work_item",
                    args(json!({"title": format!("item {i}"), "content":"c"})),
                )
                .await
                .unwrap(),
        );
        numbers.push(wi["wi_number"].as_i64().unwrap());
    }
    server
        .call(
            "update_work_item",
            args(json!({"wi_number": numbers[2], "archived": true})),
        )
        .await
        .unwrap();

    // Archived excluded by default…
    let page = body(
        &server
            .call("list_work_items", args(json!({})))
            .await
            .unwrap(),
    );
    assert_eq!(page["total"], 2, "archived excluded by default");
    assert_eq!(page["limit"], 200);

    // …and an explicit null means both (the documented escape hatch).
    let all = body(
        &server
            .call("list_work_items", args(json!({"archived": null})))
            .await
            .unwrap(),
    );
    assert_eq!(all["total"], 3);

    let clipped = body(
        &server
            .call("list_work_items", args(json!({"limit": 1})))
            .await
            .unwrap(),
    );
    assert_eq!(clipped["items"].as_array().unwrap().len(), 1);
    assert_eq!(clipped["total"], 2, "total is the full filtered count");

    // get_proposal: one call replaces list_proposals + neighbors + list_work_items.
    let proposal = body(
        &server
            .call(
                "propose_sprint",
                args(json!({
                    "title":"bundle","summary":"s",
                    "work_item_numbers":[numbers[1], numbers[0]]
                })),
            )
            .await
            .unwrap(),
    );
    let pnode = proposal["node_id"].as_i64().unwrap();
    let detail = body(
        &server
            .call("get_proposal", args(json!({"node_id": pnode})))
            .await
            .unwrap(),
    );
    let covered = detail["covered"].as_array().unwrap();
    assert_eq!(covered.len(), 2);
    assert_eq!(covered[0]["wi_number"], numbers[0], "ordered by wi_number");
    assert_eq!(covered[0]["title"], "item 0");
    assert_eq!(detail["covered_count"], 2);

    let missing = server
        .call("get_proposal", args(json!({"node_id": 999999})))
        .await
        .unwrap();
    assert_eq!(missing.is_error, Some(true), "missing proposal is isError");

    // list_proposals rows carry covered_count without a second call.
    let proposals = body(
        &server
            .call("list_proposals", args(json!({})))
            .await
            .unwrap(),
    );
    assert_eq!(proposals[0]["covered_count"], 2);

    // update_link: disposition, read and tags in one transaction — the 0004
    // workflow agents could not reach before (only mark_link_read existed).
    let link = body(
        &server
            .call("create_link", args(json!({"url":"https://example.com"})))
            .await
            .unwrap(),
    );
    let lnode = link["node_id"].as_i64().unwrap();
    let updated = body(
        &server
            .call(
                "update_link",
                args(json!({"node_id": lnode, "disposition":"Summarized",
                            "read": true, "tags":["ai"]})),
            )
            .await
            .unwrap(),
    );
    assert_eq!(updated["disposition"], "Summarized");
    assert_eq!(updated["read"], true);
    assert_eq!(updated["tags"][0], "ai");

    let bad = server
        .call(
            "update_link",
            args(json!({"node_id": lnode, "disposition":"Someday", "tags":["nope"]})),
        )
        .await
        .unwrap();
    assert_eq!(bad.is_error, Some(true));
    let after = body(&server.call("list_links", args(json!({}))).await.unwrap());
    assert_eq!(
        after["items"][0]["tags"][0], "ai",
        "a rejected patch changes nothing, not even its valid half"
    );
}

/// Name-keyed selectors over MCP (WI #575), including the error *shape* an
/// agent branches on: `isError` with a `code`, and a message that names the
/// remedy so the retry does not need a human.
#[tokio::test]
async fn tools_accept_a_project_name_and_explain_a_bad_one() {
    let (_c, pool) = fresh_korg().await;
    let server = server(pool);
    server
        .call("create_project", args(json!({"name":"korg"})))
        .await
        .expect("create_project");

    let wi = body(
        &server
            .call(
                "create_work_item",
                args(json!({"title":"by name","content":"c","project":"korg"})),
            )
            .await
            .expect("create_work_item by project name"),
    );
    assert_eq!(wi["project"], "korg");

    server
        .call("create_area", args(json!({"project":"korg","name":"ui"})))
        .await
        .expect("create_area");
    let wi = body(
        &server
            .call(
                "update_work_item",
                args(json!({"wi_number": wi["wi_number"], "area":"ui"})),
            )
            .await
            .expect("update_work_item area by name"),
    );
    assert_eq!(wi["area"], "ui");

    // An unresolvable name is an isError result carrying invalid_input and the
    // remedy — the error doubles as the documentation needed to retry.
    let failed = server
        .call(
            "create_work_item",
            args(json!({"title":"x","content":"c","project":"nope"})),
        )
        .await
        .expect("call completes");
    assert_eq!(failed.is_error, Some(true));
    let text = failed.content[0].as_text().expect("text").text.clone();
    let err: Value = serde_json::from_str(&text).expect("error body is json");
    assert_eq!(err["code"], "invalid_input", "{err}");
    let message = err["message"].as_str().unwrap_or_default();
    assert!(message.contains("nope"), "names the bad value: {message}");
    assert!(
        message.contains("list_projects"),
        "names the remedy: {message}"
    );

    // A case near-miss gets pointed at the real name rather than the list.
    let failed = server
        .call(
            "create_work_item",
            args(json!({"title":"x","content":"c","project":"KORG"})),
        )
        .await
        .expect("call completes");
    let text = failed.content[0].as_text().expect("text").text.clone();
    let err: Value = serde_json::from_str(&text).expect("error body is json");
    assert!(
        err["message"]
            .as_str()
            .unwrap_or_default()
            .contains("did you mean 'korg'"),
        "{err}"
    );

    // Nothing was created by any of the failures.
    let projects = body(
        &server
            .call("list_projects", args(json!({})))
            .await
            .expect("list_projects"),
    );
    assert_eq!(
        projects.as_array().unwrap().len(),
        1,
        "a failed name resolution created a project (the WI #537 bug)"
    );
}
