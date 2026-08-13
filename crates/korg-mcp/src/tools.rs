//! MCP tool implementations backed by `korg-core`.
//!
//! Exposes work items, cards, reading-list links, generalized relationships,
//! proposals, programs, reports and handoffs to AI agents over MCP.
//!
//! Every tool's input schema is **derived** from the struct its handler
//! deserializes (WI #540). There are no hand-written `json!` schema literals
//! left: [`tool`] and [`tool2`] take the argument types as type parameters and
//! ask `schemars` for the schema, so a field added to a shared struct in
//! `korg-core::ops` reaches `tools/list` with zero hand-edits. The enum lists
//! inside those schemas come from `korg_core::vocab`, which is why the
//! `survey_work_items` archived-default lie and the drifted vocabularies the
//! review found (F-22) cannot recur.

use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::ops;
use korg_core::repo::{
    self, CardPatch, HandoffPatch, LinkPatch, NewCard, NewHandoff, NewLink, NewProgram,
    NewProposal, NewReport, NewSchedule, NewWorkItem, ProgramPatch, ProjectPatch, ProposalPatch,
    ReportSourcePatch, SchedulePatch, WorkItemPatch,
};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    Implementation, InitializeRequestParams, InitializeResult, JsonObject, ListToolsResult,
    PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::borrow::Cow;

/// The MCP revisions korg-mcp actually implements (sprint 058, korg:1215).
///
/// Spelled out rather than left to rmcp's default of
/// [`ProtocolVersion::KNOWN_VERSIONS`], because that default is a claim about
/// the **SDK**, and serving it makes korg assert things about itself that only
/// the SDK knows. That is not a hypothetical: it is exactly how kaed broke
/// (korg:1212) — an rmcp that knew `2026-07-28` made kaed advertise it, a
/// Claude Code ≥2.1.227 client took the echo at its word, validated
/// `tools/list` against that revision's schema, and dropped **all** tools while
/// the session stayed up. korg escaped only because its rmcp topped out at
/// `2025-11-25`; the 1.8 → 3.1 bump in this sprint armed the same trap, which
/// is why the list and the cache metadata below had to land together.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[ProtocolVersion] = &[
    ProtocolVersion::V_2024_11_05,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_11_25,
    ProtocolVersion::V_2026_07_28,
];

/// What korg answers when a client asks for a revision korg does not know —
/// its ceiling, and the thing a raw probe reads to learn korg's real revision.
///
/// Pinned to a korg constant rather than inherited from `ProtocolVersion::
/// LATEST` (kaed 015 D-3). `LATEST` moving underneath a server is the mechanism
/// by which this whole bug class arrives: through a dependency bump, with no
/// code change for anyone to review. Raising korg's ceiling should be an edit
/// to this line and the list above, made deliberately.
pub const FALLBACK_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V_2026_07_28;

/// How long a client may treat korg's tool catalogue as fresh (SEP-2549).
///
/// The catalogue is a compile-time constant — [`tools()`] builds the same list
/// on every call — so it cannot go stale while the process lives, and an hour
/// is an honest under-claim bounded by how often korg is redeployed.
pub const TOOLS_CACHE_TTL_MS: u64 = 3_600_000;

/// Whether a peer at `version` is owed the SEP-2549 cache metadata.
///
/// korg serves a newer revision than several of its clients ask for, so it owns
/// the translation in both directions: a `2025-11-25` peer is entitled to
/// `2025-11-25`'s shape, and rmcp only strips the neighbouring `resultType`
/// field for legacy peers — cache metadata is korg's to gate.
fn emits_cache_metadata(version: Option<&ProtocolVersion>) -> bool {
    version.is_some_and(|v| v.as_str() >= ProtocolVersion::V_2026_07_28.as_str())
}

#[derive(Clone)]
pub struct KorgServer {
    pub pool: PgPool,
}

impl KorgServer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
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

// (There was a `NoArgs` marker here for tools taking no arguments at all.
// `list_projects` was its last user until #828 gave it `status` and `detail`,
// so every tool now derives its schema from a real argument struct.)

// --- tool descriptors -------------------------------------------------------

