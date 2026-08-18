//! The REST routes and node-preview kinds nothing had ever requested
//! (WI #551).
//!
//! Five routes had no test at all (two of them, the daily-plan mutations,
//! left with the feature in sprint 050):
//!
//! ```text
//! GET    /api/areas
//! GET    /api/projects/:name/plan      ← sole caller of repo::project_edges
//! GET    /api/reports
//! ```
//!
//! `/api/projects/:name/plan` is the one that mattered. It is the only caller
//! of `project_edges`, which had zero coverage anywhere, and it feeds both the
//! `/plan` view and the `plan-status` skill — so a regression there does not
//! throw, it just answers "where are we on the plan" wrongly, which is the
//! failure mode nobody notices.
//!
//! `GET /api/nodes/:id` is here too: `api.rs` covered it for `workitem` and
//! `card`, leaving five of the seven kinds unpreviewed.

use axum::http::StatusCode;
use korg_core::repo::{self, NewReport};
use korg_test_support::new;
use serde_json::json;
use time::macros::date;

mod common;
use common::{app, app_with_pool, req, PROJECT};

async fn work_item(router: &axum::Router, title: &str, project: Option<&str>) -> (i64, i64) {
    let mut body = json!({"title": title, "content": ""});
    if let Some(p) = project {
        body["project"] = json!(p);
    }
    let (st, wi) = req(router, "POST", "/api/work-items", Some(body)).await;
    assert_eq!(st, StatusCode::OK, "create work item: {wi:?}");
    (
        wi["wi_number"].as_i64().expect("wi_number"),
        wi["node_id"].as_i64().expect("node_id"),
    )
}

// --- areas ------------------------------------------------------------------

