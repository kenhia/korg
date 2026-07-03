//! MCP tool implementations backed by `korg-core`.
//!
//! Exposes work items, cards, reading-list links, generalized relationships,
//! and calendar slots to AI agents over the MCP protocol.

use korg_core::repo::{
    self, create_card, create_link, create_proposal, create_work_item, list_cards, list_links,
    list_projects, list_work_items, update_work_item, CardPatch, NewCard, NewLink, NewProposal,
    NewWorkItem, ProposalPatch, WorkItemPatch,
};
use korg_core::slots::{self, NewTemplateSlot};
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
use time::macros::format_description;
use time::Date;

#[derive(Clone)]
pub struct KorgServer {
    pub pool: PgPool,
}

impl KorgServer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// --- tool descriptors -----------------------------------------------------

pub fn tools() -> Vec<Tool> {
    let tags = json!({"type":"array","items":{"type":"string","minLength":1}});
    let id = json!({"type":"integer","format":"int64"});
    let date = json!({"type":"string","description":"YYYY-MM-DD"});

    vec![
        tool("create_work_item", "Create a work item. Returns its node_id and serial wi_number.", json!({
            "type":"object","additionalProperties":false,
            "required":["title","content"],
            "properties":{
                "title":{"type":"string","minLength":1},
                "content":{"type":"string"},
                "wi_type":{"type":"string","default":"task"},
                "wi_status":{"type":"string","default":"open"},
                "wi_tshirt":{"type":"string","enum":["XS","S","M","L","XL","Huge","Unknown"],"default":"Unknown"},
                "sprint":{"type":["string","null"]},
                "details":{"type":["string","null"]},
                "project_id":id,
                "area_id":id,
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("list_work_items", "List work items. Pass `project` (name) to scope to a single project; omit it to list all.", json!({
            "type":"object","additionalProperties":false,
            "properties":{"project":{"type":["string","null"],"description":"Project name to filter by"}}
        })),
        tool("get_work_item", "Fetch a single work item by its wi_number.", json!({
            "type":"object","additionalProperties":false,"required":["wi_number"],
            "properties":{"wi_number":id}
        })),
        tool("update_work_item", "Partially update a work item by its wi_number. Only the fields you pass are changed. Use this to set status (e.g. resolve), edit fields, reparent, or archive. For nullable fields (details, sprint, area_id, parent, category) pass null to clear or omit to leave unchanged.", json!({
            "type":"object","additionalProperties":false,
            "required":["wi_number"],
            "properties":{
                "wi_number":id,
                "title":{"type":"string","minLength":1},
                "content":{"type":"string"},
                "wi_type":{"type":"string"},
                "wi_status":{"type":"string"},
                "wi_tshirt":{"type":"string","enum":["XS","S","M","L","XL","Huge","Unknown"]},
                "sprint":{"type":["string","null"]},
                "details":{"type":["string","null"]},
                "area_id":{"type":["integer","null"],"format":"int64"},
                "parent":{"type":["integer","null"],"format":"int64","description":"Parent work item's wi_number; null clears the parent."},
                "archived":{"type":"boolean"},
                "category":{"type":["string","null"]},
                "tags":tags
            }
        })),
        tool("create_card", "Create a kanban card. Returns its node_id.", json!({
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
        tool("update_card", "Partially update a kanban card by its node_id. Only the fields you pass are changed (move status/rank, edit title/description, archive, reassign project). For nullable fields (project_id, category) pass null to clear or omit to leave unchanged.", json!({
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
        tool("add_comment", "Add a comment to a node (work item or card). Returns the created comment.", json!({
            "type":"object","additionalProperties":false,"required":["node_id","body"],
            "properties":{"node_id":id,"body":{"type":"string","minLength":1}}
        })),
        tool("delete_comment", "Delete a comment by its id.", json!({
            "type":"object","additionalProperties":false,"required":["id"],
            "properties":{"id":id}
        })),
        tool("create_link", "Capture a reading-list URL. Returns its node_id.", json!({
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
        tool("mark_link_read", "Mark a reading-list link read or unread.", json!({
            "type":"object","additionalProperties":false,"required":["node_id","read"],
            "properties":{"node_id":id,"read":{"type":"boolean"}}
        })),
        tool("relate", "Create a generalized relationship edge between any two nodes.", json!({
            "type":"object","additionalProperties":false,"required":["left","right","label"],
            "properties":{"left":id,"right":id,"label":{"type":"string","minLength":1}}
        })),
        tool("neighbors", "List the nodes linked to a node (any kind), with labels. Each entry includes `rel_id`, the edge id to pass to `unrelate`.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{"node_id":id}
        })),
        tool("unrelate", "Remove a relationship edge by its id (the `rel_id` from `neighbors`, or the id returned by `relate`).", json!({
            "type":"object","additionalProperties":false,"required":["id"],
            "properties":{"id":id}
        })),
        tool("list_slots", "List calendar timebox slots between two dates (inclusive).", json!({
            "type":"object","additionalProperties":false,"required":["from","to"],
            "properties":{"from":date.clone(),"to":date.clone()}
        })),
        tool("generate_slots", "Materialize slots from the weekly template for N days starting at a date.", json!({
            "type":"object","additionalProperties":false,"required":["start","days"],
            "properties":{"start":date,"days":{"type":"integer","minimum":1}}
        })),
        tool("set_slot_goal", "Set (or clear) the small goal on a slot.", json!({
            "type":"object","additionalProperties":false,"required":["node_id"],
            "properties":{"node_id":id,"goal":{"type":["string","null"]}}
        })),
        tool("list_slot_templates", "List the editable weekly slot template.", empty()),
        tool("set_slot_template", "Replace the entire weekly slot template.", json!({
            "type":"object","additionalProperties":false,"required":["slots"],
            "properties":{"slots":{"type":"array","items":{
                "type":"object","additionalProperties":false,
                "required":["dow","position","duration_minutes"],
                "properties":{
                    "dow":{"type":"integer","minimum":0,"maximum":6},
                    "position":{"type":"integer"},
                    "duration_minutes":{"type":"integer","minimum":1},
                    "label":{"type":["string","null"]}
                }}}}
        })),
        tool("propose_sprint", "Propose a sprint: bundle a title + summary with the work items it covers, in one call. Returns the proposal's node_id and which of the given wi_numbers actually resolved to covered items.", json!({
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
        tool("list_proposals", "List sprint proposals, pinned first then by rank (the drag order a user or agent left them in). Pass `status` to filter.", json!({
            "type":"object","additionalProperties":false,
            "properties":{"status":{"type":["string","null"],"enum":["proposed","active","done","declined",null]}}
        })),
        tool("update_proposal", "Partially update a sprint proposal by its node_id. Only the fields you pass are changed. Use this for status transitions (proposed -> active -> done/declined), reordering (rank), pinning, or archiving.", json!({
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
        tool("list_projects", "List projects.", empty()),
        tool("create_project", "Create a project by name (idempotent — returns the existing id if it already exists). Returns its id.", json!({
            "type":"object","additionalProperties":false,"required":["name"],
            "properties":{"name":{"type":"string","minLength":1}}
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

fn to_err(e: anyhow::Error) -> CallToolResult {
    CallToolResult::error(vec![
        Content::json(json!({ "message": e.to_string() })).expect("encode error"),
    ])
}

fn parse_args<T: serde::de::DeserializeOwned>(args: Option<JsonObject>) -> Result<T, ErrorData> {
    let v = Value::Object(args.unwrap_or_default());
    serde_json::from_value(v)
        .map_err(|e| ErrorData::invalid_params(format!("invalid arguments: {e}"), None))
}

fn parse_date(s: &str) -> Result<Date, ErrorData> {
    let fmt = format_description!("[year]-[month]-[day]");
    Date::parse(s, &fmt).map_err(|e| ErrorData::invalid_params(format!("invalid date `{s}`: {e}"), None))
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

fn default_task() -> String { "task".into() }
fn default_open() -> String { "open".into() }
fn default_unknown() -> String { "Unknown".into() }
fn default_backlog() -> String { "Backlog".into() }

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
struct ListSlotsArgs {
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct GenerateSlotsArgs {
    start: String,
    days: i64,
}

#[derive(Deserialize)]
struct SetSlotGoalArgs {
    node_id: i64,
    #[serde(default)]
    goal: Option<String>,
}

#[derive(Deserialize)]
struct TemplateSlotArg {
    dow: i16,
    position: i32,
    duration_minutes: i32,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
struct SetSlotTemplateArgs {
    slots: Vec<TemplateSlotArg>,
}

#[derive(Deserialize)]
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
                    Ok(r) => ok_json(json!({"node_id": r.node_id, "wi_number": r.wi_number})),
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
            "get_work_item" => {
                let a: WiNumberArgs = parse_args(args)?;
                match repo::get_work_item(&self.pool, a.wi_number).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
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
                    area_id: a.area_id,
                    parent: a.parent,
                    archived: a.archived,
                    category: a.category,
                    tags: a.tags,
                };
                match update_work_item(&self.pool, a.wi_number, patch).await {
                    Ok(()) => ok_json(json!({ "ok": true })),
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
                    Ok(node_id) => ok_json(json!({ "node_id": node_id })),
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
                    Ok(()) => ok_json(json!({ "ok": true })),
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
                    Ok(()) => ok_json(json!({ "ok": true })),
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
                    Ok(node_id) => ok_json(json!({ "node_id": node_id })),
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
                    Ok(()) => ok_json(json!({ "ok": true })),
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
                let a: NodeIdArgs = parse_args(args)?;
                match repo::neighbors(&self.pool, a.node_id).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "unrelate" => {
                let a: IdArgs = parse_args(args)?;
                match repo::unrelate(&self.pool, a.id).await {
                    Ok(()) => ok_json(json!({ "ok": true })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_slots" => {
                let a: ListSlotsArgs = parse_args(args)?;
                let (from, to) = (parse_date(&a.from)?, parse_date(&a.to)?);
                match slots::list_slots(&self.pool, from, to).await {
                    Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "generate_slots" => {
                let a: GenerateSlotsArgs = parse_args(args)?;
                let start = parse_date(&a.start)?;
                match slots::generate_slots(&self.pool, start, a.days).await {
                    Ok(n) => ok_json(json!({ "created": n })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "set_slot_goal" => {
                let a: SetSlotGoalArgs = parse_args(args)?;
                match slots::set_slot_goal(&self.pool, a.node_id, a.goal.as_deref()).await {
                    Ok(()) => ok_json(json!({ "ok": true })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_slot_templates" => match slots::list_templates(&self.pool).await {
                Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                Err(e) => Ok(to_err(e)),
            },
            "set_slot_template" => {
                let a: SetSlotTemplateArgs = parse_args(args)?;
                let rows: Vec<NewTemplateSlot> = a
                    .slots
                    .into_iter()
                    .map(|t| NewTemplateSlot {
                        dow: t.dow,
                        position: t.position,
                        duration_minutes: t.duration_minutes,
                        label: t.label,
                    })
                    .collect();
                match slots::set_weekly_template(&self.pool, &rows).await {
                    Ok(()) => ok_json(json!({ "ok": true })),
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
                    Ok(r) => ok_json(json!({ "node_id": r.node_id, "covered": r.covered })),
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
                    Ok(()) => ok_json(json!({ "ok": true })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "list_projects" => match list_projects(&self.pool).await {
                Ok(v) => ok_json(serde_json::to_value(v).unwrap()),
                Err(e) => Ok(to_err(e)),
            },
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
                 generalized relationships, and calendar timebox slots, over Postgres.",
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