pub fn tools() -> Vec<Tool> {
    vec![
        tool::<NewWorkItem>("create_work_item", "Create a work item. Returns the created row (including node_id and the serial wi_number, which are the same number since the 0009 identity migration)."),
        tool::<ops::ListWorkItems>("list_work_items", "The work-item list: lean rows (wi_number, node_id, project, title, wi_type, wi_status, wi_tshirt, comment_count -- no content/details), ordered by wi_number, as {items, total, limit, offset, omitted}. `omitted` is {closed, archived} -- the rows the defaults hid, so a narrowed list can never be mistaken for the whole corpus. Defaults: everything not terminal (`open`, `resolved`, `done`) and unarchived. Pass wi_status:\"all\" (or one status, including \"closed\") for the rest, `project` (name) to scope to one project. There is no detail flag: reading one item in full is get_work_item, which also inlines its comments and edges."),
        tool::<ops::WorkItemSelector>("get_work_item", "Fetch a single work item by `wi_number` OR `node_id` -- for a work item they are the SAME number (the 0009 identity migration), so pass whichever you are holding; passing both is invalid_input rather than a guess. isError with code `not_found` if there is none. Comments are inlined (up to 10; `comments_truncated:true` + `comment_count` signal a longer thread — fetch the whole thread with list_comments, which returns every comment in one unpaginated array). Comments often hold the real payload (resolution rationale, decisions), so prefer this over list_work_items when you need the full state of one item."),
        tool2::<ops::WiNumber, WorkItemPatch>("update_work_item", "Partially update a work item by its wi_number; returns the updated row (isError with code `not_found` if the wi_number does not exist). Only the fields you pass are changed. Status lifecycle: open -> resolved (implemented; may still need a user test or PR) -> done (agent satisfied; terminal but visible in default lists) -> closed (reserved for Ken; hidden from list_work_items unless you pass wi_status \"closed\" or \"all\" -- do not set unless directed). For nullable fields (project_id, details, sprint, area_id, parent, category) pass null to clear or omit to leave unchanged. Moving projects (project_id) clears an area that no longer belongs to the target project unless you pass a valid area_id in the same call."),
        tool::<NewCard>("create_card", "Create a kanban card. Returns the created card row."),
        tool2::<ops::NodeId, CardPatch>("update_card", "Partially update a kanban card by its node_id; returns the updated card (isError with code `not_found` if that node is missing or is not a card). Projects are addressed by `project_id` here and over REST alike (get ids from list_projects) -- REST used to take a project *name* and silently create it. Only the fields you pass are changed (move status/rank, edit title/description, archive, reassign project). For nullable fields (project_id, category) pass null to clear or omit to leave unchanged."),
        tool::<ops::ListCards>("list_cards", "List cards as {items, total, limit, offset}, ordered by status, then rank, then node_id. Each row includes `comment_count`. Archived cards are EXCLUDED by default."),
        tool::<ops::NodeId>("list_comments", "List the comments on a node (work item or card), oldest first."),
        tool2::<ops::NodeId, ops::CommentBody>("add_comment", "Add a comment to a node of any kind. Returns the created comment; isError with code `not_found` if the node does not exist."),
        tool::<ops::Id>("delete_comment", "Delete a comment by its id. Returns {deleted: bool} — false means there was no such comment."),
        tool2::<ops::Id, ops::CommentBody>("update_comment", "Edit a comment's body by its id (from list_comments). `created` is preserved; `updated` advances."),
        tool::<NewLink>("create_link", "Capture a reading-list URL. Returns the created link row."),
        tool::<ops::ListLinks>("list_links", "List reading-list links as {items, total, limit, offset}, ordered by node_id. Archived links are EXCLUDED by default. Each row carries `created` and `updated`, so capture recency is readable without a second call."),
        tool2::<ops::NodeId, LinkPatch>("update_link", "Update a reading-list link in ONE transaction: disposition, read flag, tags, archived -- any combination. This is how an agent records what it decided about a captured URL (migration 0004's intended workflow), and `archived` is its lifecycle end: the disposal list_links has always excluded by default. Returns the updated link; isError `not_found` if the node is missing or is not a link; an invalid disposition changes nothing."),
        tool::<ops::NodeId>("delete_link", "Hard-delete a captured URL -- the disposal for a link that was never real (a probe, a mistyped capture). For a link that WAS real, archive it with update_link instead; delete is irreversible. REFUSES with `conflict` if the link has any relationship edge or comment, naming what points at it -- anything referenced has a history, and resolving that reference is your decision, not the database's. Returns {deleted: bool} -- false means there was no such link."),
        tool::<MarkLinkReadArgs>("mark_link_read", "DEPRECATED -- use update_link, which does this plus disposition and tags in one transaction. Marks a reading-list link read or unread; returns the updated link."),
        tool::<ops::Relate>("relate", "Create a relationship edge between any two nodes. The label reads left-to-right. The vocabulary is CLOSED -- these eight labels and no others: `covers` (proposal -> work item), `includes` (program -> proposal), `finding` (report -> work item), `depends_on` (dependent -> dependency), `has_handoff` (node -> handoff; normally written by create_handoff, but relate attaches an existing handoff to another node), and `has_attachment` (node -> attachment: what makes an uploaded image BELONG to something -- an attachment with no such edge is `pending` and the sweeper deletes it 24h after upload, so writing this edge rescues an image and unrelating one lets it go) are DIRECTED -- orientation is meaningful, and the reverse is a distinct edge (A depends_on B plus B depends_on A is a cycle, not a duplicate). `related-to` (the two nodes are related) and `collides-with` (same contract / fold on landing -- the kfdc curator's mined collisions, korg #1002) are UNDIRECTED -- orientation is stored but meaningless, so read them symmetrically. An unregistered label is invalid_input naming the registry and the near-miss; `covers`, `includes`, `finding`, and `has_handoff` also validate endpoint kinds (`has_handoff`'s right end must be a handoff), and `covers` additionally requires BOTH ENDS IN THE SAME PROJECT -- a proposal bundles work from one project, and cross-project work belongs to a program, the layer above proposals (korg #968). `includes` is the layer where cross-project work is legal, so it has NO same-project rule; `depends_on` across projects likewise stays legal and is the normal case. Exact duplicates dedup, and relating the reverse of an undirected edge returns the existing one. Optionally pass `rank` -- the position of the right endpoint within the left one, which is how a program's slices are ordered; re-relating an existing edge WITH a rank moves it in place and keeps its provenance, and re-relating without one leaves the position alone (so reordering never needs unrelate + relate). Optionally pass `origin` -- self-reported provenance (e.g. your skill name); it is recorded, not verified. Both endpoints must exist (isError `not_found`) and must differ (isError `invalid_input` -- self-edges are rejected)."),
        tool2::<ops::NodeId, ops::Neighbors>("neighbors", "List the nodes linked to a node (any kind), with labels. Returns {items, total, limit, truncated}. Each item has `rel_id` (pass to `unrelate`), `direction` (\"out\" = the queried node is the edge's left, so the label reads queried->neighbor; \"in\" = the reverse) and `directed` -- when `directed` is false the label is registry-undirected (e.g. related-to) and you MUST treat the edge as symmetric, ignoring `direction`. Filter server-side with `label` and/or `kind` instead of pulling every edge: e.g. label=\"covers\", kind=\"workitem\" for a proposal's work items. Ordering is neighbor node_id then rel_id."),
        tool::<ops::Id>("unrelate", "Remove a relationship edge by its id (the `rel_id` from `neighbors`, or the id returned by `relate`). Returns {deleted: bool} — false means there was no such edge."),
        tool::<NewProposal>("propose_sprint", "Propose a sprint: bundle a title + summary with the work items it covers, in one call. `project` (or `project_id`) is REQUIRED, and every covered work item must be in it -- a proposal is a single-project bundle, so one from another project is invalid_input naming both projects, not a silent drop. Cross-project work is bundled by a program, the layer above proposals (korg #968). `summary` is a routing contract capped at 500 characters -- what the bundle is, why now, roughly how big -- and the analysis goes in `notes`, which is unbounded; an over-long summary is invalid_input, not a truncation. Returns the created proposal plus `covered` -- which of the given wi_numbers actually resolved. Numbers that do not resolve are dropped, so compare `covered` against your request."),
        tool::<NewReport>("create_report", "Create or replace the daily report for (source, report_date). Same-day re-runs REPLACE both the content and the finding set -- findings you omit are unlinked -- but keep the node_id (links/comments survive). `findings_linked` echoes the wi_numbers that resolved; numbers that do not resolve are dropped, so compare it against your request."),
        tool::<ops::ListReports>("list_reports", "List daily reports, newest first (summary fields only). Pass `source` to filter."),
        tool::<ops::NodeId>("get_report", "Fetch one report by node_id: full body plus linked finding work items. isError with code `not_found` if there is none."),
        tool::<ops::ListProposals>("list_proposals", "The sprint queue: pinned first, then rank, then node_id (a stable order -- equal ranks no longer shuffle between calls). Returns {items, omitted} -- NOT a bare array; `omitted` counts the rows the defaults hid ({done, declined, archived}), so a narrowed queue can never be mistaken for the whole corpus. Defaults: live proposals only (`proposed` + `active`), unarchived, and the lean projection -- node_id, title, status, project, rank, pinned, covered_count, comment_count, no `summary`. Pass status:\"all\" (or one status) for the rest, detail:\"full\" for every column including `summary` and `notes`. Reading one proposal is get_proposal, which also gives you the work items it covers."),
        tool::<ops::NodeId>("get_proposal", "Fetch one sprint proposal by node_id with everything needed to start it: the proposal fields (including `notes`, the full analysis the 500-character `summary` cannot hold), `covered` (the work items it covers -- wi_number, node_id, title, wi_status, wi_tshirt, project, comment_count -- ordered by wi_number), and inlined comments (up to 10, with `comments_truncated`). This replaces the old list_proposals + neighbors + list_work_items dance. isError with code `not_found` if there is none."),
        tool2::<ops::NodeId, ProposalPatch>("update_proposal", "Partially update a sprint proposal by its node_id; returns the updated proposal (isError with code `not_found` if that node is missing or is not a proposal). Only the fields you pass are changed. Use this for status transitions (proposed -> active -> done/declined), reordering (rank), pinning, or archiving. `summary` is capped at 500 characters (invalid_input above that) -- long-form analysis belongs in `notes`, which is unbounded; pass null to clear it."),
        tool::<NewProgram>("create_program", "Create a program -- the CROSS-project layer (korg #968). A program bundles sprint PROPOSALS, ordered, and is the sanctioned home for work that spans repos; a proposal itself is strictly single-project. It takes NO project: passing `project`/`project_id` is invalid_input, because a program filed under one repo rebuilds exactly the mis-routing that rule cured. Its `span` -- which repos it touches -- is DERIVED from the projects of the proposals it includes, so it cannot go stale when you add a slice. `slices` are proposal node_ids IN ORDER (first = slice 1), written as ranked `includes` edges; an id that is not a proposal is refused outright rather than dropped, because a program's slice list is its plan. `aim` is the routing contract capped at 500 characters -- analysis goes in `notes`, which is unbounded."),
        tool::<ops::NodeId>("get_program", "Fetch one program by node_id with the whole rollup, so a consumer NEVER crawls program -> proposals -> work items: the program fields, `span` (its derived projects), and `slices` -- the included proposals in program order, each with node_id, title, status, project, its position `rank`, `covered_count`, and per-status work-item counts (open/resolved/done/closed). Also carries inlined comments and the `related` block, where a `has_handoff` edge surfaces -- a program is precisely the kind that accrues one, and it is required context. isError with code `not_found` if there is none."),
        tool::<ops::ListPrograms>("list_programs", "List programs, pinned first then rank then node_id. Returns {items, omitted} -- NOT a bare array; `omitted` is {done, archived}, the rows the defaults hid. Defaults: live programs only (`active` + `holding`), unarchived. Pass status:\"all\" (or one status) for the rest. There is no detail flag and no project filter: a program has no project of its own (its `span` is derived and is on every row), and with a 500-character `aim` the full row is already the lean one. `holding` means started but nothing currently in flight -- it is the state a program spends most of its life in, and it is deliberately not collapsed into `active`."),
        tool2::<ops::NodeId, ProgramPatch>("update_program", "Partially update a program by its node_id; returns the updated program (isError `not_found` if that node is missing or is not a program). Only the fields you pass change: title, aim, notes, status (active -> holding -> done), rank, pinned, archived, tags. `aim` is capped at 500 characters. Slice membership and ORDER are not here -- use relate/unrelate with the `includes` label, passing `rank` to position a slice."),
        tool2::<ops::NodeId, ops::SetAwaiting>("set_awaiting", "Mark a node -- ANY kind: work item, proposal, program, card -- as awaiting Ken, or clear that marker. This is how an agent says \"this moves only when Ken acts\": a decision, an ops action no agent can perform, a review. Pass awaiting:true with a `note` saying what is being asked; pass awaiting:false to clear it, which you SHOULD do yourself once you have the answer rather than leaving an answered ask in the lane. Re-marking a node that is already awaiting KEEPS the original timestamp and only updates the note -- the age of an ask is what makes the lane useful, so it is not restarted. The marker is a real column, not a tag: tags are written wholesale and an unrelated tag edit would silently clear it. Clearing also happens automatically when Ken himself acts (a work item reaching `closed`, a proposal reaching `done`/`declined`, anything archived)."),
        tool::<ops::NoArgs>("list_awaiting", "Everything currently waiting on Ken, oldest ask first -- one call, every node kind. Each row carries node_id, kind, wi_number (work items), a resolved title, project, the node's OWN status, `awaiting_since` and `awaiting_note`, so a board renders the lane without a follow-up read per row. Archived nodes and ones in a state only Ken sets (closed work items, done/declined proposals, done programs) are excluded -- the lane holds live asks, never answered ones. Note that `resolved` and `done` work items DO appear: \"implemented, needs your user test\" is the canonical awaiting-Ken state."),
        tool::<ops::NoArgs>("get_board", "The whole board in ONE call (korg #970) -- what a dashboard needs, instead of the 17-call crawl the 2026-07-31 backlog review had to do. Takes NO arguments: it is one screen's state, and every filter it could offer is already decided by what the panels are. Returns {generated, active, queue, proposals_omitted, proposal_edges, blocked, programs, programs_omitted, awaiting, depth, reports, events, sources}. `active` and `queue` are the `active` and `proposed` proposals, pinned-first-then-rank, each with `summary`, a work-item rollup (covered_count + open/resolved/done/closed) so progress renders without a call per proposal, and `synopsis` -- the newest comment opening with the \u{27e6}curator\u{27e7} marker (korg #1003, the machine-curated line; null when none). `proposals_omitted` is the usual {done, declined, archived}. `proposal_edges` (korg #1003) are every edge whose BOTH endpoints are active/queue rows -- {left, right, label, directed (from the registry -- read undirected labels symmetrically), origin, created}, the read surface for edge provenance, which is how curated edges (origin \"kfdc-curator\") stay distinguishable from human ones. `programs` are the live programs, each with its ordered `slices` exactly as get_program returns them. `blocked` (korg #978) is the DETERMINISTIC half of Deconfliction: every UNMET depends_on holding up a live row, one entry per (row, blocker) pair -- {proposal, via, dependent, dependent_wi_number, blocker, blocker_kind, blocker_wi_number, blocker_title, blocker_project, blocker_status, sequenced_by}. `via` is \"proposal\" when the row itself carries the edge and \"covered\" when one of its covered work items does; both count. Unfinished is derived from the vocabulary per kind, so a `done` work item does NOT block (that is the completion split, not the list-visibility one). One hop only, and an archived blocker does not block -- a row held blocked forever by something nobody will ever do is worse than one that frees up. `sequenced_by` names the live program whose ordered slices ALREADY express this dependency (kfdc #1070), so a render can drop what Operations is drawing anyway instead of showing the same data twice; null when there is none. `events` (korg #977) is the ticker: the newest 20 status transitions, newest first -- {node_id, kind, wi_number, title, project, from_status, to_status, at}. A write that does not CHANGE the status produces no event, which is the whole point: `node.updated` advances on any edit, so a feed built from it would date a proposal by its last tag edit. The log starts at migration 0026 and was deliberately NOT backfilled (there was no honest history to backfill from), so an empty ticker means \"nothing has moved since the migration\", never \"nothing has ever moved\". `awaiting` is list_awaiting's lane, `depth` is per-project queue depth (every project, with its `status`), `reports` the newest 5, and `sources` (korg #950) is every reporting source with its `freshness` (fresh|stale|retired|unrated) and what it `asserts` (ok|attention|problem|unknown) -- anything not `fresh` asserts `unknown`, never a last known status, which is the whole point: a stale GREEN is how korg lied about the fleet for 11 days in July 2026. `sources` is deliberately UNCAPPED where `reports` is 5 -- a cap could push a stale source off the board behind fresher ones, which inverts the failure it exists to catch. There is deliberately NO counters block -- live proposals is active+queue, shipped is proposals_omitted.done, awaiting is awaiting.length, projects is depth filtered by status; a counter that can disagree with the list beside it is a bug. `generated` is Postgres's clock, the same one every timestamp here came from, so ages computed against it are right. For one proposal's covered items, one program's detail, or anything paged, use that entity's own read."),
        tool::<NewSchedule>("create_schedule", "Create a SCHEDULE -- a work-item template plus a cadence, for work that a DATE makes appear (korg #581). This is korg's answer to \"recheck this after the DST transition\" and \"the restore drill is due again\", neither of which could be filed before: prose on a done node is out of every default view, and a work item for something not due for three months makes the queue lie. `cadence` is once|weekly|monthly|quarterly|yearly -- `once` is not a special case, it is the cadence whose interval is ZERO, so its `anchor_at` simply IS its fire date. `anchor_at` is where the interval counts from: SEED IT with the real last-done date (a quarterly drill last run 2026-07-08 anchored there comes due 2026-10-08, not today). `anchor_mode` picks which event advances it -- `completed` (default; when the materialized work item reaches done/closed) or `created` (at materialization). `title` and `template` may use exactly these substitutions: {YEAR}, {MONTH}, {DAY}, {DATE}, {QUARTER}; any other {PLACEHOLDER} is invalid_input rather than rendering literally into a title months from now. Creating a schedule materializes NOTHING -- korg runs no scheduler; call materialize_schedule when the work starts."),
        tool::<ops::NodeId>("get_schedule", "Fetch one schedule by node_id: its template fields, the read-time `due` predicate with `due_at`, `preview_title` (what the title renders to RIGHT NOW, so you can see what materializing would create), `outstanding` (whether its last materialized item is still open), and `materialized` -- every work item it has ever produced, newest first, which is the answer to \"when was this drill actually run?\". Also inlined comments and the `related` block. isError with code `not_found` if there is none."),
        tool::<ops::ListSchedules>("list_schedules", "List schedules, SOONEST-DUE FIRST -- the first row is the next thing that wants doing. Returns {items, omitted} -- NOT a bare array; `omitted` is {done, archived, not_due}, and `not_due` is what a due_only read hid, so \"nothing is due\" is never confusable with \"I filtered it out\". Defaults: live schedules (`active` + `paused`), unarchived. Pass due_only:true for just what is due now. `due` is computed on every read, never stored -- a stored due flag is a cached predicate over a clock, which is exactly the thing that goes stale silently. A schedule is due when it is active, its interval has elapsed since `anchor_at`, AND its last materialized work item is not still open/resolved -- while that item is open, IT is the surface and the schedule does not compete with it."),
        tool2::<ops::NodeId, SchedulePatch>("update_schedule", "Partially update a schedule by node_id; returns the updated schedule (isError `not_found` if that node is missing or is not a schedule). Only the fields you pass change: title, template, notes, cadence, anchor_mode, anchor_at, status, wi_type, wi_tshirt, archived, tags. Two common moves: `status:\"paused\"` stops a schedule surfacing without losing its anchor or its history, and setting `anchor_at` defers a drill (or records that you did the maintenance outside korg) without touching its cadence."),
        tool::<ops::MaterializeSchedule>("materialize_schedule", "Turn a due schedule into a real work item -- the explicit write that stands in for the daily tick korg deliberately does NOT run (#581/#950: building korg's answer to time on an unattended scheduled job would repeat the failure #950 exists to fix). Renders the template's substitutions from Postgres's clock, creates the work item in the schedule's project with its `wi_type` (default `maintenance`, so automation can find generated items as a group), writes a `materializes` edge for provenance, advances the anchor if `anchor_mode` is `created`, and marks a `once` schedule done. Refuses a schedule that is not due (pass force:true to bring one forward, e.g. a restore drill after a schema change) and ALWAYS refuses one whose last item is still open -- force does not lift that, because a second copy of a drill already in flight is not something you can want."),
        tool::<ops::NoArgs>("list_report_sources", "Every known report source and whether korg can currently BELIEVE it (korg #950) -- most-alarming first. A source that filed daily and then stopped is itself the alert: in July 2026 kmon crashed 1s into collection for 11 straight days and korg kept serving its last GREEN, so the fleet read healthy BECAUSE the thing checking it was broken. Each row carries `freshness` (fresh|stale|retired|unrated) and `asserts` (ok|attention|problem|unknown). THE RULE: anything not `fresh` asserts `unknown` -- never the last known status, because restating a stale GREEN is the exact failure this ends. There is deliberately no last_status field to reach for. `unrated` means fewer than 3 reports so no cadence can be inferred and none was declared; declare one with set_report_source to skip that state. Cadence is the median gap between a source's own report_dates unless declared; grace defaults to the cadence (floor 1 day), so a daily source goes stale on the third silent day."),
        tool2::<ops::SourceName, ReportSourcePatch>("set_report_source", "Declare a report source's expected cadence, its grace window, or its retirement (korg #950). Every field is an OVERRIDE -- omit one and korg keeps deriving it from the source's own report history; pass null to clear an override back to derivation. `retired:true` is how a deliberately-ended source stops nagging WITHOUT also silencing a broken one: it is an explicit declaration, so \"we turned this off\" stays distinguishable from \"this died\", which silence alone can never do. You may declare a source BEFORE its first report -- that is how a new daily source skips the `unrated` state on day one instead of spending three days in it. Returns the source's freshness row."),
        tool::<NewHandoff>("create_handoff", "Create a handoff -- a durable, cross-machine context document -- and attach it to the work it describes in ONE call. `related_node_ids` are the nodes it belongs to (work items, a sprint proposal, a report, another handoff); each becomes a `has_handoff` edge. REQUIRED context, not optional related reading: the handoff surfaces automatically in those nodes' get_work_item/get_proposal `related` block. Rejects an empty `related_node_ids` unless you pass `allow_standalone` (so a forgotten link can't orphan it), and rejects the whole create if any id does not resolve (isError `not_found` -- no partial insert). Returns the handoff plus the owner ids linked."),
        tool::<ops::NodeId>("get_handoff", "Fetch one handoff by node_id: title, summary, full Markdown `body`, and the nodes it is attached to (`related`, both directions, capped with `related_truncated`). isError with code `not_found` if there is none. This is the authoritative read once a `has_handoff` edge points you here."),
        tool2::<ops::NodeId, HandoffPatch>("update_handoff", "Partially update a handoff by its node_id; returns the updated handoff (isError with code `not_found` if that node is missing or is not a handoff). Only the fields you pass change (title, summary, body, tags, archived). Relationship changes go through relate/unrelate, not here."),
        tool::<ops::AttachmentSelector>("get_attachment", "Fetch one image attachment by `img_id` (e.g. \"img-c2a\", the display id as it appears in a markdown token) OR `node_id` -- they are the same number, hex and decimal, so pass whichever you are holding; passing both is invalid_input rather than a guess. Returns METADATA ONLY: filename, sniffed mime, byte size, pixel dimensions, content hash, the owning node ids, and a `url` per size. Bytes never travel over MCP -- to READ an image, GET the `agent` variant's url (<=1568px, Anthropic's vision ceiling; anything larger is downscaled and the extra bytes are wasted) to a temp file and read that file natively. `state` is `pending` (no owner yet -- swept 24h after upload) or `linked`, and it is DERIVED from the has_attachment edges on every read, so it cannot disagree with them. isError with code `not_found` if there is none. Every operation on the BYTES is REST, not MCP: upload with `curl -F file=@shot.png '<korg>/api/img?owner=<node_id>'`, and discard with `DELETE <korg>/api/img/<img-id>`, which is the only path that removes the record and the blobs together."),
        tool::<ops::NodeId>("list_attachments", "Every image attached to a node -- work item, card, proposal, anything -- oldest first, as a bare array with the same metadata get_attachment returns. get_work_item already inlines this for a work item, so reach for it when you hold some other kind of node, or after attaching. An image pasted into a COMMENT belongs to the node that comment is on: a korg comment is not a node and cannot own an edge, so the node owns the image and the comment body carries the placement token. A node with no images (or no such node) is an empty array, not an error -- this is a projection of a node's edges, and that node's own read is what says whether it exists."),
        tool::<ops::ListProjects>("list_projects","Answers \"which project does this work belong to?\". Returns {items, omitted:{archived}} — NOT a bare array; `omitted` is how you know rows were filtered out rather than absent. Each item is name + description (a one-line routing contract saying what belongs here and what does not) + status, the last only when it is not `active`. Defaults: active projects only, lean projection. Pass status:\"all\" to include archived (never a correct route target — their work went to a successor), detail:\"full\" for every column including machines, src_path, gh_repo, category, notes and the created/updated timestamps."),
        tool::<ops::ProjectName>("get_project", "Fetch one project by name: every column — including `notes`, the long-form operational context the lean list omits, and `created`/`updated`, which say how stale this project's routing metadata is — plus the areas under it, so a focused read needs no follow-up list_areas. isError with code `not_found` if there is none."),
        tool::<ops::CreateProject>("create_project", "Create a project by name (idempotent — returns the existing id if it already exists). Returns its id."),
        tool2::<ops::ProjectName, ProjectPatch>("update_project", "Update a project's metadata by name (the name itself is immutable), returning the updated project: status (active|maintenance|inactive|archived), machines, deploy_to, category, description, notes, gh_repo, src_path. Omitted fields are unchanged. `description` is capped at 160 characters — it is a routing line, not a blurb; put anything longer in `notes`."),
        tool::<ops::ProjectRef>("list_areas", "List the areas under a project (by project name), each with its `description` — what the area is for. get_project inlines the same rows, so a focused project read needs no follow-up call."),
        tool::<ops::CreateArea>("create_area", "Create an area under a project by name (idempotent — updates the description if it already exists). Returns its id."),
        tool::<ops::UpdateArea>("update_area", "Rename an area (`new_name`) and/or re-describe it (`description`; null clears), selected by project + current name. Returns the updated row. Work items keep pointing at the same area through a rename. isError `not_found` if no such area, `conflict` if the new name is already taken in that project."),
        tool::<ops::AreaRef>("delete_area", "Delete an area by project + name. An area is a label, not a record with a history, so delete is its lifecycle end — there is no archive. REFUSES with `conflict` naming the count if any work item is still filed under it; move those off first, because deleting would silently unfile them. Returns {deleted: bool} — false means there was no such area."),
    ]
}

