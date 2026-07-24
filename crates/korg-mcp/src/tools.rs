//! MCP tool implementations backed by `korg-core`.
//!
//! Exposes work items, cards, reading-list links, generalized relationships,
//! topics, and source-linked daily planning to AI agents over MCP.
//!
//! Every tool's input schema is **derived** from the struct its handler
//! deserializes (WI #540). There are no hand-written `json!` schema literals
//! left: [`tool`] and [`tool2`] take the argument types as type parameters and
//! ask `schemars` for the schema, so a field added to a shared struct in
//! `korg-core::ops` reaches `tools/list` with zero hand-edits. The enum lists
//! inside those schemas come from `korg_core::vocab`, which is why the
//! `survey_work_items` archived-default lie and the drifted vocabularies the
//! review found (F-22) cannot recur.

use korg_core::config::KorgConfig;
use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::ops;
use korg_core::repo::{
    self, CardPatch, LinkPatch, NewCard, NewLink, NewProposal, NewReport, NewWorkItem,
    ProjectPatch, ProposalPatch, WorkItemPatch,
};
use korg_core::{daily_plan, topics};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ErrorData, Implementation, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use schemars::JsonSchema;
use serde::Serialize;
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

// --- schema derivation ------------------------------------------------------

/// `schemars`'s JSON Schema for `T`, normalized into the object rmcp wants.
///
/// Normalization drops the artefacts of *how* the schema was produced —
/// `$schema`, the struct's Rust name as `title`, and the struct's doc comment
/// as a root `description` (the tool's own description carries that) — and
/// pins `additionalProperties: false`, which korg's tool surface has always
/// advertised. `required` is sorted so the output is byte-stable across
/// compilers.
fn schema_of<T: JsonSchema>() -> JsonObject {
    let mut schema = match serde_json::to_value(schemars::schema_for!(T)) {
        Ok(Value::Object(m)) => m,
        other => panic!(
            "schema for {} is not an object: {other:?}",
            type_name::<T>()
        ),
    };
    for artefact in ["$schema", "title", "description"] {
        schema.remove(artefact);
    }
    schema.insert("type".into(), json!("object"));
    schema.insert("additionalProperties".into(), json!(false));
    let properties = schema.entry("properties").or_insert_with(|| json!({}));
    if let Value::Object(properties) = properties {
        for field in properties.values_mut() {
            tidy_default(field);
        }
    }
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    }
    schema
}

/// Two corrections to the `default` `schemars` reads off `#[serde(default)]`.
///
/// `Option<T>` fields default to `null`, which says nothing an omittable field
/// doesn't already say — drop it, and let the `schema_with` builder's own
/// default (page limits, for instance) stand. `Decimal` fields default to the
/// *string* `"0"` because that is how `rust_decimal` serializes, but the wire
/// type is a JSON number and the schema must advertise one.
fn tidy_default(field: &mut Value) {
    let Some(field) = field.as_object_mut() else {
        return;
    };
    if field.get("default").is_some_and(Value::is_null) {
        field.remove("default");
    }
    if field.get("type") == Some(&json!("number")) {
        if let Some(numeric) = field
            .get("default")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<f64>().ok())
        {
            // A whole number stays whole: `0`, not `0.0`.
            let numeric = if numeric.fract() == 0.0 && numeric.abs() < i64::MAX as f64 {
                json!(numeric as i64)
            } else {
                json!(numeric)
            };
            field.insert("default".into(), numeric);
        }
    }
}

fn type_name<T>() -> &'static str {
    std::any::type_name::<T>()
}

/// Merge an id selector's schema into an operation body's schema.
///
/// MCP carries the target id in the same flat argument object as the body,
/// while REST carries it in the path — so the MCP schema is the union of the
/// two derived schemas rather than a third hand-written shape.
fn merge(mut sel: JsonObject, body: JsonObject) -> JsonObject {
    let props = merged_map(sel.remove("properties"), body.get("properties").cloned());
    let mut required = merged_list(sel.remove("required"), body.get("required").cloned());
    required.sort_by(|a, b| a.as_str().cmp(&b.as_str()));

    let mut out = sel;
    out.insert("properties".into(), Value::Object(props));
    if required.is_empty() {
        out.remove("required");
    } else {
        out.insert("required".into(), Value::Array(required));
    }
    out
}

