//! MCP tool implementations backed by `korg-core`.
//!
//! Exposes work items, cards, reading-list links, generalized relationships,
//! and calendar slots to AI agents over the MCP protocol.

use korg_core::config::KorgConfig;
use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::repo::{
    self, create_card, create_link, create_proposal, create_work_item, list_cards, list_links,
    list_projects, list_work_items, update_work_item, upsert_report, CardPatch, NewCard, NewLink,
    NewProposal, NewReport, NewWorkItem, ProposalPatch, WorkItemPatch,
};
use korg_core::{daily_plan, topics};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::Arc;
use time::macros::format_description;
use time::Date;

#[derive(Clone)]
pub struct KorgServer {
    pub pool: PgPool,
    pub config: Arc<KorgConfig>,
}

impl KorgServer {
    pub fn new(pool: PgPool, config: Arc<KorgConfig>) -> Self {
        Self { pool, config }
    }
}

// --- tool descriptors -----------------------------------------------------

pub fn tools() -> Vec<Tool> {
    let tags = json!({"type":"array","items":{"type":"string","minLength":1}});
    let id = json!({"type":"integer","format":"int64"});
    let date = json!({"type":"string","description":"YYYY-MM-DD"});

    vec![
        tool("create_work_item", "Create a work item. Returns the created row (including node_id and the serial wi_number, which are the same number since the 0009 identity migration).", json!({
            "type":"object","additionalProperties":false,
            "required":["title","content"],
            "properties":{
                "title":{"type":"string","minLength":1},
                "content":{"type":"string"},
                "wi_type":{"type":"string","enum":["task","bug","chore","feature","research","tweak","brainstorm"],"default":"task"},
                "wi_status":{"type":"string","enum":["open","resolved","done","closed"],"default":"open"},
                "wi_tshirt":{"type":"string","enum":["XS","S","M","L","XL","Huge","Unknown"],"default":"Unknown"},
                "sprint":{"type":["string","null"]},
                "details":{"type":["string","null"]},
                "project_id":id,
                "area_id":id,
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("list_work_items", "List work items (each includes `comment_count`; get_work_item to read the discussion). Pass `project` (name) to scope to a single project; omit it to list all.", json!({
            "type":"object","additionalProperties":false,
            "properties":{"project":{"type":["string","null"],"description":"Project name to filter by"}}
        })),
        tool("survey_work_items", "Slim, paginated work-item listing (wi_number, node_id, project, title, wi_type, wi_status, wi_tshirt, comment_count only -- no content/details). Use this instead of list_work_items for cross-project surveys, which can exceed tool-output limits at instance scale. Returns {items, total, limit, offset}.", json!({
            "type":"object","additionalProperties":false,
            "properties":{
                "project":{"type":["string","null"],"description":"Project name to filter by"},
                "wi_status":{"type":["string","null"],"description":"Status to filter by, e.g. \"open\""},
                "archived":{"type":["boolean","null"],"default":false,"description":"Filter by archived flag; omit/null for both"},
                "limit":{"type":"integer","minimum":1,"maximum":500,"default":50},
                "offset":{"type":"integer","minimum":0,"default":0}
            }
        })),
        tool("get_work_item", "Fetch a single work item by its wi_number (isError with code `not_found` if there is none), with its comments inlined (up to 10; `comments_truncated:true` + `comment_count` signal a longer thread — page the tail via list_comments). Comments often hold the real payload (resolution rationale, decisions), so prefer this over list_work_items when you need the full state of one item.", json!({
            "type":"object","additionalProperties":false,"required":["wi_number"],
            "properties":{"wi_number":id}
        })),
        tool("update_work_item", "Partially update a work item by its wi_number; returns the updated row (isError with code `not_found` if the wi_number does not exist). Only the fields you pass are changed. Status lifecycle: open -> resolved (implemented; may still need a user test or PR) -> done (agent satisfied; terminal but visible in default lists) -> closed (reserved for Ken; hidden by default -- do not set unless directed). For nullable fields (project_id, details, sprint, area_id, parent, category) pass null to clear or omit to leave unchanged. Moving projects (project_id) clears an area that no longer belongs to the target project unless you pass a valid area_id in the same call.", json!({
            "type":"object","additionalProperties":false,
            "required":["wi_number"],
            "properties":{
                "wi_number":id,
                "title":{"type":"string","minLength":1},
                "content":{"type":"string"},
                "wi_type":{"type":"string","enum":["task","bug","chore","feature","research","tweak","brainstorm"]},
                "wi_status":{"type":"string","enum":["open","resolved","done","closed"]},
                "wi_tshirt":{"type":"string","enum":["XS","S","M","L","XL","Huge","Unknown"]},
                "sprint":{"type":["string","null"]},
                "details":{"type":["string","null"]},
                "project_id":{"type":["integer","null"],"format":"int64","description":"Move to this project (id); null unassigns. Get ids from list_projects."},
                "area_id":{"type":["integer","null"],"format":"int64"},
                "parent":{"type":["integer","null"],"format":"int64","description":"Parent work item's wi_number; null clears the parent."},
                "archived":{"type":"boolean"},
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("create_card", "Create a kanban card. Returns the created card row.", json!({
            "type":"object","additionalProperties":false,"required":["title"],
            "properties":{
                "title":{"type":"string","minLength":1},
                "status":{"type":"string","enum":["Backlog","Research","OnDeck","Active","Done","Cut"],"default":"Backlog"},
                "description":{"type":"string","default":""},
                "rank":{"type":"number","default":0},
                "project_id":id,
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("update_card", "Partially update a kanban card by its node_id; returns the updated card (isError with code `not_found` if that node is missing or is not a card). Only the fields you pass are changed (move status/rank, edit title/description, archive, reassign project). For nullable fields (project_id, category) pass null to clear or omit to leave unchanged.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{
                "node_id":id,
                "status":{"type":"string","enum":["Backlog","Research","OnDeck","Active","Done","Cut"]},
                "rank":{"type":"number"},
                "title":{"type":"string","minLength":1},
                "description":{"type":"string"},
                "archived":{"type":"boolean"},
                "project_id":{"type":["integer","null"],"format":"int64"},
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("list_cards", "List all cards ordered by status then rank.", empty()),
        tool("list_comments", "List the comments on a node (work item or card), oldest first.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{"node_id":id}
        })),
        tool("add_comment", "Add a comment to a node of any kind. Returns the created comment; isError with code `not_found` if the node does not exist.", json!({
            "type":"object","additionalProperties":false,"required":["node_id","body"],
            "properties":{"node_id":id,"body":{"type":"string","minLength":1}}
        })),
        tool("delete_comment", "Delete a comment by its id. Returns {deleted: bool} — false means there was no such comment.", json!({
            "type":"object","additionalProperties":false,"required":["id"],
            "properties":{"id":id}
        })),
        tool("update_comment", "Edit a comment's body by its id (from list_comments). `created` is preserved; `updated` advances.", json!({
            "type":"object","additionalProperties":false,"required":["id","body"],
            "properties":{"id":id,"body":{"type":"string","minLength":1}}
        })),
        tool("create_link", "Capture a reading-list URL. Returns the created link row.", json!({
            "type":"object","additionalProperties":false,"required":["url"],
            "properties":{
                "url":{"type":"string","minLength":1},
                "title":{"type":["string","null"]},
                "project_id":id,
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("list_links", "List reading-list links.", empty()),
        tool("mark_link_read", "Mark a reading-list link read or unread. Returns the updated link.", json!({
            "type":"object","additionalProperties":false,"required":["node_id","read"],
            "properties":{"node_id":id,"read":{"type":"boolean"}}
        })),
        tool("relate", "Create a relationship edge between any two nodes. The label reads left-to-right. Known labels and their direction: `covers` (proposal -> work item), `finding` (report -> work item), `depends_on` (dependent -> dependency) are DIRECTED -- orientation is meaningful, and the reverse is a distinct edge (A depends_on B plus B depends_on A is a cycle, not a duplicate). `related-to` is UNDIRECTED -- orientation is stored but meaningless, so read it symmetrically. Any other label is allowed and its direction is caller-defined: korg stores your order faithfully without interpreting it. Exact duplicates dedup. Both endpoints must exist (isError `not_found`) and must differ (isError `invalid_input` -- self-edges are rejected).", json!({
            "type":"object","additionalProperties":false,"required":["left","right","label"],
            "properties":{"left":id,"right":id,"label":{"type":"string","minLength":1}}
        })),
        tool("neighbors", "List the nodes linked to a node (any kind), with labels. Returns {items, total, limit, truncated}. Each item has `rel_id` (pass to `unrelate`), `direction` (\"out\" = the queried node is the edge's left, so the label reads queried->neighbor; \"in\" = the reverse) and `directed` -- when `directed` is false the label is registry-undirected (e.g. related-to) and you MUST treat the edge as symmetric, ignoring `direction`. Filter server-side with `label` and/or `kind` instead of pulling every edge: e.g. label=\"covers\", kind=\"workitem\" for a proposal's work items. Ordering is neighbor node_id then rel_id.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{
                "node_id":id,
                "label":{"type":["string","null"],"description":"Only edges with this relationship label."},
                "kind":{"type":["string","null"],"description":"Only neighbors of this node kind, e.g. \"workitem\"."},
                "limit":{"type":"integer","minimum":1,"maximum":500,"default":100,"description":"Cap on returned edges; `truncated` says whether more matched."}
            }
        })),
        tool("unrelate", "Remove a relationship edge by its id (the `rel_id` from `neighbors`, or the id returned by `relate`). Returns {deleted: bool} — false means there was no such edge.", json!({
            "type":"object","additionalProperties":false,"required":["id"],
            "properties":{"id":id}
        })),
        tool("create_topic", "Create a reusable planning topic. Returns the created topic.", json!({
            "type":"object","additionalProperties":false,"required":["name"],
            "properties":{"name":{"type":"string","minLength":1},"description":{"type":["string","null"]},"project_id":id,"category":{"type":["string","null"]},"tags":tags}
        })),
        tool("get_topic", "Fetch a topic by node_id, including archived topics. isError with code `not_found` if there is none.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],"properties":{"node_id":id}
        })),
        tool("list_topics", "List non-archived topics.", empty()),
        tool("search_topics", "Search non-archived topic names and descriptions.", json!({
            "type":"object","additionalProperties":false,"required":["query"],"properties":{"query":{"type":"string"}}
        })),
        tool("update_topic", "Partially update a topic; returns the updated topic.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{"node_id":id,"name":{"type":"string","minLength":1},"description":{"type":["string","null"]},"category":{"type":["string","null"]},"tags":tags}
        })),
        tool("archive_topic", "Archive or restore a topic; returns the updated topic.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],"properties":{"node_id":id,"archived":{"type":"boolean","default":true}}
        })),
        tool("list_daily_plan", "List daily plan items in an inclusive date range with snapshots and current source titles.", json!({
            "type":"object","additionalProperties":false,"required":["from","to"],
            "properties":{"from":date.clone(),"to":date.clone()}
        })),
        tool("create_daily_plan_item", "Plan a work item, card, or topic. Display is resolved and snapshotted server-side. Returns the created item.", json!({
            "type":"object","additionalProperties":false,"required":["source_node_id","plan_date"],
            "properties":{"source_node_id":id,"plan_date":date.clone()}
        })),
        tool("set_daily_plan_completion", "Complete or uncomplete any daily plan item; timestamp is server-authoritative. Returns the updated item.", json!({
            "type":"object","additionalProperties":false,"required":["node_id","completed"],
            "properties":{"node_id":id,"completed":{"type":"boolean"}}
        })),
        tool("delete_daily_plan_item", "Delete an item from an open day; past structure is frozen. Returns {deleted: true}.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],"properties":{"node_id":id}
        })),
        tool("reorder_daily_plan", "Replace the complete order for an open day. Returns the day in its new order.", json!({
            "type":"object","additionalProperties":false,"required":["plan_date","node_ids"],
            "properties":{"plan_date":date.clone(),"node_ids":{"type":"array","items":id}}
        })),
        tool("move_daily_plan_item", "Move an item to today/future. Open sources transfer; past sources copy and remain unchanged.", json!({
            "type":"object","additionalProperties":false,"required":["node_id","target_date"],
            "properties":{"node_id":id,"target_date":date.clone(),"target_position":{"type":"integer","minimum":0,"default":0}}
        })),
        tool("daily_plan_history", "Return all complete and incomplete historical items plus completion totals/rate. End must be before local today.", json!({
            "type":"object","additionalProperties":false,"required":["from","to"],
            "properties":{"from":date.clone(),"to":date,"source_node_id":id}
        })),
        tool("propose_sprint", "Propose a sprint: bundle a title + summary with the work items it covers, in one call. Returns the created proposal plus `covered` -- which of the given wi_numbers actually resolved. Numbers that do not resolve are dropped, so compare `covered` against your request.", json!({
            "type":"object","additionalProperties":false,
            "required":["title","summary"],
            "properties":{
                "title":{"type":"string","minLength":1},
                "summary":{"type":"string"},
                "work_item_numbers":{"type":"array","items":{"type":"integer","format":"int64"},"default":[]},
                "project_id":id,
                "rank":{"type":"number","default":0,"description":"Drag-order position; lower sorts first among unpinned proposals."},
                "pinned":{"type":"boolean","default":false},
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("create_report", "Create or replace the daily report for (source, report_date). Same-day re-runs REPLACE both the content and the finding set -- findings you omit are unlinked -- but keep the node_id (links/comments survive). `findings_linked` echoes the wi_numbers that resolved; numbers that do not resolve are dropped, so compare it against your request.", json!({
            "type":"object","additionalProperties":false,
            "required":["source","report_date","status","summary","body"],
            "properties":{
                "source":{"type":"string","minLength":1,"description":"reporter id, e.g. 'kmon'"},
                "report_date":{"type":"string","format":"date","description":"YYYY-MM-DD"},
                "status":{"type":"string","enum":["ok","attention","problem"]},
                "summary":{"type":"string","minLength":1,"description":"one-liner for the list view"},
                "body":{"type":"string","description":"full markdown report"},
                "model":{"type":["string","null"]},
                "escalated":{"type":"boolean","default":false},
                "finding_work_items":{"type":"array","items":{"type":"integer","format":"int64"},"default":[]}
            }
        })),
        tool("list_reports", "List daily reports, newest first (summary fields only). Pass `source` to filter.", json!({
            "type":"object","additionalProperties":false,
            "properties":{"source":{"type":["string","null"]},"limit":{"type":"integer","default":30}}
        })),
        tool("get_report", "Fetch one report by node_id: full body plus linked finding work items. isError with code `not_found` if there is none.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{"node_id":id}
        })),
        tool("list_proposals", "List sprint proposals, pinned first then by rank (the drag order a user or agent left them in). Pass `status` to filter.", json!({
            "type":"object","additionalProperties":false,
            "properties":{"status":{"type":["string","null"],"enum":["proposed","active","done","declined",null]}}
        })),
        tool("update_proposal", "Partially update a sprint proposal by its node_id; returns the updated proposal (isError with code `not_found` if that node is missing or is not a proposal). Only the fields you pass are changed. Use this for status transitions (proposed -> active -> done/declined), reordering (rank), pinning, or archiving.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{
                "node_id":id,
                "title":{"type":"string","minLength":1},
                "summary":{"type":"string"},
                "status":{"type":"string","enum":["proposed","active","done","declined"]},
                "rank":{"type":"number"},
                "pinned":{"type":"boolean"},
                "archived":{"type":"boolean"},
                "tags":tags
            }
        })),
        tool("list_projects", "List projects, including metadata: status (active|maintenance|inactive|archived), machines (where the working copy lives), deploy_to (where it deploys), category.", empty()),
        tool("create_project", "Create a project by name (idempotent — returns the existing id if it already exists). Returns its id.", json!({
            "type":"object","additionalProperties":false,"required":["name"],
            "properties":{"name":{"type":"string","minLength":1}}
        })),
        tool("update_project", "Update a project's metadata by name (the name itself is immutable), returning the updated project: status (active|maintenance|inactive|archived), machines, deploy_to, category, description, gh_repo, cn_path. Omitted fields are unchanged.", json!({
            "type":"object","additionalProperties":false,"required":["name"],
            "properties":{
                "name":{"type":"string","minLength":1},
                "status":{"type":"string","enum":["active","maintenance","inactive","archived"]},
                "machines":{"type":"array","items":{"type":"string"}},
                "deploy_to":{"type":"array","items":{"type":"string"}},
                "category":{"type":["string","null"]},
                "description":{"type":["string","null"]},
                "gh_repo":{"type":["string","null"]},
                "cn_path":{"type":["string","null"]}
            }
        })),
        tool("list_areas", "List the areas under a project (by project name).", json!({
            "type":"object","additionalProperties":false,"required":["project"],
            "properties":{"project":{"type":"string","minLength":1}}
        })),
        tool("create_area", "Create an area under a project by name (idempotent — updates the description if it already exists). Returns its id.", json!({
            "type":"object","additionalProperties":false,"required":["project","name"],
            "properties":{
                "project":{"type":"string","minLength":1},
                "name":{"type":"string","minLength":1},
                "description":{"type":["string","null"]}
            }
        })),
    ]
}

fn empty() -> Value {
    json!({"type":"object","additionalProperties":false,"properties":{}})
}

fn tool(name: &'static str, desc: &'static str, schema: Value) -> Tool {
    Tool::new(name, desc, obj(schema))
}

fn obj(v: Value) -> JsonObject {
    match v {
        Value::Object(m) => m,
        _ => panic!("tool schema must be an object"),
    }
}

fn ok_json(v: Value) -> Result<CallToolResult, ErrorData> {
    let c = Content::json(v)
        .map_err(|e| ErrorData::internal_error(format!("failed to encode response: {e}"), None))?;
    Ok(CallToolResult::success(vec![c]))
}

/// Tool errors carry the same stable `code` as REST bodies (D-5), so an agent
/// can tell "you passed a bad value" from "korg is broken" without parsing prose.
fn to_err(e: impl ErrorClass + std::fmt::Display) -> CallToolResult {
    err_with_code(e.to_string(), e.code())
}

/// Not-found for a read that resolved to nothing — an `isError` result, not a
/// successful `null` (D-6).
fn not_found(message: String) -> CallToolResult {
    err_with_code(message, ErrorCode::NotFound)
}

fn err_with_code(message: String, code: ErrorCode) -> CallToolResult {
    CallToolResult::error(vec![Content::json(
        json!({ "message": message, "code": code.as_str() }),
    )
    .expect("encode error")])
}

fn parse_args<T: serde::de::DeserializeOwned>(args: Option<JsonObject>) -> Result<T, ErrorData> {
    let v = Value::Object(args.unwrap_or_default());
    serde_json::from_value(v)
        .map_err(|e| ErrorData::invalid_params(format!("invalid arguments: {e}"), None))
}

fn parse_date(s: &str) -> Result<Date, ErrorData> {
    let fmt = format_description!("[year]-[month]-[day]");
    Date::parse(s, &fmt)
        .map_err(|e| ErrorData::invalid_params(format!("invalid date `{s}`: {e}"), None))
}

/// Deserialize a nullable, optional field into `Option<Option<T>>` so callers can
/// distinguish "key absent" (leave unchanged) from "key present and null" (clear).
/// Paired with `#[serde(default)]`: absent -> `None`, `null` -> `Some(None)`,
/// value -> `Some(Some(v))`.
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

// --- argument shapes ------------------------------------------------------

fn default_task() -> String {
    "task".into()
}
fn default_open() -> String {
    "open".into()
}
fn default_unknown() -> String {
    "Unknown".into()
}
fn default_backlog() -> String {
    "Backlog".into()
}

#[derive(Deserialize)]
struct CreateWorkItemArgs {
    title: String,
    content: String,
    #[serde(default = "default_task")]
    wi_type: String,
    #[serde(default = "default_open")]
    wi_status: String,
    #[serde(default = "default_unknown")]
    wi_tshirt: String,
    #[serde(default)]
    sprint: Option<String>,
    #[serde(default)]
    details: Option<String>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    area_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct WiNumberArgs {
    wi_number: i64,
}

#[derive(Deserialize)]
struct UpdateWorkItemArgs {
    wi_number: i64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    wi_type: Option<String>,
    #[serde(default)]
    wi_status: Option<String>,
    #[serde(default)]
    wi_tshirt: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    sprint: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    details: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    project_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    area_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    parent: Option<Option<i64>>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    category: Option<Option<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct UpdateCardArgs {
    node_id: i64,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    rank: Option<f64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    project_id: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    category: Option<Option<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct IdArgs {
    id: i64,
}

#[derive(Deserialize)]
struct UpdateCommentArgs {
    id: i64,
    body: String,
}

#[derive(Deserialize)]
struct UpdateProjectArgs {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    machines: Option<Vec<String>>,
    #[serde(default)]
    deploy_to: Option<Vec<String>>,
    #[serde(default)]
    category: Option<Option<String>>,
    #[serde(default)]
    description: Option<Option<String>>,
    #[serde(default)]
    gh_repo: Option<Option<String>>,
    #[serde(default)]
    cn_path: Option<Option<String>>,
}

#[derive(Deserialize)]
struct CreateProjectArgs {
    name: String,
}

#[derive(Deserialize)]
struct ProjectArgs {
    project: String,
}

#[derive(Deserialize)]
struct CreateAreaArgs {
    project: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Deserialize)]
struct AddCommentArgs {
    node_id: i64,
    body: String,
}

#[derive(Deserialize, Default)]
struct ListWorkItemsArgs {
    #[serde(default)]
    project: Option<String>,
}

fn default_survey_limit() -> i64 {
    50
}

#[derive(Deserialize)]
struct SurveyWorkItemsArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    wi_status: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default = "default_survey_limit")]
    limit: i64,
    #[serde(default)]
    offset: i64,
}

#[derive(Deserialize)]
struct CreateCardArgs {
    title: String,
    #[serde(default = "default_backlog")]
    status: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    rank: f64,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct CreateLinkArgs {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct MarkLinkReadArgs {
    node_id: i64,
    read: bool,
}

#[derive(Deserialize)]
struct RelateArgs {
    left: i64,
    right: i64,
    label: String,
}

#[derive(Deserialize)]
struct NodeIdArgs {
    node_id: i64,
}

#[derive(Deserialize)]
struct NeighborsArgs {
    node_id: i64,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Deserialize)]
struct DateRangeArgs {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct CreateTopicArgs {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct SearchTopicsArgs {
    query: String,
}

#[derive(Deserialize)]
struct UpdateTopicArgs {
    node_id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    category: Option<Option<String>>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ArchiveTopicArgs {
    node_id: i64,
    #[serde(default = "default_true")]
    archived: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct CreateDailyPlanItemArgs {
    source_node_id: i64,
    plan_date: String,
}

#[derive(Deserialize)]
struct CompletionArgs {
    node_id: i64,
    completed: bool,
}

#[derive(Deserialize)]
struct ReorderDailyPlanArgs {
    plan_date: String,
    node_ids: Vec<i64>,
}

#[derive(Deserialize)]
struct MoveDailyPlanItemArgs {
    node_id: i64,
    target_date: String,
    #[serde(default)]
    target_position: i32,
}

#[derive(Deserialize)]
struct HistoryArgs {
    from: String,
    to: String,
    #[serde(default)]
    source_node_id: Option<i64>,
}

#[derive(Deserialize)]
struct CreateReportArgs {
    source: String,
    report_date: String,
    status: String,
    summary: String,
    body: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    escalated: bool,
    #[serde(default)]
    finding_work_items: Vec<i64>,
}

#[derive(serde::Deserialize)]
struct ListReportsArgs {
    #[serde(default)]
    source: Option<String>,
    #[serde(default = "default_report_limit")]
    limit: i64,
}

fn default_report_limit() -> i64 {
    30
}

#[derive(serde::Deserialize)]
struct GetReportArgs {
    node_id: i64,
}

#[derive(serde::Deserialize)]
struct ProposeSprintArgs {
    title: String,
    summary: String,
    #[serde(default)]
    work_item_numbers: Vec<i64>,
    #[serde(default)]
    project_id: Option<i64>,
    #[serde(default)]
    rank: f64,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize, Default)]
struct ListProposalsArgs {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct UpdateProposalArgs {
    node_id: i64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    rank: Option<f64>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

// --- dispatch -------------------------------------------------------------

impl KorgServer {
    pub async fn call(
        &self,
        name: &str,
        args: Option<JsonObject>,
    ) -> Result<CallToolResult, ErrorData> {
        match name {
            "create_work_item" => {
                let a: CreateWorkItemArgs = parse_args(args)?;
                let new = NewWorkItem {
                    project_id: a.project_id,
                    area_id: a.area_id,
                    wi_type: a.wi_type,
                    wi_status: a.wi_status,
                    wi_tshirt: a.wi_tshirt,
                    sprint: a.sprint,
                    title: a.title,
                    content: a.content,
                    details: a.details,
                    category: a.category,
                    tags: a.tags,
                };
                match create_work_item(&self.pool, new).await {
                    Ok(item) => ok_json(serde_json::to_value(item).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_work_items" => {
                let a: ListWorkItemsArgs = parse_args(args)?;
                let res = match a.project {
                    Some(p) => repo::list_work_items_by_project(&self.pool, &p).await,
                    None => list_work_items(&self.pool).await,
                };
                match res {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "survey_work_items" => {
                let a: SurveyWorkItemsArgs = parse_args(args)?;
                let limit = a.limit.clamp(1, 500);
                let offset = a.offset.max(0);
                match repo::survey_work_items(
                    &self.pool,
                    a.project.as_deref(),
                    a.wi_status.as_deref(),
                    a.archived,
                    limit,
                    offset,
                )
                .await
                {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "get_work_item" => {
                let a: WiNumberArgs = parse_args(args)?;
                match repo::get_work_item_detail(&self.pool, a.wi_number).await {
                    Ok(Some(v)) => ok_json(serde_json::to_value(v).unwrap()),
                    Ok(None) => Ok(not_found(format!("no work item #{}", a.wi_number))),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "update_work_item" => {
                let a: UpdateWorkItemArgs = parse_args(args)?;
                let patch = WorkItemPatch {
                    title: a.title,
                    content: a.content,
                    details: a.details,
                    wi_type: a.wi_type,
                    wi_status: a.wi_status,
                    wi_tshirt: a.wi_tshirt,
                    sprint: a.sprint,
                    project_id: a.project_id,
                    area_id: a.area_id,
                    parent: a.parent,
                    archived: a.archived,
                    category: a.category,
                    tags: a.tags,
                };
                match update_work_item(&self.pool, a.wi_number, patch).await {
                    Ok(item) => ok_json(serde_json::to_value(item).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_card" => {
                let a: CreateCardArgs = parse_args(args)?;
                let rank = Decimal::try_from(a.rank)
                    .map_err(|e| ErrorData::invalid_params(format!("invalid rank: {e}"), None))?;
                let new = NewCard {
                    project_id: a.project_id,
                    category: a.category,
                    tags: a.tags,
                    status: a.status,
                    title: a.title,
                    description: a.description,
                    rank,
                };
                match create_card(&self.pool, new).await {
                    Ok(card) => ok_json(serde_json::to_value(card).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "update_card" => {
                let a: UpdateCardArgs = parse_args(args)?;
                let rank = match a.rank {
                    Some(r) => Some(Decimal::try_from(r).map_err(|e| {
                        ErrorData::invalid_params(format!("invalid rank: {e}"), None)
                    })?),
                    None => None,
                };
                let patch = CardPatch {
                    status: a.status,
                    rank,
                    title: a.title,
                    description: a.description,
                    archived: a.archived,
                    project_id: a.project_id,
                    category: a.category,
                    tags: a.tags,
                };
                match repo::update_card(&self.pool, a.node_id, patch).await {
                    Ok(card) => ok_json(serde_json::to_value(card).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_cards" => match list_cards(&self.pool).await {
                Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                Err(e) => Ok(to_err(e)),
            },
            "list_comments" => {
                let a: NodeIdArgs = parse_args(args)?;
                match repo::list_comments(&self.pool, a.node_id).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "add_comment" => {
                let a: AddCommentArgs = parse_args(args)?;
                match repo::add_comment(&self.pool, a.node_id, &a.body).await {
                    Ok(c) => ok_json(serde_json::to_value(c).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "delete_comment" => {
                let a: IdArgs = parse_args(args)?;
                match repo::delete_comment(&self.pool, a.id).await {
                    Ok(deleted) => ok_json(json!({ "deleted": deleted })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "update_comment" => {
                let a: UpdateCommentArgs = parse_args(args)?;
                match repo::update_comment(&self.pool, a.id, &a.body).await {
                    Ok(c) => ok_json(serde_json::to_value(c).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_link" => {
                let a: CreateLinkArgs = parse_args(args)?;
                let new = NewLink {
                    project_id: a.project_id,
                    category: a.category,
                    tags: a.tags,
                    url: a.url,
                    title: a.title,
                };
                match create_link(&self.pool, new).await {
                    Ok(link) => ok_json(serde_json::to_value(link).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_links" => match list_links(&self.pool).await {
                Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                Err(e) => Ok(to_err(e)),
            },
            "mark_link_read" => {
                let a: MarkLinkReadArgs = parse_args(args)?;
                match repo::mark_link_read(&self.pool, a.node_id, a.read).await {
                    Ok(link) => ok_json(serde_json::to_value(link).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "relate" => {
                let a: RelateArgs = parse_args(args)?;
                match repo::relate(&self.pool, a.left, a.right, &a.label).await {
                    Ok(id) => ok_json(json!({ "id": id })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "neighbors" => {
                let a: NeighborsArgs = parse_args(args)?;
                match repo::neighbors(
                    &self.pool,
                    a.node_id,
                    repo::NeighborQuery {
                        label: a.label,
                        kind: a.kind,
                        limit: a.limit,
                    },
                )
                .await
                {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "unrelate" => {
                let a: IdArgs = parse_args(args)?;
                match repo::unrelate(&self.pool, a.id).await {
                    Ok(deleted) => ok_json(json!({ "deleted": deleted })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_topic" => {
                let a: CreateTopicArgs = parse_args(args)?;
                match topics::create_topic(
                    &self.pool,
                    topics::NewTopic {
                        project_id: a.project_id,
                        category: a.category,
                        tags: a.tags,
                        name: a.name,
                        description: a.description,
                    },
                )
                .await
                {
                    Ok(topic) => ok_json(serde_json::to_value(topic).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "get_topic" => {
                let a: NodeIdArgs = parse_args(args)?;
                match topics::get_topic(&self.pool, a.node_id).await {
                    Ok(Some(v)) => ok_json(serde_json::to_value(v).unwrap()),
                    Ok(None) => Ok(not_found(format!("no topic with node_id {}", a.node_id))),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_topics" => match topics::list_topics(&self.pool).await {
                Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                Err(e) => Ok(to_err(e)),
            },
            "search_topics" => {
                let a: SearchTopicsArgs = parse_args(args)?;
                match topics::search_topics(&self.pool, &a.query).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "update_topic" => {
                let a: UpdateTopicArgs = parse_args(args)?;
                match topics::update_topic(
                    &self.pool,
                    a.node_id,
                    topics::TopicPatch {
                        name: a.name,
                        description: a.description,
                        category: a.category,
                        tags: a.tags,
                    },
                )
                .await
                {
                    Ok(topic) => ok_json(serde_json::to_value(topic).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "archive_topic" => {
                let a: ArchiveTopicArgs = parse_args(args)?;
                match topics::archive_topic(&self.pool, a.node_id, a.archived).await {
                    Ok(topic) => ok_json(serde_json::to_value(topic).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_daily_plan" => {
                let a: DateRangeArgs = parse_args(args)?;
                let (from, to) = (parse_date(&a.from)?, parse_date(&a.to)?);
                match daily_plan::list_items(&self.pool, from, to).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_daily_plan_item" => {
                let a: CreateDailyPlanItemArgs = parse_args(args)?;
                let context = self
                    .config
                    .lifecycle_context()
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                match daily_plan::create_item(
                    &self.pool,
                    a.source_node_id,
                    parse_date(&a.plan_date)?,
                    &context,
                )
                .await
                {
                    Ok(item) => ok_json(serde_json::to_value(item).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "set_daily_plan_completion" => {
                let a: CompletionArgs = parse_args(args)?;
                let context = self
                    .config
                    .lifecycle_context()
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                match daily_plan::set_completion(&self.pool, a.node_id, a.completed, &context).await
                {
                    Ok(item) => ok_json(serde_json::to_value(item).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "delete_daily_plan_item" => {
                let a: NodeIdArgs = parse_args(args)?;
                let context = self
                    .config
                    .lifecycle_context()
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                match daily_plan::delete_item(&self.pool, a.node_id, &context).await {
                    Ok(()) => ok_json(json!({ "deleted": true })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "reorder_daily_plan" => {
                let a: ReorderDailyPlanArgs = parse_args(args)?;
                let context = self
                    .config
                    .lifecycle_context()
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                match daily_plan::reorder_day(
                    &self.pool,
                    parse_date(&a.plan_date)?,
                    &a.node_ids,
                    &context,
                )
                .await
                {
                    Ok(items) => ok_json(serde_json::to_value(items).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "move_daily_plan_item" => {
                let a: MoveDailyPlanItemArgs = parse_args(args)?;
                let context = self
                    .config
                    .lifecycle_context()
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                match daily_plan::move_item(
                    &self.pool,
                    a.node_id,
                    parse_date(&a.target_date)?,
                    a.target_position,
                    &context,
                )
                .await
                {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "daily_plan_history" => {
                let a: HistoryArgs = parse_args(args)?;
                let context = self
                    .config
                    .lifecycle_context()
                    .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
                match daily_plan::history(
                    &self.pool,
                    parse_date(&a.from)?,
                    parse_date(&a.to)?,
                    a.source_node_id,
                    &context,
                )
                .await
                {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_report" => {
                let a: CreateReportArgs = parse_args(args)?;
                let fmt = time::macros::format_description!("[year]-[month]-[day]");
                let report_date = time::Date::parse(&a.report_date, &fmt).map_err(|e| {
                    ErrorData::invalid_params(format!("invalid report_date: {e}"), None)
                })?;
                let new = NewReport {
                    source: a.source,
                    report_date,
                    status: a.status,
                    summary: a.summary,
                    body: a.body,
                    model: a.model,
                    escalated: a.escalated,
                    findings: a.finding_work_items,
                };
                match upsert_report(&self.pool, new).await {
                    Ok(r) => ok_json(json!({
                        "node_id": r.node_id, "replaced": r.replaced,
                        "findings_linked": r.findings_linked
                    })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_reports" => {
                let a: ListReportsArgs = parse_args(args)?;
                match repo::list_reports(&self.pool, a.source.as_deref(), a.limit).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "get_report" => {
                let a: GetReportArgs = parse_args(args)?;
                match repo::get_report(&self.pool, a.node_id).await {
                    Ok(Some(v)) => ok_json(serde_json::to_value(v).unwrap()),
                    Ok(None) => Ok(not_found(format!("no report with node_id {}", a.node_id))),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "propose_sprint" => {
                let a: ProposeSprintArgs = parse_args(args)?;
                let rank = Decimal::try_from(a.rank)
                    .map_err(|e| ErrorData::invalid_params(format!("invalid rank: {e}"), None))?;
                let new = NewProposal {
                    project_id: a.project_id,
                    category: a.category,
                    tags: a.tags,
                    title: a.title,
                    summary: a.summary,
                    rank,
                    pinned: a.pinned,
                    covers: a.work_item_numbers,
                };
                match create_proposal(&self.pool, new).await {
                    Ok(r) => ok_json(serde_json::to_value(r).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_proposals" => {
                let a: ListProposalsArgs = parse_args(args)?;
                match repo::list_proposals(&self.pool, a.status.as_deref()).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "update_proposal" => {
                let a: UpdateProposalArgs = parse_args(args)?;
                let rank = match a.rank {
                    Some(r) => Some(Decimal::try_from(r).map_err(|e| {
                        ErrorData::invalid_params(format!("invalid rank: {e}"), None)
                    })?),
                    None => None,
                };
                let patch = ProposalPatch {
                    title: a.title,
                    summary: a.summary,
                    status: a.status,
                    rank,
                    pinned: a.pinned,
                    archived: a.archived,
                    tags: a.tags,
                };
                match repo::update_proposal(&self.pool, a.node_id, patch).await {
                    Ok(proposal) => ok_json(serde_json::to_value(proposal).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_projects" => match list_projects(&self.pool).await {
                Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                Err(e) => Ok(to_err(e)),
            },
            "update_project" => {
                let a: UpdateProjectArgs = parse_args(args)?;
                let patch = repo::ProjectPatch {
                    gh_repo: a.gh_repo,
                    cn_path: a.cn_path,
                    description: a.description,
                    status: a.status,
                    machines: a.machines,
                    deploy_to: a.deploy_to,
                    category: a.category,
                };
                match repo::update_project_by_name(&self.pool, &a.name, &patch).await {
                    Ok(project) => ok_json(serde_json::to_value(project).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_project" => {
                let a: CreateProjectArgs = parse_args(args)?;
                match repo::create_project(&self.pool, &a.name).await {
                    Ok(id) => ok_json(json!({ "id": id })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_areas" => {
                let a: ProjectArgs = parse_args(args)?;
                match repo::list_areas(&self.pool, &a.project).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "create_area" => {
                let a: CreateAreaArgs = parse_args(args)?;
                match repo::create_area(&self.pool, &a.project, &a.name, a.description.as_deref())
                    .await
                {
                    Ok(id) => ok_json(json!({ "id": id })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            other => Err(ErrorData::invalid_params(
                format!("unknown tool: {other}"),
                None,
            )),
        }
    }
}

impl ServerHandler for KorgServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("korg-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "korg MCP server — unified work items, cards, reading-list links, \
                  generalized relationships, topics, and source-linked daily planning, over Postgres. \
                  Mutations validate their target and return the updated entity; errors are isError \
                  results carrying {message, code} where code is one of invalid_input, not_found, \
                  conflict, internal.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(&request.name, request.arguments).await
    }
}