/// `mark_link_read` is the one deprecated tool with no core counterpart worth
/// sharing — it does what `update_link` does, worse.
#[derive(serde::Deserialize, JsonSchema)]
struct MarkLinkReadArgs {
    node_id: i64,
    read: bool,
}

// --- responses --------------------------------------------------------------

fn ok_json(v: Value) -> Result<CallToolResult, ErrorData> {
    let c = ContentBlock::json(v)
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
    CallToolResult::error(vec![ContentBlock::json(
        json!({ "message": message, "code": code.as_str() }),
    )
    .expect("encode error")])
}

/// A repo result as a tool result: the entity on success, `{message, code}` on
/// failure. Every tool ends this way.
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

// --- dispatch ---------------------------------------------------------------

impl KorgServer {
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
                let (limit, offset) = repo::PageQuery {
                    limit: a.limit,
                    offset: a.offset,
                }
                .resolve_public();
                respond(
                    repo::list_work_items_lean(
                        pool,
                        a.project.as_deref(),
                        a.wi_status.as_deref(),
                        a.archived,
                        limit,
                        offset,
                    )
                    .await,
                )
            }
            "get_work_item" => {
                // Either spelling of the same number (WI #966) — the read every
                // agent reaches for should not turn on which key it happens to
                // be holding.
                let a: ops::WorkItemSelector = parse_args(args)?;
                match a.resolve() {
                    // An `isError` result carrying code `invalid_input`, the way
                    // every other refusal on this surface arrives — not a
                    // protocol-level error the agent has to read differently.
                    Err(e) => Ok(to_err(e)),
                    Ok(wi_number) => {
                        respond_found(repo::get_work_item_detail(pool, wi_number).await, || {
                            format!("no work item #{wi_number}")
                        })
                    }
                }
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
            "delete_link" => {
                let a: ops::NodeId = parse_args(args)?;
                match repo::delete_link(pool, a.node_id).await {
                    Ok(deleted) => ok_json(json!({ "deleted": deleted })),
                    Err(e) => Ok(to_err(e)),
                }
            }
            "mark_link_read" => {
                let a: MarkLinkReadArgs = parse_args(args)?;
                respond(repo::mark_link_read(pool, a.node_id, a.read).await)
            }

            // --- relationships ---
            "relate" => {
                let a: ops::Relate = parse_args(args)?;
                match repo::relate(pool, a.left, a.right, &a.label, a.origin.as_deref(), a.rank)
                    .await
                {
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
                let (status, project) = (a.status.as_deref(), a.project.as_deref());
                match a.detail.as_deref() {
                    Some("full") => {
                        respond(repo::list_proposals_full(pool, status, project, a.archived).await)
                    }
                    _ => {
                        respond(repo::list_proposals_lean(pool, status, project, a.archived).await)
                    }
                }
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

            // --- programs (#968) ---
            "create_program" => {
                let new: NewProgram = parse_args(args)?;
                respond(repo::create_program(pool, new).await)
            }
            "get_program" => {
                let a: ops::NodeId = parse_args(args)?;
                respond_found(repo::get_program_detail(pool, a.node_id).await, || {
                    format!("no program with node_id {}", a.node_id)
                })
            }
            "list_programs" => {
                let a: ops::ListPrograms = parse_args(args)?;
                respond(repo::list_programs(pool, a.status.as_deref(), a.archived).await)
            }
            "update_program" => {
                let a: ops::NodeId = parse_args(args.clone())?;
                let patch: ProgramPatch = parse_args(args)?;
                respond(repo::update_program(pool, a.node_id, patch).await)
            }

            // --- the awaiting-Ken marker (#969) ---
            "set_awaiting" => {
                let a: ops::NodeId = parse_args(args.clone())?;
                let s: ops::SetAwaiting = parse_args(args)?;
                respond(repo::set_awaiting(pool, a.node_id, s.awaiting, s.note.as_deref()).await)
            }
            "list_awaiting" => respond(repo::list_awaiting(pool).await),

            // --- the board rollup (#970) ---
            "get_board" => respond(repo::board_rollup(pool).await),

            // --- schedules (#581) ---
            "create_schedule" => {
                let new: NewSchedule = parse_args(args)?;
                respond(repo::create_schedule(pool, new).await)
            }
            "get_schedule" => {
                let a: ops::NodeId = parse_args(args)?;
                respond_found(repo::get_schedule_detail(pool, a.node_id).await, || {
                    format!("no schedule with node_id {}", a.node_id)
                })
            }
            "list_schedules" => {
                let a: ops::ListSchedules = parse_args(args)?;
                respond(
                    repo::list_schedules(
                        pool,
                        a.status.as_deref(),
                        a.project.as_deref(),
                        a.due_only,
                        a.archived,
                    )
                    .await,
                )
            }
            "update_schedule" => {
                let a: ops::NodeId = parse_args(args.clone())?;
                let patch: SchedulePatch = parse_args(args)?;
                respond(repo::update_schedule(pool, a.node_id, patch).await)
            }
            "materialize_schedule" => {
                let a: ops::MaterializeSchedule = parse_args(args)?;
                respond(repo::materialize_schedule(pool, a.node_id, a.force).await)
            }

            // --- report-source staleness (#950) ---
            "list_report_sources" => respond(repo::list_report_sources(pool).await),
            "set_report_source" => {
                let a: ops::SourceName = parse_args(args.clone())?;
                let patch: ReportSourcePatch = parse_args(args)?;
                respond(repo::set_report_source(pool, &a.source, patch).await)
            }

            // --- handoffs ---
            "create_handoff" => {
                let new: NewHandoff = parse_args(args)?;
                respond(repo::create_handoff(pool, new).await)
            }
            "get_handoff" => {
                let a: ops::NodeId = parse_args(args)?;
                respond_found(repo::get_handoff(pool, a.node_id).await, || {
                    format!("no handoff with node_id {}", a.node_id)
                })
            }
            "update_handoff" => {
                let (a, patch) = parse_args2::<ops::NodeId, HandoffPatch>(args)?;
                respond(repo::update_handoff(pool, a.node_id, patch).await)
            }

            // --- attachments (#582/#1119) ---
            "get_attachment" => {
                let a: ops::AttachmentSelector = parse_args(args)?;
                match a.resolve() {
                    Err(e) => Ok(to_err(e)),
                    Ok(node_id) => respond_found(repo::get_attachment(pool, node_id).await, || {
                        format!("no attachment with node_id {node_id}")
                    }),
                }
            }
            "list_attachments" => {
                let a: ops::NodeId = parse_args(args)?;
                respond(repo::list_attachments(pool, a.node_id).await)
            }
            // There is deliberately no `delete_attachment` tool. Deleting an
            // image is a *bytes* operation — the record and the blobs have to
            // go together — and this surface has no store handle. A tool that
            // deleted only the row would strand the bytes forever, because the
            // sweeper finds orphans through the row that no longer exists.
            // `DELETE /api/img/<img-id>` is the one path that owns both, which
            // is the same REST-for-bytes split upload already lives on.

            // --- projects and areas ---
            "list_projects" => {
                let a: ops::ListProjects = parse_args(args)?;
                match a.detail.as_deref() {
                    Some("full") => {
                        respond(repo::list_projects_full(pool, a.status.as_deref()).await)
                    }
                    _ => respond(repo::list_projects_lean(pool, a.status.as_deref()).await),
                }
            }
            "get_project" => {
                let a: ops::ProjectName = parse_args(args)?;
                respond_found(repo::get_project_detail(pool, &a.name).await, || {
                    format!("no project named '{}'", a.name)
                })
            }
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
            "update_area" => {
                let a: ops::UpdateArea = parse_args(args)?;
                respond(
                    repo::update_area(
                        pool,
                        &a.project,
                        &a.name,
                        a.new_name.as_deref(),
                        a.description.as_ref().map(|d| d.as_deref()),
                    )
                    .await,
                )
            }
            "delete_area" => {
                let a: ops::AreaRef = parse_args(args)?;
                match repo::delete_area(pool, &a.project, &a.name).await {
                    Ok(deleted) => ok_json(json!({ "deleted": deleted })),
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
    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(SUPPORTED_PROTOCOL_VERSIONS)
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(FALLBACK_PROTOCOL_VERSION)
            .with_server_info(Implementation::new("korg-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(crate::server_instructions())
    }

    /// Override purely to render the project roster from live data (WI #674).
    ///
    /// `get_info` is synchronous, so the roster cannot be built there — but
    /// `initialize` returns a future, which is both why this override exists
    /// and why "rank at initialize" was implementable at all. Everything else
    /// is the default behaviour, `set_peer_info` included; drop that line and
    /// the peer never learns who connected.
    ///
    /// **Including the negotiation**, which rmcp 3.x added to the default
    /// `initialize` this override replaces. The Streamable-HTTP transport
    /// happens to re-negotiate on top of whatever a handler returns, so korg
    /// would answer correctly today without these two lines — but that is the
    /// transport's habit, not a contract, and inheriting a fix by luck is how
    /// the roster override quietly went stale in the first place. Repeated here
    /// so the handler is right on its own terms; `negotiate_protocol_version`
    /// is `pub(crate)` in rmcp, hence the open-coded equivalent.
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        let requested = request.protocol_version.clone();
        context.peer.set_peer_info(request);
        let mut info = self.get_info();
        if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
            info.protocol_version = requested;
        }
        info.instructions = Some(crate::server_instructions_with_roster(&self.pool).await);
        Ok(info)
    }

    /// The one result `2026-07-28` changes the shape of.
    ///
    /// SEP-2549 puts cache metadata on *paginated* results, and korg advertises
    /// tools only — `CallToolResult` carries none in any revision — so
    /// `tools/list` is the entire surface of the upgrade. `cacheScope: public`
    /// is not a shrug: the catalogue is identical for every caller of a given
    /// build, so there is nothing user-specific to leak into a shared cache.
    ///
    /// korg has always hand-written this method (the `#[tool_handler]` macro
    /// generates it only when absent), so unlike kaed there was nothing to
    /// unpick here — just two fields to set.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let result = ListToolsResult {
            tools: tools(),
            ..Default::default()
        };
        Ok(
            if emits_cache_metadata(context.protocol_version().as_ref()) {
                result
                    .with_ttl_ms(TOOLS_CACHE_TTL_MS)
                    .with_cache_scope(CacheScope::Public)
            } else {
                result
            },
        )
    }

    /// Every korg tool completes in the request that made it: no elicitation,
    /// no SEP-2663 tasks. So the MRTR envelope rmcp 3.x introduced is always
    /// [`CallToolResponse::Complete`], and stripping `resultType` back off for a
    /// pre-2026-07-28 peer is rmcp's job, not ours
    /// (`ServerResult::strip_result_type_for_legacy_peer`).
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(self.call(&request.name, request.arguments).await?.into())
    }
}