fn merged_map(a: Option<Value>, b: Option<Value>) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for src in [a, b].into_iter().flatten() {
        if let Value::Object(m) = src {
            out.extend(m);
        }
    }
    out
}

fn merged_list(a: Option<Value>, b: Option<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for src in [a, b].into_iter().flatten() {
        if let Value::Array(items) = src {
            for item in items {
                if !out.contains(&item) {
                    out.push(item);
                }
            }
        }
    }
    out
}

/// A tool whose whole argument object is `A`.
fn tool<A: JsonSchema>(name: &'static str, desc: &'static str) -> Tool {
    Tool::new(name, desc, schema_of::<A>())
}

/// A tool that takes an id selector `S` plus an operation body `B` — the two
/// halves the handler deserializes from the same object.
fn tool2<S: JsonSchema, B: JsonSchema>(name: &'static str, desc: &'static str) -> Tool {
    Tool::new(name, desc, merge(schema_of::<S>(), schema_of::<B>()))
}

/// Tools that take no arguments at all.
#[derive(JsonSchema)]
struct NoArgs {}

// --- tool descriptors -------------------------------------------------------

pub fn tools() -> Vec<Tool> {
    vec![
        tool::<NewWorkItem>("create_work_item", "Create a work item. Returns the created row (including node_id and the serial wi_number, which are the same number since the 0009 identity migration)."),
        tool::<ops::ListWorkItems>("list_work_items", "List work items as {items, total, limit, offset}, ordered by wi_number. Each row includes `comment_count` -- get_work_item to read the discussion. Pass `project` (name) to scope to one project; omit it for all projects, but prefer survey_work_items for cross-project sweeps because these rows carry full content and details. Archived items are EXCLUDED by default."),
        tool::<ops::SurveyWorkItems>("survey_work_items", "Slim, paginated work-item listing (wi_number, node_id, project, title, wi_type, wi_status, wi_tshirt, comment_count only -- no content/details). Use this instead of list_work_items for cross-project surveys, which can exceed tool-output limits at instance scale. Returns {items, total, limit, offset}."),
        tool::<ops::WiNumber>("get_work_item", "Fetch a single work item by its wi_number (isError with code `not_found` if there is none), with its comments inlined (up to 10; `comments_truncated:true` + `comment_count` signal a longer thread — page the tail via list_comments). Comments often hold the real payload (resolution rationale, decisions), so prefer this over list_work_items when you need the full state of one item."),
        tool2::<ops::WiNumber, WorkItemPatch>("update_work_item", "Partially update a work item by its wi_number; returns the updated row (isError with code `not_found` if the wi_number does not exist). Only the fields you pass are changed. Status lifecycle: open -> resolved (implemented; may still need a user test or PR) -> done (agent satisfied; terminal but visible in default lists) -> closed (reserved for Ken; hidden by default -- do not set unless directed). For nullable fields (project_id, details, sprint, area_id, parent, category) pass null to clear or omit to leave unchanged. Moving projects (project_id) clears an area that no longer belongs to the target project unless you pass a valid area_id in the same call."),
        tool::<NewCard>("create_card", "Create a kanban card. Returns the created card row."),
        tool2::<ops::NodeId, CardPatch>("update_card", "Partially update a kanban card by its node_id; returns the updated card (isError with code `not_found` if that node is missing or is not a card). Projects are addressed by `project_id` here and over REST alike (get ids from list_projects) -- REST used to take a project *name* and silently create it. Only the fields you pass are changed (move status/rank, edit title/description, archive, reassign project). For nullable fields (project_id, category) pass null to clear or omit to leave unchanged."),
        tool::<ops::ListCards>("list_cards", "List cards as {items, total, limit, offset}, ordered by status, then rank, then node_id. Each row includes `comment_count`. Archived cards are EXCLUDED by default."),
        tool::<ops::NodeId>("list_comments", "List the comments on a node (work item or card), oldest first."),
        tool2::<ops::NodeId, ops::CommentBody>("add_comment", "Add a comment to a node of any kind. Returns the created comment; isError with code `not_found` if the node does not exist."),
        tool::<ops::Id>("delete_comment", "Delete a comment by its id. Returns {deleted: bool} — false means there was no such comment."),
        tool2::<ops::Id, ops::CommentBody>("update_comment", "Edit a comment's body by its id (from list_comments). `created` is preserved; `updated` advances."),
        tool::<NewLink>("create_link", "Capture a reading-list URL. Returns the created link row."),
        tool::<ops::ListLinks>("list_links", "List reading-list links as {items, total, limit, offset}, ordered by node_id. Archived links are EXCLUDED by default."),
        tool2::<ops::NodeId, LinkPatch>("update_link", "Update a reading-list link in ONE transaction: disposition, read flag, tags -- any combination. This is how an agent records what it decided about a captured URL (migration 0004's intended workflow). Returns the updated link; isError `not_found` if the node is missing or is not a link; an invalid disposition changes nothing."),
        tool::<MarkLinkReadArgs>("mark_link_read", "DEPRECATED -- use update_link, which does this plus disposition and tags in one transaction. Marks a reading-list link read or unread; returns the updated link."),
        tool::<ops::Relate>("relate", "Create a relationship edge between any two nodes. The label reads left-to-right. The vocabulary is CLOSED -- these four labels and no others: `covers` (proposal -> work item), `finding` (report -> work item), `depends_on` (dependent -> dependency) are DIRECTED -- orientation is meaningful, and the reverse is a distinct edge (A depends_on B plus B depends_on A is a cycle, not a duplicate). `related-to` is UNDIRECTED -- orientation is stored but meaningless, so read it symmetrically. An unregistered label is invalid_input naming the registry and the near-miss; `covers`/`finding` also validate both endpoint kinds. Exact duplicates dedup, and relating the reverse of an undirected edge returns the existing one. Optionally pass `origin` -- self-reported provenance (e.g. your skill name); it is recorded, not verified. Both endpoints must exist (isError `not_found`) and must differ (isError `invalid_input` -- self-edges are rejected)."),
        tool2::<ops::NodeId, ops::Neighbors>("neighbors", "List the nodes linked to a node (any kind), with labels. Returns {items, total, limit, truncated}. Each item has `rel_id` (pass to `unrelate`), `direction` (\"out\" = the queried node is the edge's left, so the label reads queried->neighbor; \"in\" = the reverse) and `directed` -- when `directed` is false the label is registry-undirected (e.g. related-to) and you MUST treat the edge as symmetric, ignoring `direction`. Filter server-side with `label` and/or `kind` instead of pulling every edge: e.g. label=\"covers\", kind=\"workitem\" for a proposal's work items. Ordering is neighbor node_id then rel_id."),
        tool::<ops::Id>("unrelate", "Remove a relationship edge by its id (the `rel_id` from `neighbors`, or the id returned by `relate`). Returns {deleted: bool} — false means there was no such edge."),
        tool::<topics::NewTopic>("create_topic", "Create a reusable planning topic. Returns the created topic."),
        tool::<ops::NodeId>("get_topic", "Fetch a topic by node_id, including archived topics. isError with code `not_found` if there is none."),
        tool::<ops::ListTopics>("list_topics", "List topics as {items, total, limit, offset}, ordered by name. Pass `q` to match name/description. Each row includes `comment_count`. Archived topics are EXCLUDED by default."),
        tool::<ops::ListTopics>("search_topics", "Alias for list_topics with a `q` filter; same {items, total, limit, offset} shape. Prefer list_topics."),
        tool2::<ops::NodeId, topics::TopicPatch>("update_topic", "Partially update a topic; returns the updated topic."),
        tool2::<ops::NodeId, ops::ArchiveTopic>("archive_topic", "Archive or restore a topic; returns the updated topic."),
        tool::<ops::DateRange>("list_daily_plan", "List daily plan items in an inclusive date range with snapshots and current source titles."),
        tool::<ops::CreateDailyPlanItem>("create_daily_plan_item", "Plan a work item, card, or topic. Display is resolved and snapshotted server-side. Returns the created item."),
        tool2::<ops::NodeId, ops::SetCompletion>("set_daily_plan_completion", "Complete or uncomplete any daily plan item; timestamp is server-authoritative. Returns the updated item."),
        tool::<ops::NodeId>("delete_daily_plan_item", "Delete an item from an open day; past structure is frozen. Returns {deleted: true}."),
        tool2::<ReorderPlanDate, ops::ReorderDailyPlan>("reorder_daily_plan", "Replace the complete order for an open day. Returns the day in its new order."),
        tool2::<ops::NodeId, ops::MoveDailyPlanItem>("move_daily_plan_item", "Move an item to today/future. Open sources transfer; past sources copy and remain unchanged."),
        tool::<ops::HistoryRange>("daily_plan_history", "Return all complete and incomplete historical items plus completion totals/rate. End must be before local today."),
        tool::<NewProposal>("propose_sprint", "Propose a sprint: bundle a title + summary with the work items it covers, in one call. Returns the created proposal plus `covered` -- which of the given wi_numbers actually resolved. Numbers that do not resolve are dropped, so compare `covered` against your request."),
        tool::<NewReport>("create_report", "Create or replace the daily report for (source, report_date). Same-day re-runs REPLACE both the content and the finding set -- findings you omit are unlinked -- but keep the node_id (links/comments survive). `findings_linked` echoes the wi_numbers that resolved; numbers that do not resolve are dropped, so compare it against your request."),
        tool::<ops::ListReports>("list_reports", "List daily reports, newest first (summary fields only). Pass `source` to filter."),
        tool::<ops::NodeId>("get_report", "Fetch one report by node_id: full body plus linked finding work items. isError with code `not_found` if there is none."),
        tool::<ops::ListProposals>("list_proposals", "List sprint proposals: pinned first, then rank, then node_id (a stable order -- equal ranks no longer shuffle between calls). Each row carries `covered_count` and `comment_count`; call get_proposal for the covered work items themselves. Filter by `status` and/or `project` (name)."),
        tool::<ops::NodeId>("get_proposal", "Fetch one sprint proposal by node_id with everything needed to start it: the proposal fields, `covered` (the work items it covers -- wi_number, node_id, title, wi_status, wi_tshirt, project, comment_count -- ordered by wi_number), and inlined comments (up to 10, with `comments_truncated`). This replaces the old list_proposals + neighbors + list_work_items dance. isError with code `not_found` if there is none."),
        tool2::<ops::NodeId, ProposalPatch>("update_proposal", "Partially update a sprint proposal by its node_id; returns the updated proposal (isError with code `not_found` if that node is missing or is not a proposal). Only the fields you pass are changed. Use this for status transitions (proposed -> active -> done/declined), reordering (rank), pinning, or archiving."),
        tool::<NoArgs>("list_projects", "List projects, including metadata: status (active|maintenance|inactive|archived), machines (where the working copy lives), deploy_to (where it deploys), category."),
        tool::<ops::CreateProject>("create_project", "Create a project by name (idempotent — returns the existing id if it already exists). Returns its id."),
        tool2::<ops::ProjectName, ProjectPatch>("update_project", "Update a project's metadata by name (the name itself is immutable), returning the updated project: status (active|maintenance|inactive|archived), machines, deploy_to, category, description, gh_repo, cn_path. Omitted fields are unchanged."),
        tool::<ops::ProjectRef>("list_areas", "List the areas under a project (by project name)."),
        tool::<ops::CreateArea>("create_area", "Create an area under a project by name (idempotent — updates the description if it already exists). Returns its id."),
    ]
}