/// `GET /api/areas` lists a project's areas, and is scoped to the project it
/// was asked about rather than returning every area korg knows.
#[tokio::test]
async fn areas_are_listed_per_project() {
    let (_pg, router) = app().await;

    for name in ["korg", "other"] {
        let (st, _) = req(
            &router,
            "POST",
            "/api/projects",
            Some(json!({"name": name})),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
    }
    for (project, area) in [("korg", "core"), ("korg", "web"), ("other", "elsewhere")] {
        let (st, body) = req(
            &router,
            "POST",
            "/api/areas",
            Some(json!({"project": project, "name": area})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "create area: {body:?}");
    }

    let (st, areas) = req(&router, "GET", "/api/areas?project=korg", None).await;
    assert_eq!(st, StatusCode::OK);
    let names: Vec<&str> = areas
        .as_array()
        .expect("array")
        .iter()
        .map(|a| a["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, vec!["core", "web"], "korg's areas only");

    let (st, none) = req(&router, "GET", "/api/areas?project=no-such", None).await;
    assert_eq!(
        st,
        StatusCode::OK,
        "an unknown project has no areas; that is not an error: {none:?}"
    );
    assert!(none.as_array().expect("array").is_empty());
}

// --- the plan view ----------------------------------------------------------

/// `GET /api/projects/:name/plan` returns the project's work items **and** its
/// `depends_on` edges — the only exercise `repo::project_edges` gets anywhere.
///
/// The assertion that matters is the scoping one: an edge is included only if
/// *both* endpoints belong to the project. An edge leaking in from another
/// project would draw a dependency arrow between nodes the view cannot render.
#[tokio::test]
async fn the_plan_view_returns_items_and_their_intra_project_edges() {
    let (_pg, router) = app().await;
    for name in ["korg", "other"] {
        req(
            &router,
            "POST",
            "/api/projects",
            Some(json!({"name": name})),
        )
        .await;
    }

    let (_, a) = work_item(&router, "foundation", Some("korg")).await;
    let (_, b) = work_item(&router, "depends on foundation", Some("korg")).await;
    let (_, elsewhere) = work_item(&router, "another project's item", Some("other")).await;
    let (_, unscoped) = work_item(&router, "no project at all", None).await;

    let relate = |left: i64, right: i64| {
        let router = router.clone();
        async move {
            let (st, body) = req(
                &router,
                "POST",
                "/api/relationships",
                Some(json!({"left": left, "right": right, "label": "depends_on"})),
            )
            .await;
            assert_eq!(st, StatusCode::OK, "relate: {body:?}");
        }
    };
    relate(b, a).await; // both in korg — belongs in the plan
    relate(a, elsewhere).await; // crosses into another project
    relate(a, unscoped).await; // one endpoint has no project

    let (st, plan) = req(&router, "GET", "/api/projects/korg/plan", None).await;
    assert_eq!(st, StatusCode::OK);

    let titles: Vec<&str> = plan["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["title"].as_str().expect("title"))
        .collect();
    assert_eq!(
        titles.len(),
        2,
        "the plan holds korg's items and nobody else's: {titles:?}"
    );

    let edges = plan["edges"].as_array().expect("edges");
    assert_eq!(
        edges.len(),
        1,
        "only the edge with both endpoints in korg belongs: {edges:?}"
    );
    assert_eq!(
        edges[0],
        json!([b, a]),
        "and it reads left-depends-on-right"
    );
}

/// The plan payload is the **whole** project, past the one-page ceiling it used
/// to stop at (WI #1391).
///
/// The old handler took one `LIST_LIMIT_MAX` page and dropped the envelope, so
/// a project past 500 items lost its newest ones — the read orders by
/// `wi_number` — and said nothing a consumer could notice it by. Both the
/// `/plan` view and the `plan-status` skill derive a frontier from this set: a
/// clipped answer is a wrong frontier, not a shorter one.
///
/// 501 items, seeded in one statement rather than 501 round trips — the subject
/// is the handler's paging, not the create path.
#[tokio::test]
async fn the_plan_view_walks_past_one_page() {
    let (_pg, pool, router) = app_with_pool().await;
    let over_one_page = repo::LIST_LIMIT_MAX + 1;

    let project_id: i64 = sqlx::query_scalar("SELECT id FROM project WHERE name = $1")
        .bind(PROJECT)
        .fetch_one(&pool)
        .await
        .expect("the harness seeded the test project");
    sqlx::query(
        "WITH n AS ( \
             INSERT INTO node (kind, project_id) \
             SELECT 'workitem', $1 FROM generate_series(1, $2) \
             RETURNING id \
         ) \
         INSERT INTO workitem (node_id, wi_number, wi_type, wi_status, wi_tshirt, title, content) \
         SELECT id, id, 'task', 'open', 'S', 'bulk item ' || id, '' FROM n",
    )
    .bind(project_id)
    .bind(over_one_page)
    .execute(&pool)
    .await
    .expect("seed a project past the page ceiling");

    let (st, plan) = req(
        &router,
        "GET",
        &format!("/api/projects/{PROJECT}/plan"),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        plan["items"].as_array().expect("items").len() as i64,
        over_one_page,
        "every item, not the first {} of them",
        repo::LIST_LIMIT_MAX
    );
    assert_eq!(
        plan["total"], over_one_page,
        "`total` is what makes a complete answer distinguishable from a clipped \
         one — the payload can be checked against itself"
    );
}

/// A project with nothing in it answers honestly rather than 404ing — the
/// `/plan` view renders an empty graph, and `plan-status` reports no work.
#[tokio::test]
async fn an_empty_project_has_an_empty_plan() {
    let (_pg, router) = app().await;
    req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name": "empty"})),
    )
    .await;

    let (st, plan) = req(&router, "GET", "/api/projects/empty/plan", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(plan["items"].as_array().expect("items").is_empty());
    assert!(plan["edges"].as_array().expect("edges").is_empty());
}

// --- reports ----------------------------------------------------------------

/// `GET /api/reports` and `GET /api/reports/:node_id`. Reports are written over
/// MCP only, so these two reads are the whole REST surface for them — and the
/// `/reports` UI's only source.
#[tokio::test]
async fn reports_can_be_read_over_rest() {
    let (_pg, pool, router) = app_with_pool().await;

    let (st, empty) = req(&router, "GET", "/api/reports", None).await;
    assert_eq!(st, StatusCode::OK);
    assert!(
        empty.as_array().expect("array").is_empty(),
        "no reports yet: {empty:?}"
    );

    // Reports have no REST write route — seed through the repo `create_report`
    // dispatches to.
    let finding = repo::create_work_item(&pool, new::work_item("a finding"))
        .await
        .expect("wi");
    for (source, day) in [
        ("kmon", date!(2026 - 07 - 10)),
        ("kmon", date!(2026 - 07 - 11)),
        ("other", date!(2026 - 07 - 11)),
    ] {
        repo::upsert_report(
            &pool,
            NewReport {
                findings: vec![finding.wi_number],
                body: "the body".into(),
                ..new::report(source, day)
            },
        )
        .await
        .expect("report");
    }

    let (st, all) = req(&router, "GET", "/api/reports", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(all.as_array().expect("array").len(), 3);

    let (st, mine) = req(&router, "GET", "/api/reports?source=kmon", None).await;
    assert_eq!(st, StatusCode::OK);
    let items = mine.as_array().expect("array");
    assert_eq!(items.len(), 2, "the source filter reaches the query string");
    assert_eq!(
        items[0]["report_date"], "2026-07-11",
        "newest first survives the REST hop"
    );

    let node_id = items[0]["node_id"].as_i64().expect("node_id");
    let (st, one) = req(&router, "GET", &format!("/api/reports/{node_id}"), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        one["body"], "the body",
        "the full read carries the markdown"
    );
    assert_eq!(
        one["findings"][0]["wi_number"], finding.wi_number,
        "and the linked findings"
    );

    let (st, missing) = req(&router, "GET", "/api/reports/999999", None).await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "a missing report must 404, not 200 with null"
    );
    assert_eq!(missing["code"], "not_found");
}

// --- node previews ----------------------------------------------------------

/// `GET /api/nodes/:id` resolves any node id to a uniform preview. `api.rs`
/// covers `workitem` and `card`; this covers the other five kinds.
///
/// The preview is what the UI renders when you follow a relationship to a node
/// whose kind you were not already looking at, so an uncovered kind degrades
/// silently to the fallback title `"{kind} #{id}"` — a preview that renders,
/// looks deliberate, and says nothing. Each assertion below therefore checks a
/// field that only the kind-specific branch can produce.
#[tokio::test]
async fn every_node_kind_previews_with_its_own_shape() {
    let (_pg, pool, router) = app_with_pool().await;

    let preview = |id: i64| {
        let router = router.clone();
        async move {
            let (st, body) = req(&router, "GET", &format!("/api/nodes/{id}"), None).await;
            assert_eq!(st, StatusCode::OK, "preview {id}: {body:?}");
            body
        }
    };
    let field = |p: &serde_json::Value, label: &str| -> Option<String> {
        p["fields"]
            .as_array()?
            .iter()
            .find(|f| f["label"] == label)?["value"]
            .as_str()
            .map(str::to_string)
    };

    // --- link ---
    let link = repo::create_link(&pool, new::link("https://example.invalid/read-me"))
        .await
        .expect("link");
    let p = preview(link.node_id).await;
    assert_eq!(p["kind"], "link");
    assert_eq!(
        p["badges"],
        json!(["Unread", "unread"]),
        "a link previews its disposition and read state"
    );
    assert_eq!(
        field(&p, "URL").as_deref(),
        Some("https://example.invalid/read-me")
    );

    // --- report ---
    let report = repo::upsert_report(
        &pool,
        NewReport {
            body: "the full report".into(),
            escalated: true,
            model: Some("claude".into()),
            ..new::report("kmon", date!(2026 - 07 - 11))
        },
    )
    .await
    .expect("report");
    let p = preview(report.node_id).await;
    assert_eq!(p["kind"], "report");
    assert_eq!(
        p["title"], "kmon — 2026-07-11",
        "a report titles itself source — date"
    );
    assert_eq!(p["badges"], json!(["ok", "escalated"]));
    assert_eq!(field(&p, "Model").as_deref(), Some("claude"));
    assert_eq!(p["body"], "the full report");

    // --- sprint_proposal ---
    let proposal = repo::create_proposal(
        &pool,
        korg_core::repo::NewProposal {
            summary: "what this sprint is".into(),
            pinned: true,
            ..new::proposal("a proposal")
        },
    )
    .await
    .expect("proposal");
    let p = preview(proposal.row.node_id).await;
    assert_eq!(p["kind"], "sprint_proposal");
    assert_eq!(p["title"], "a proposal");
    assert_eq!(p["badges"], json!(["proposed", "pinned"]));
    assert_eq!(p["body"], "what this sprint is");
    assert_eq!(p["body_label"], "Summary");
}

/// No node has this id, and the preview says so rather than inventing one.
#[tokio::test]
async fn previewing_a_missing_node_is_not_found() {
    let (_pg, router) = app().await;
    let (st, body) = req(&router, "GET", "/api/nodes/999999", None).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

// --- programs and the awaiting lane (#968, #969) ----------------------------
//
// Both surfaces are what the web UI calls, and both are new routes: without
// these the `/programs` pages and the Today lane are typed against a contract
// nothing exercises.

/// The full program round-trip over REST, ending at the rollup the detail page
/// renders from a single call.
#[tokio::test]
async fn a_program_is_created_listed_and_rolled_up_over_rest() {
    let (_pg, router) = app().await;

    // Two proposals in two projects — the case a proposal is not allowed to be.
    let (st, _) = req(
        &router,
        "POST",
        "/api/projects",
        Some(json!({"name": "elsewhere"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (wi, _) = work_item(&router, "work in the first slice", Some(PROJECT)).await;
    let (st, first) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(
            json!({"title": "first slice", "summary": "s", "project": PROJECT,
                    "work_item_numbers": [wi]}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "first proposal: {first:?}");
    let (st, second) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title": "second slice", "summary": "s", "project": "elsewhere"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "second proposal: {second:?}");

    let (st, program) = req(
        &router,
        "POST",
        "/api/programs",
        Some(json!({
            "title": "spans two repos",
            "aim": "the thing a proposal may not be",
            "slices": [second["node_id"], first["node_id"]],
        })),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "create program: {program:?}");
    let id = program["node_id"].as_i64().expect("node_id");
    assert_eq!(
        program["span"],
        json!(["elsewhere", PROJECT]),
        "span is derived from the slices, alphabetical"
    );

    // The list is the `{items, omitted}` envelope, not a bare array.
    let (st, list) = req(&router, "GET", "/api/programs", None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list["items"].as_array().expect("items").len(), 1);
    assert_eq!(list["omitted"]["done"], 0);

    // The detail read: slices in the order given, each with its own rollup.
    let (st, detail) = req(&router, "GET", &format!("/api/programs/{id}"), None).await;
    assert_eq!(st, StatusCode::OK, "get program: {detail:?}");
    let slices = detail["slices"].as_array().expect("slices");
    assert_eq!(slices.len(), 2);
    assert_eq!(
        slices[0]["title"], "second slice",
        "caller's order, not node_id order"
    );
    assert_eq!(slices[1]["covered_count"], 1);
    assert_eq!(slices[1]["open"], 1);

    // Status transition, and the list narrows once it is done.
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/programs/{id}"),
        Some(json!({"status": "done"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, list) = req(&router, "GET", "/api/programs", None).await;
    assert!(list["items"].as_array().expect("items").is_empty());
    assert_eq!(list["omitted"]["done"], 1, "hidden, and counted");
}

/// D-6 over the wire: the refusal is a 400 that names the rule, not a silent
/// drop that leaves the caller believing the program is filed somewhere.
#[tokio::test]
async fn rest_refuses_a_program_with_a_project() {
    let (_pg, router) = app().await;
    let (st, body) = req(
        &router,
        "POST",
        "/api/programs",
        Some(json!({"title": "t", "aim": "a", "project": PROJECT})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body:?}");
    assert_eq!(body["code"], "invalid_input");
    assert!(
        body["error"].as_str().unwrap().contains("CROSS-project"),
        "the refusal teaches the rule: {body:?}"
    );
}

/// The lane and its one-click clear — the two calls the Today page makes.
#[tokio::test]
async fn the_awaiting_lane_is_readable_and_clearable_over_rest() {
    let (_pg, router) = app().await;
    let (wi_number, node) = work_item(&router, "needs a decision", Some(PROJECT)).await;

    let (st, marked) = req(
        &router,
        "PUT",
        &format!("/api/nodes/{node}/awaiting"),
        Some(json!({"awaiting": true, "note": "which approach?"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "set: {marked:?}");
    assert!(marked["awaiting_since"].is_string());

    let (st, lane) = req(&router, "GET", "/api/awaiting", None).await;
    assert_eq!(st, StatusCode::OK);
    let rows = lane.as_array().expect("a bare array, per the shape table");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["title"], "needs a decision");
    assert_eq!(rows[0]["kind"], "workitem");
    assert_eq!(rows[0]["wi_number"], wi_number);
    assert_eq!(
        rows[0]["status"], "open",
        "the node's own status, for the board"
    );
    assert_eq!(rows[0]["awaiting_note"], "which approach?");

    // The UI's clear button.
    let (st, cleared) = req(
        &router,
        "PUT",
        &format!("/api/nodes/{node}/awaiting"),
        Some(json!({"awaiting": false})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "clear: {cleared:?}");
    assert!(cleared["awaiting_since"].is_null());

    let (_, lane) = req(&router, "GET", "/api/awaiting", None).await;
    assert!(lane.as_array().expect("array").is_empty());
}

// --- the board rollup (#970) ------------------------------------------------

/// The whole board over REST, in one GET. The assertion that matters is the
/// *shape*: kfdc and korg-dash type against these keys, and it is one composite
/// object rather than any of the collection envelopes.
#[tokio::test]
async fn the_board_is_one_request_with_every_panel_on_it() {
    let (_pg, router) = app().await;

    let (wi, _) = work_item(&router, "in flight", Some(PROJECT)).await;
    let (st, firing) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title": "firing", "summary": "the mission",
                    "project": PROJECT, "work_item_numbers": [wi]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{firing:?}");
    let firing_id = firing["node_id"].as_i64().expect("node_id");
    let (st, _) = req(
        &router,
        "PATCH",
        &format!("/api/proposals/{firing_id}"),
        Some(json!({"status": "active"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, queued) = req(
        &router,
        "POST",
        "/api/proposals",
        Some(json!({"title": "on deck", "summary": "next", "project": PROJECT})),
    )
    .await;
    let (st, program) = req(
        &router,
        "POST",
        "/api/programs",
        Some(json!({"title": "operation", "aim": "a",
                    "slices": [firing_id, queued["node_id"]]})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{program:?}");

    let (blocked_wi, blocked) = work_item(&router, "your ops action", Some(PROJECT)).await;
    let (st, _) = req(
        &router,
        "PUT",
        &format!("/api/nodes/{blocked}/awaiting"),
        Some(json!({"awaiting": true, "note": "rotate the password"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, board) = req(&router, "GET", "/api/board", None).await;
    assert_eq!(st, StatusCode::OK, "{board:?}");
    assert!(board["generated"].is_string(), "Postgres's clock, stamped");

    let active = board["active"].as_array().expect("active");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0]["title"], "firing");
    assert_eq!(
        active[0]["summary"], "the mission",
        "Fire Missions' subtitle"
    );
    assert_eq!(
        (
            active[0]["covered_count"].as_i64(),
            active[0]["open"].as_i64()
        ),
        (Some(1), Some(1))
    );

    assert_eq!(board["queue"].as_array().expect("queue").len(), 1);
    assert_eq!(board["proposals_omitted"]["done"], 0);

    let programs = board["programs"].as_array().expect("programs");
    assert_eq!(programs.len(), 1);
    assert_eq!(
        programs[0]["slices"].as_array().expect("slices").len(),
        2,
        "Operations renders from this alone — no follow-up per program"
    );
    assert_eq!(programs[0]["span"], json!([PROJECT]));
    assert_eq!(board["programs_omitted"]["archived"], 0);

    let awaiting = board["awaiting"].as_array().expect("awaiting");
    assert_eq!(awaiting.len(), 1);
    assert_eq!(awaiting[0]["wi_number"], blocked_wi);
    assert_eq!(awaiting[0]["awaiting_note"], "rotate the password");

    let depth = board["depth"].as_array().expect("depth");
    assert!(depth
        .iter()
        .any(|d| d["project"] == PROJECT && d["status"] == "active"));
    assert!(board["reports"].is_array());

    // D-3, over the wire: there is no counters block, and there must not be —
    // every figure a header bar wants comes out of the lists above.
    assert!(
        board.get("counts").is_none(),
        "a counter that can disagree with the list beside it is a bug"
    );
}

/// `GET /api/work-items/flow` (#1318): the same series the MCP tool serves,
/// over the query-string spelling — default window without a param, narrowed
/// with one, and the envelope naming horizon and timezone either way.
#[tokio::test]
async fn work_item_flow_serves_the_series_over_rest() {
    let (_pg, router) = app().await;

    let (st, flow) = req(&router, "GET", "/api/work-items/flow", None).await;
    assert_eq!(st, StatusCode::OK, "{flow:?}");
    let days = flow["days"].as_array().expect("days");
    assert_eq!(
        days.len(),
        korg_core::repo::FLOW_DAYS_DEFAULT as usize,
        "the default window"
    );
    assert!(flow["horizon"].is_string(), "the clamp boundary is named");
    assert!(
        flow.get("backlog_before").is_some(),
        "the window delta's baseline is on the envelope (#1432)"
    );
    assert_eq!(flow["timezone"], "UTC", "the app config's timezone");
    for key in [
        "day",
        "added",
        "closed",
        "backlog",
        "added_durable",
        "closed_durable",
    ] {
        assert!(days[0].get(key).is_some(), "a flow day is missing `{key}`");
    }

    let (st, narrow) = req(&router, "GET", "/api/work-items/flow?days=3", None).await;
    assert_eq!(st, StatusCode::OK, "{narrow:?}");
    assert_eq!(narrow["days"].as_array().expect("days").len(), 3);
}