/// `mark_link_read` is the one deprecated tool with no core counterpart worth
/// sharing — it does what `update_link` does, worse.
#[derive(serde::Deserialize, JsonSchema)]
struct MarkLinkReadArgs {
    node_id: i64,
    read: bool,
}

/// `reorder_daily_plan` selects the day rather than a node.
#[derive(serde::Deserialize, JsonSchema)]
struct ReorderPlanDate {
    #[schemars(schema_with = "ops::schema::date")]
    plan_date: String,
}

// --- responses --------------------------------------------------------------

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

/// A repo result as a tool result: the entity on success, `{message, code}` on
/// failure. Every tool ends this way, over both `anyhow::Error` (repo) and
/// `PlanningError` (daily plan) — the two carry the same `code` classification.
fn respond<T, E>(r: Result<T, E>) -> Result<CallToolResult, ErrorData>
where
    T: Serialize,
    E: ErrorClass + std::fmt::Display,
{
    match r {
        Ok(v) => ok_json(serde_json::to_value(v).map_err(|e| {
            ErrorData::internal_error(format!("failed to encode response: {e}"), None)
        })?),
        Err(e) => Ok(to_err(e)),
    }
}

/// A single-item read: `None` becomes an `isError` not-found, never a
/// successful `null` (D-6).
fn respond_found<T: Serialize>(
    r: anyhow::Result<Option<T>>,
    missing: impl FnOnce() -> String,
) -> Result<CallToolResult, ErrorData> {
    match r {
        Ok(Some(v)) => respond::<T, anyhow::Error>(Ok(v)),
        Ok(None) => Ok(not_found(missing())),
        Err(e) => Ok(to_err(e)),
    }
}

// --- arguments --------------------------------------------------------------

fn parse_args<T: serde::de::DeserializeOwned>(args: Option<JsonObject>) -> Result<T, ErrorData> {
    let v = Value::Object(args.unwrap_or_default());
    serde_json::from_value(v)
        .map_err(|e| ErrorData::invalid_params(format!("invalid arguments: {e}"), None))
}

/// Deserialize one argument object into both halves it carries: the id
/// selector and the operation body. The body is the same type korg-api
/// deserializes from a request body, so there is exactly one definition of it;
/// neither half declares `deny_unknown_fields`, so each ignores the other's
/// keys.
fn parse_args2<S, B>(args: Option<JsonObject>) -> Result<(S, B), ErrorData>
where
    S: serde::de::DeserializeOwned,
    B: serde::de::DeserializeOwned,
{
    let v = Value::Object(args.unwrap_or_default());
    let invalid =
        |e: serde_json::Error| ErrorData::invalid_params(format!("invalid arguments: {e}"), None);
    let selector = serde_json::from_value(v.clone()).map_err(invalid)?;
    let body = serde_json::from_value(v).map_err(invalid)?;
    Ok((selector, body))
}

fn parse_date(s: &str) -> Result<Date, ErrorData> {
    let fmt = format_description!("[year]-[month]-[day]");
    Date::parse(s, &fmt)
        .map_err(|e| ErrorData::invalid_params(format!("invalid date `{s}`: {e}"), None))
}

// --- dispatch ---------------------------------------------------------------

impl KorgServer {
    fn context(&self) -> Result<daily_plan::LifecycleContext, ErrorData> {
        self.config
            .lifecycle_context()
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }

    pub async fn call(
        &self,
        name: &str,
        args: Option<JsonObject>,
    ) -> Result<CallToolResult, ErrorData> {
        let pool = &self.pool;
        match name {
            // --- work items ---
            "create_work_item" => {
                let new: NewWorkItem = parse_args(args)?;
                respond(repo::create_work_item(pool, new).await)
            }
            "list_work_items" => {
                let a: ops::ListWorkItems = parse_args(args)?;
                respond(repo::list_work_items(pool, a.into()).await)
            }
            "survey_work_items" => {
                let a: ops::SurveyWorkItems = parse_args(args)?;
                respond(
                    repo::survey_work_items(
                        pool,
                        a.project.as_deref(),
                        a.wi_status.as_deref(),
                        a.archived,
                        a.limit.clamp(1, repo::LIST_LIMIT_MAX),
                        a.offset.max(0),
                    )
                    .await,
                )
            }
            "get_work_item" => {
                let a: ops::WiNumber = parse_args(args)?;
                respond_found(repo::get_work_item_detail(pool, a.wi_number).await, || {
                    format!("no work item #{}", a.wi_number)
                })
            }
            "update_work_item" => {
                let (a, patch) = parse_args2::<ops::WiNumber, WorkItemPatch>(args)?;
                respond(repo::update_work_item(pool, a.wi_number, patch).await)
            }

            // --- cards ---
            "create_card" => {
                let new: NewCard = parse_args(args)?;
                respond(repo::create_card(pool, new).await)
            }
            "update_card" => {
                let (a, patch) = parse_args2::<ops::NodeId, CardPatch>(args)?;
                respond(repo::update_card(pool, a.node_id, patch).await)
            }
            "list_cards" => {
                let a: ops::ListCards = parse_args(args)?;
                respond(repo::list_cards(pool, a.into()).await)
            }

            // --- comments ---
            "list_comments" => {
                let a: ops::NodeId = parse_args(args)?;
                respond(repo::list_comments(pool, a.node_id).await)
            }
            "add_comment" => {
                let (a, b) = parse_args2::<ops::NodeId, ops::CommentBody>(args)?;
                respond(repo::add_comment(pool, a.node_id, &b.body).await)
            }
            "update_comment" => {
                let (a, b) = parse_args2::<ops::Id, ops::CommentBody>(args)?;
                respond(repo::update_comment(pool, a.id, &b.body).await)
            }
            "delete_comment" => {
                let a: ops::Id = parse_args(args)?;
                match repo::delete_comment(pool, a.id).await {
                    Ok(deleted) => ok_json(json!({ "deleted": deleted })),
                    Err(e) => Ok(to_err(e)),
                }
            }

            // --- reading-list links ---
            "create_link" => {
                let new: NewLink = parse_args(args)?;
                respond(repo::create_link(pool, new).await)
            }
            "list_links" => {
                let a: ops::ListLinks = parse_args(args)?;
                respond(repo::list_links(pool, a.into()).await)
            }
            "update_link" => {
                let (a, patch) = parse_args2::<ops::NodeId, LinkPatch>(args)?;
                respond(repo::update_link(pool, a.node_id, patch).await)
            }
            "mark_link_read" => {
                let a: MarkLinkReadArgs = parse_args(args)?;
                respond(repo::mark_link_read(pool, a.node_id, a.read).await)
            }

            // --- relationships ---
            "relate" => {
                let a: ops::Relate = parse_args(args)?;
                match repo::relate(pool, a.left, a.right, &a.label, a.origin.as_deref()).await {
                    Ok(id) => ok_json(json!({ "id": id })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "neighbors" => {
                let (a, q) = parse_args2::<ops::NodeId, ops::Neighbors>(args)?;
                respond(repo::neighbors(pool, a.node_id, q.into()).await)
            }
            "unrelate" => {
                let a: ops::Id = parse_args(args)?;
                match repo::unrelate(pool, a.id).await {
                    Ok(deleted) => ok_json(json!({ "deleted": deleted })),
                    Err(e) => Ok(to_err(e)),
                }
            }

            // --- topics ---
            "create_topic" => {
                let new: topics::NewTopic = parse_args(args)?;
                respond(topics::create_topic(pool, new).await)
            }
            "get_topic" => {
                let a: ops::NodeId = parse_args(args)?;
                respond_found(topics::get_topic(pool, a.node_id).await, || {
                    format!("no topic with node_id {}", a.node_id)
                })
            }
            // search_topics is list_topics with a `q` filter; both names stay
            // registered, one implementation (WI #534).
            "list_topics" | "search_topics" => {
                let a: ops::ListTopics = parse_args(args)?;
                respond(topics::list_topics(pool, a.into()).await)
            }
            "update_topic" => {
                let (a, patch) = parse_args2::<ops::NodeId, topics::TopicPatch>(args)?;
                respond(topics::update_topic(pool, a.node_id, patch).await)
            }
            "archive_topic" => {
                let (a, b) = parse_args2::<ops::NodeId, ops::ArchiveTopic>(args)?;
                respond(topics::archive_topic(pool, a.node_id, b.archived).await)
            }

            // --- daily planning ---
            "list_daily_plan" => {
                let a: ops::DateRange = parse_args(args)?;
                respond(
                    daily_plan::list_items(pool, parse_date(&a.from)?, parse_date(&a.to)?).await,
                )
            }
            "create_daily_plan_item" => {
                let a: ops::CreateDailyPlanItem = parse_args(args)?;
                let context = self.context()?;
                respond(
                    daily_plan::create_item(
                        pool,
                        a.source_node_id,
                        parse_date(&a.plan_date)?,
                        &context,
                    )
                    .await,
                )
            }
            "set_daily_plan_completion" => {
                let (a, b) = parse_args2::<ops::NodeId, ops::SetCompletion>(args)?;
                let context = self.context()?;
                respond(daily_plan::set_completion(pool, a.node_id, b.completed, &context).await)
            }
            "delete_daily_plan_item" => {
                let a: ops::NodeId = parse_args(args)?;
                let context = self.context()?;
                match daily_plan::delete_item(pool, a.node_id, &context).await {
                    Ok(()) => ok_json(json!({ "deleted": true })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "reorder_daily_plan" => {
                let (day, b) = parse_args2::<ReorderPlanDate, ops::ReorderDailyPlan>(args)?;
                let context = self.context()?;
                respond(
                    daily_plan::reorder_day(
                        pool,
                        parse_date(&day.plan_date)?,
                        &b.node_ids,
                        &context,
                    )
                    .await,
                )
            }
            "move_daily_plan_item" => {
                let (a, b) = parse_args2::<ops::NodeId, ops::MoveDailyPlanItem>(args)?;
                let context = self.context()?;
                respond(
                    daily_plan::move_item(
                        pool,
                        a.node_id,
                        parse_date(&b.target_date)?,
                        b.target_position,
                        &context,
                    )
                    .await,
                )
            }
            "daily_plan_history" => {
                let a: ops::HistoryRange = parse_args(args)?;
                let context = self.context()?;
                respond(
                    daily_plan::history(
                        pool,
                        parse_date(&a.from)?,
                        parse_date(&a.to)?,
                        a.source_node_id,
                        &context,
                    )
                    .await,
                )
            }

            // --- daily reports ---
            "create_report" => {
                let new: NewReport = parse_args(args)?;
                respond(repo::upsert_report(pool, new).await)
            }
            "list_reports" => {
                let a: ops::ListReports = parse_args(args)?;
                respond(repo::list_reports(pool, a.source.as_deref(), a.limit).await)
            }
            "get_report" => {
                let a: ops::NodeId = parse_args(args)?;
                respond_found(repo::get_report(pool, a.node_id).await, || {
                    format!("no report with node_id {}", a.node_id)
                })
            }

            // --- sprint proposals ---
            "propose_sprint" => {
                let new: NewProposal = parse_args(args)?;
                respond(repo::create_proposal(pool, new).await)
            }
            "list_proposals" => {
                let a: ops::ListProposals = parse_args(args)?;
                respond(repo::list_proposals(pool, a.into()).await)
            }
            "get_proposal" => {
                let a: ops::NodeId = parse_args(args)?;
                respond_found(repo::get_proposal_detail(pool, a.node_id).await, || {
                    format!("no proposal with node_id {}", a.node_id)
                })
            }
            "update_proposal" => {
                let (a, patch) = parse_args2::<ops::NodeId, ProposalPatch>(args)?;
                respond(repo::update_proposal(pool, a.node_id, patch).await)
            }

            // --- projects and areas ---
            "list_projects" => respond(repo::list_projects(pool).await),
            "create_project" => {
                let a: ops::CreateProject = parse_args(args)?;
                match repo::create_project(pool, &a.name).await {
                    Ok(id) => ok_json(json!({ "id": id })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "update_project" => {
                let (a, patch) = parse_args2::<ops::ProjectName, ProjectPatch>(args)?;
                respond(repo::update_project_by_name(pool, &a.name, &patch).await)
            }
            "list_areas" => {
                let a: ops::ProjectRef = parse_args(args)?;
                respond(repo::list_areas(pool, &a.project).await)
            }
            "create_area" => {
                let a: ops::CreateArea = parse_args(args)?;
                match repo::create_area(pool, &a.project, &a.name, a.description.as_deref()).await {
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
            .with_instructions(crate::server_instructions())
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
