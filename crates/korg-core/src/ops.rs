//! Operation shapes — the ONE definition of every request body korg accepts.
//!
//! Before this module the same shape was written three times: a patch struct in
//! korg-core, a request struct in korg-api, an arg struct in korg-mcp — each
//! with its own `double_option` copy, and a fourth hand-written copy as a
//! `json!` schema literal in the MCP tool table (F-22). Adding one field meant
//! editing four places and hoping.
//!
//! Now the serde-facing types live here (and, for the operations that already
//! had a natural home, directly on the core `New*`/`*Patch` structs in
//! [`crate::repo`]). Both transports deserialize the
//! *same* types, and the MCP input schemas are derived from them via
//! [`schemars`] — including the enum lists, which come from [`crate::vocab`],
//! so a vocabulary change reaches the tool schema with no hand-edit at all.
//!
//! What deliberately does NOT live here: the REST query-string filter structs.
//! A query string cannot carry a JSON `null`, so REST spells the tri-state
//! `archived` filter as `"true" | "false" | "all"` while MCP spells it as a
//! nullable boolean. Those are genuinely different wire shapes; both funnel
//! into the same `repo::*Query` type, which is where the sharing belongs.

use rust_decimal::Decimal;
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer};

use crate::repo::{self, archived_default, ArchivedFilter, PageQuery};

/// Deserialize a nullable, optional field into `Option<Option<T>>` so callers
/// can distinguish "key absent" (leave unchanged) from "key present and null"
/// (clear). Paired with `#[serde(default)]`: absent -> `None`, `null` ->
/// `Some(None)`, value -> `Some(Some(v))`.
///
/// This is *the* copy. korg-api's `deser_nullable_str`/`deser_nullable_i64` and
/// korg-mcp's `double_option` were three implementations of it.
pub fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

// --- serde defaults (shared by both transports) ----------------------------

pub(crate) fn default_task() -> String {
    "task".into()
}
pub(crate) fn default_open() -> String {
    "open".into()
}
pub(crate) fn default_unknown() -> String {
    "Unknown".into()
}
pub(crate) fn default_backlog() -> String {
    "Backlog".into()
}
/// A schedule's default anchor style (#581). `completed` is the one the WI
/// listed first and the one maintenance is actually reasoned about in — "it has
/// been three months since we last *did* it".
pub(crate) fn default_completed() -> String {
    "completed".into()
}
/// What a materialised work item is filed as unless the schedule says otherwise
/// — the distinct type #581 asked for so automation can find generated items.
pub(crate) fn default_maintenance() -> String {
    "maintenance".into()
}
fn default_report_limit() -> i64 {
    30
}

// The documented defaults for the paging knobs. `serde` cannot use these — the
// fields are `Option`, because "absent" has to stay distinguishable — but the
// *schema* must still advertise what absent resolves to, so they are wired in
// through `#[schemars(default = …)]`. `repo::PageQuery::resolve` is what
// actually applies them.
fn documented_page_limit() -> i64 {
    repo::LIST_LIMIT_DEFAULT
}
fn documented_page_offset() -> i64 {
    0
}
fn documented_neighbor_limit() -> i64 {
    repo::NEIGHBOR_LIMIT_DEFAULT
}
fn documented_flow_days() -> i64 {
    repo::FLOW_DAYS_DEFAULT
}

// --- schema fragments ------------------------------------------------------

/// Field-schema builders for [`schemars`]'s `schema_with`.
///
/// `schemars` can derive a field's *structure* (name, type, nullability,
/// requiredness) but not korg's domain constraints. These builders supply
/// those, and they read the enum lists straight out of [`crate::vocab`] — the
/// point of the exercise. The hand-written schemas they replace had already
/// drifted from the vocabulary they claimed to describe.
pub mod schema {
    use super::*;
    use crate::vocab;
    use serde_json::Value;

    fn strings(values: &[&str]) -> Vec<Value> {
        values.iter().map(|v| Value::String((*v).into())).collect()
    }

    /// `{"type":"string","enum":[…]}` — a required vocabulary field.
    fn enumerated(values: &[&str]) -> Schema {
        json_schema!({ "type": "string", "enum": strings(values) })
    }

    /// `{"type":["string","null"],"enum":[…,null]}` — a nullable vocabulary
    /// *filter*, where null means "no filter".
    fn nullable_enumerated(values: &[&str]) -> Schema {
        let mut variants = strings(values);
        variants.push(Value::Null);
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }

    /// What `search` may be narrowed to: every node kind that carries prose,
    /// plus `comment` — which is not a node kind but IS a document, and the one
    /// that most often holds the answer (#1177).
    ///
    /// Derived from `NODE_KINDS` rather than listed, minus `attachment`: an
    /// attachment is a filename and a mime type, so offering it as a filter
    /// would promise a search that can only ever return nothing.
    pub fn search_kind(_: &mut SchemaGenerator) -> Schema {
        let mut variants: Vec<Value> = vocab::NODE_KINDS
            .iter()
            .filter(|k| **k != "attachment")
            .map(|k| Value::String((*k).into()))
            .collect();
        variants.push(Value::String("comment".into()));
        variants.push(Value::Null);
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }

    /// `search` scope. Omitted means live rows only; `"all"` is the whole
    /// corpus, terminal and archived included.
    pub fn search_scope(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["string", "null"], "enum": ["live", "all", null] })
    }

    pub fn non_empty(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "minLength": 1 })
    }

    /// An optional string that must not be blank when it *is* given — an
    /// omitted rename is "leave it alone", an empty one is a mistake.
    pub fn non_empty_opt(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["string", "null"], "minLength": 1 })
    }

    pub fn tags(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "array", "items": { "type": "string", "minLength": 1 } })
    }

    pub fn wi_numbers(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "array", "items": { "type": "integer", "format": "int64" } })
    }

    pub fn node_ids(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "array", "items": { "type": "integer", "format": "int64" } })
    }

    pub fn wi_type(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::WI_TYPES)
    }
    pub fn wi_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::WI_STATUSES)
    }
    /// The `list_work_items` row filter (WI #861). The vocabulary plus `"all"`,
    /// which no single status names — omitting the argument means the
    /// non-terminal statuses, so there has to be a way to ask for `closed` rows
    /// alongside the rest. Same shape as `proposal_status_filter`, for the same
    /// reason.
    pub fn wi_status_filter(_: &mut SchemaGenerator) -> Schema {
        let mut variants = strings(&vocab::WI_STATUSES);
        variants.push(Value::String("all".into()));
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }
    pub fn wi_tshirt(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::WI_TSHIRTS)
    }
    pub fn card_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::CARD_STATUSES)
    }
    pub fn card_status_filter(_: &mut SchemaGenerator) -> Schema {
        nullable_enumerated(&vocab::CARD_STATUSES)
    }
    pub fn disposition(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::LINK_DISPOSITIONS)
    }
    pub fn disposition_filter(_: &mut SchemaGenerator) -> Schema {
        nullable_enumerated(&vocab::LINK_DISPOSITIONS)
    }
    pub fn proposal_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::PROPOSAL_STATUSES)
    }
    /// The `list_proposals` row filter (WI #852). The vocabulary plus `"all"`,
    /// which no single status names — omitting the argument means the live
    /// queue (`proposed` + `active`), so there has to be a way to ask for the
    /// rest. Same shape as `project_status_filter`, for the same reason.
    pub fn proposal_status_filter(_: &mut SchemaGenerator) -> Schema {
        let mut variants = strings(&vocab::PROPOSAL_STATUSES);
        variants.push(Value::String("all".into()));
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }
    /// `list_proposals` payload width (WI #852).
    pub fn proposal_detail(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["string", "null"], "enum": ["lean", "full"] })
    }
    pub fn program_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::PROGRAM_STATUSES)
    }
    /// The `list_programs` row filter (#968). Same shape and same reason as
    /// `proposal_status_filter`: omitting it means the live set
    /// (`queued` + `active` + `holding`), so `"all"` is the only way to reach
    /// `done`.
    pub fn program_status_filter(_: &mut SchemaGenerator) -> Schema {
        let mut variants = strings(&vocab::PROGRAM_STATUSES);
        variants.push(Value::String("all".into()));
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }
    pub fn report_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::REPORT_STATUSES)
    }
    /// An optional RFC 3339 instant. Unlike [`report_date`], which is a calendar
    /// day, a schedule anchor is a point in time — the interval arithmetic is
    /// done against `now()`, and a day-granular anchor would make "due" mean
    /// something different depending on the reader's timezone.
    pub fn timestamp_opt(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": ["string", "null"],
            "format": "date-time",
            "description": "RFC 3339 timestamp, e.g. 2026-07-08T00:00:00Z",
        })
    }
    pub fn schedule_cadence(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::SCHEDULE_CADENCES)
    }
    pub fn schedule_anchor(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::SCHEDULE_ANCHORS)
    }
    pub fn schedule_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::SCHEDULE_STATUSES)
    }
    /// The `list_schedules` row filter (#581). Same shape and same reason as
    /// `program_status_filter`: omitting it means the live set
    /// (`active` + `paused`), so `"all"` is the only way to reach `done`.
    pub fn schedule_status_filter(_: &mut SchemaGenerator) -> Schema {
        let mut variants = strings(&vocab::SCHEDULE_STATUSES);
        variants.push(Value::String("all".into()));
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }
    pub fn project_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::PROJECT_STATUSES)
    }
    /// The `list_projects` row filter (WI #828). The vocabulary plus `"all"`,
    /// which no single status names — omitting the argument means `active`, so
    /// there has to be a way to ask for the rest.
    pub fn project_status_filter(_: &mut SchemaGenerator) -> Schema {
        let mut variants = strings(&vocab::PROJECT_STATUSES);
        variants.push(Value::String("all".into()));
        json_schema!({ "type": ["string", "null"], "enum": variants })
    }
    /// `list_projects` payload width (WI #828).
    pub fn project_detail(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["string", "null"], "enum": ["lean", "full"] })
    }
    /// Nullable because null *clears* the category here rather than meaning
    /// "no filter": `create_project` takes only a name, so a project genuinely
    /// has none until someone sets it (WI #678).
    pub fn project_category(_: &mut SchemaGenerator) -> Schema {
        nullable_enumerated(&vocab::PROJECT_CATEGORIES)
    }

    /// Fractional rank. Arrives as a JSON number and is stored as a `Decimal`.
    pub fn rank(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "number" })
    }

    /// The same, where absent means "leave the position alone" rather than zero.
    pub fn rank_opt(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": ["number", "null"] })
    }

    pub fn date(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "description": "YYYY-MM-DD" })
    }

    /// `report_date`, which additionally advertises the `date` format.
    pub fn report_date(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "format": "date", "description": "YYYY-MM-DD" })
    }

    /// The tri-state `archived` filter every collection read shares (D-3).
    pub fn archived_filter(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": ["boolean", "null"],
            "description": "Omit for unarchived only (the default); true for archived only; null for both."
        })
    }

    pub fn limit(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "minimum": 1,
            "maximum": repo::LIST_LIMIT_MAX,
            "default": repo::LIST_LIMIT_DEFAULT
        })
    }

    pub fn offset(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "minimum": 0, "default": 0 })
    }

    pub fn neighbor_limit(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "minimum": 1,
            "maximum": repo::NEIGHBOR_LIMIT_MAX,
            "default": repo::NEIGHBOR_LIMIT_DEFAULT,
            "description": "Cap on returned edges; `truncated` says whether more matched."
        })
    }

    pub fn report_limit(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "default": super::default_report_limit() })
    }

    /// `work_item_flow`'s window (#1318). Bounded below only: the upper bound
    /// is the transition horizon, applied at read time as a clamp the response
    /// reports, never as a schema rejection.
    pub fn flow_days(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "minimum": 1,
            "default": repo::FLOW_DAYS_DEFAULT,
            "description": "How many days back from today, in the board's timezone. A window reaching past the transition horizon is clamped to the horizon — short series, never zero-filled days."
        })
    }
}

// --- operations with no core `New*`/`*Patch` counterpart --------------------
//
// Everything below is an operation whose payload never had a core struct to
// live on. The create/update payloads that DO have one (work items, cards,
// links, proposals, projects, reports) are the core types themselves —
// see `repo::NewWorkItem`, `repo::WorkItemPatch`, and friends.

/// `create_project` / `POST /api/projects`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateProject {
    #[schemars(schema_with = "schema::non_empty")]
    pub name: String,
}

/// `create_area` / `POST /api/areas`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateArea {
    #[schemars(schema_with = "schema::non_empty")]
    pub project: String,
    #[schemars(schema_with = "schema::non_empty")]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

/// `update_area` (WI #889). Areas are keyed by `(project, name)` like every
/// other name-selected entity, so `name` selects and `new_name` renames.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UpdateArea {
    #[schemars(schema_with = "schema::non_empty")]
    pub project: String,
    /// The area to update, by its current name.
    #[schemars(schema_with = "schema::non_empty")]
    pub name: String,
    /// Rename to this. Omit to leave the name alone.
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty_opt")]
    pub new_name: Option<String>,
    /// What the area is for; null clears it, omitted leaves it unchanged.
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
}

/// `delete_area` (WI #889) — the `(project, name)` selector, no payload.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AreaRef {
    #[schemars(schema_with = "schema::non_empty")]
    pub project: String,
    #[schemars(schema_with = "schema::non_empty")]
    pub name: String,
}

/// `list_areas` — and the `project` query parameter it shares with REST.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProjectRef {
    #[schemars(schema_with = "schema::non_empty")]
    pub project: String,
}

/// `list_projects` (WI #828). Both arguments are optional and their defaults
/// are the point: absent `status` means `active` — the rows that could be a
/// correct routing answer — and absent `detail` means the lean projection.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ListProjects {
    /// Which rows. Omit for `active`; `"all"` to include archived.
    #[serde(default)]
    #[schemars(schema_with = "schema::project_status_filter")]
    pub status: Option<String>,
    /// How wide. Omit for the lean routing projection (name, description, and
    /// status only when it is not `active`); `"full"` for every column,
    /// including `notes` and the `created`/`updated` timestamps.
    #[serde(default)]
    #[schemars(schema_with = "schema::project_detail")]
    pub detail: Option<String>,
}

/// `update_project` addresses a project by name; the name itself is immutable
/// (WI #246), so it is not part of [`repo::ProjectPatch`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProjectName {
    #[schemars(schema_with = "schema::non_empty")]
    pub name: String,
}

/// A comment body: `add_comment`, `update_comment`, and both REST equivalents.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommentBody {
    #[schemars(schema_with = "schema::non_empty")]
    pub body: String,
}

/// `relate` / `POST /api/relationships`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Relate {
    pub left: i64,
    pub right: i64,
    #[schemars(schema_with = "schema::non_empty")]
    pub label: String,
    /// Self-reported provenance (D-17): who is writing this edge — the web
    /// client sends `"web"`, a skill sends its name. Optional and unverified
    /// (korg is no-auth HTTP); absent means unknown, stored NULL.
    #[serde(default)]
    pub origin: Option<String>,
    /// Position of the right endpoint within the left one — used by `includes`
    /// to order a program's slices (#968). Lower sorts first; unranked edges
    /// sort last.
    ///
    /// Re-relating an existing edge **with** a rank moves it in place, keeping
    /// the edge's `created`/`origin`; re-relating **without** one leaves the
    /// position alone. That is the reorder path — `unrelate` + `relate` would
    /// work too but throws the provenance away.
    #[serde(default)]
    #[schemars(schema_with = "schema::rank_opt")]
    pub rank: Option<Decimal>,
}

// --- collection-read filters (MCP spelling) ---------------------------------
//
// REST spells these as query strings (see the module docs); both spellings
// resolve to the same `repo::*Query`.

/// `list_work_items` — the lean read since #861. Every argument is optional and
/// the two defaults are the point: absent `wi_status` means the non-terminal
/// statuses, absent `archived` means unarchived, and `omitted` reports both.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListWorkItems {
    /// Project name to filter by
    #[serde(default)]
    pub project: Option<String>,
    /// Which rows. Omit for everything not terminal (`open`, `resolved`,
    /// `done`, `parked`); `"all"` to include `closed` too; one status for
    /// exactly that.
    #[serde(default)]
    #[schemars(schema_with = "schema::wi_status_filter")]
    pub wi_status: Option<String>,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
    #[serde(default)]
    #[schemars(schema_with = "schema::limit", default = "documented_page_limit")]
    pub limit: Option<i64>,
    #[serde(default)]
    #[schemars(schema_with = "schema::offset", default = "documented_page_offset")]
    pub offset: Option<i64>,
}

/// `search` / `GET /api/search` (WI #1177). One required argument and four
/// narrowings; the ranking is not a knob (see `repo::search`).
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct Search {
    /// Plain terms. All terms are required; when that matches nothing the
    /// query is relaxed to any-term and the response says `relaxed: true`.
    /// Quoted `"exact phrases"` and a leading `-` to exclude both work, and a
    /// query carrying an exclusion is never relaxed.
    #[schemars(schema_with = "schema::non_empty")]
    pub q: String,
    /// Restrict to one document kind.
    #[serde(default)]
    #[schemars(schema_with = "schema::search_kind")]
    pub kind: Option<String>,
    /// Project name to filter by. A comment inherits its parent's project.
    #[serde(default)]
    pub project: Option<String>,
    /// Which rows. Omit for live ones only — each kind's own terminal state is
    /// excluded, matching what its list read hides. `"all"` searches
    /// everything, which is what you want when chasing a decision that was
    /// settled and closed.
    #[serde(default)]
    #[schemars(schema_with = "schema::search_scope")]
    pub scope: Option<String>,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
    #[serde(default)]
    #[schemars(schema_with = "schema::limit", default = "documented_page_limit")]
    pub limit: Option<i64>,
    #[serde(default)]
    #[schemars(schema_with = "schema::offset", default = "documented_page_offset")]
    pub offset: Option<i64>,
}

impl From<Search> for repo::SearchQuery {
    fn from(a: Search) -> Self {
        Self {
            q: a.q,
            kind: a.kind,
            project: a.project,
            scope: a.scope,
            archived: a.archived,
            page: PageQuery {
                limit: a.limit,
                offset: a.offset,
            },
        }
    }
}

/// `work_item_flow` / `GET /api/work-items/flow` (#1318). One optional knob —
/// the window. Everything else about the read is decided elsewhere: the
/// timezone is the server's, the horizon is the log's, and the durable
/// threshold is a constant the response reports.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema, Default)]
pub struct WorkItemFlow {
    #[serde(default)]
    #[schemars(schema_with = "schema::flow_days", default = "documented_flow_days")]
    pub days: Option<i64>,
}

// `survey_work_items` lived here as a deprecated alias of `list_work_items`
// (#861 folded the survey's projection into the list read; the alias kept the
// shipped `refill-queue` skill working through one deprecation window). #867
// moved that skill onto `list_work_items` and deployed it to all three hosts,
// which un-gated the deletion (#871). Three work-item tools are two again.

/// `list_cards`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListCards {
    #[serde(default)]
    #[schemars(schema_with = "schema::card_status_filter")]
    pub status: Option<String>,
    /// Project name to filter by
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
    #[serde(default)]
    #[schemars(schema_with = "schema::limit", default = "documented_page_limit")]
    pub limit: Option<i64>,
    #[serde(default)]
    #[schemars(schema_with = "schema::offset", default = "documented_page_offset")]
    pub offset: Option<i64>,
}

impl From<ListCards> for repo::CardQuery {
    fn from(a: ListCards) -> Self {
        Self {
            status: a.status,
            project: a.project,
            archived: a.archived,
            page: PageQuery {
                limit: a.limit,
                offset: a.offset,
            },
        }
    }
}

/// `list_links`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListLinks {
    #[serde(default)]
    #[schemars(schema_with = "schema::disposition_filter")]
    pub disposition: Option<String>,
    #[serde(default)]
    pub read: Option<bool>,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
    #[serde(default)]
    #[schemars(schema_with = "schema::limit", default = "documented_page_limit")]
    pub limit: Option<i64>,
    #[serde(default)]
    #[schemars(schema_with = "schema::offset", default = "documented_page_offset")]
    pub offset: Option<i64>,
}

impl From<ListLinks> for repo::LinkQuery {
    fn from(a: ListLinks) -> Self {
        Self {
            disposition: a.disposition,
            read: a.read,
            archived: a.archived,
            page: PageQuery {
                limit: a.limit,
                offset: a.offset,
            },
        }
    }
}

/// `list_programs` (#968). No `project` filter and no `detail` flag, unlike
/// `ListProposals`: a program carries no project (D-6, its span is derived), and
/// there are single digits of them with a 500-char `aim`, so the full row is
/// already lean.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListPrograms {
    #[serde(default)]
    #[schemars(schema_with = "schema::program_status_filter")]
    pub status: Option<String>,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
}

/// `list_schedules` (#581, sprint 051).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListSchedules {
    #[serde(default)]
    #[schemars(schema_with = "schema::schedule_status_filter")]
    pub status: Option<String>,
    /// Restrict to one project by name.
    #[serde(default)]
    pub project: Option<String>,
    /// Only schedules that are due **right now**. The whole read is a
    /// read-time predicate, so this is a filter rather than a stored flag —
    /// see `repo::SCHEDULE_DUE_SQL`.
    #[serde(default)]
    pub due_only: bool,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
}

/// `materialize_schedule` (#581, sprint 051).
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct MaterializeSchedule {
    pub node_id: i64,
    /// Materialize a schedule that is not yet due — e.g. a restore drill
    /// brought forward because the schema shape changed. Never lifts the
    /// outstanding-item check: a second copy of a drill whose first copy is
    /// still open is not something a caller can want.
    #[serde(default)]
    pub force: bool,
}

/// `set_report_source` (#950, sprint 051) — a source is addressed by name.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SourceName {
    #[schemars(schema_with = "schema::non_empty")]
    pub source: String,
}

/// A tool that takes no arguments. `list_awaiting` is deliberately one: #969
/// asked for a lane "one read can list", and every filter it could offer is
/// already decided by D-7 (never archived, never a state only Ken sets).
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct NoArgs {}

/// `set_awaiting` (#969) — the marker, settable and clearable by agents.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetAwaiting {
    /// `true` raises the marker, `false` clears it. Clearing is deliberately an
    /// agent-reachable operation (D-8): an agent that asked a question and got
    /// its answer in-session retracts its own marker rather than leaving an
    /// answered ask in the lane until Ken clicks it away.
    pub awaiting: bool,
    /// What is being asked of Ken — the thing that makes the lane actionable
    /// without opening every row. Only valid with `awaiting: true`; re-setting
    /// with a new note keeps the original `awaiting_since`, because the age of
    /// the ask is the point.
    #[serde(default)]
    pub note: Option<String>,
}

/// `list_proposals` (WI #852). Row filter and projection both narrow by
/// default — this read measured ~46k tokens unfiltered, 71% of it `done`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListProposals {
    #[serde(default)]
    #[schemars(schema_with = "schema::proposal_status_filter")]
    pub status: Option<String>,
    /// Project name to filter by
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "archived_default")]
    #[schemars(schema_with = "schema::archived_filter")]
    pub archived: ArchivedFilter,
    #[serde(default)]
    #[schemars(schema_with = "schema::proposal_detail")]
    pub detail: Option<String>,
}

impl From<ListProposals> for repo::ProposalQuery {
    fn from(a: ListProposals) -> Self {
        Self {
            status: a.status,
            project: a.project,
        }
    }
}

/// `list_reports`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListReports {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default = "default_report_limit")]
    #[schemars(schema_with = "schema::report_limit")]
    pub limit: i64,
}

/// `neighbors`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct Neighbors {
    /// Only edges with this relationship label.
    #[serde(default)]
    pub label: Option<String>,
    /// Only neighbors of this node kind, e.g. "workitem".
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    #[schemars(
        schema_with = "schema::neighbor_limit",
        default = "documented_neighbor_limit"
    )]
    pub limit: Option<i64>,
}

impl From<Neighbors> for repo::NeighborQuery {
    fn from(a: Neighbors) -> Self {
        Self {
            label: a.label,
            kind: a.kind,
            limit: a.limit,
        }
    }
}

// --- id selectors -----------------------------------------------------------
//
// MCP carries the target id inside the argument object; REST carries it in the
// path. These tiny structs are the MCP half, deserialized from the *same*
// object as the operation body (bodies do not `deny_unknown_fields`, so each
// pass ignores the other's key). That keeps one definition of the body without
// `serde(flatten)`'s buffering caveats.

/// `{node_id}` — the selector for every node-addressed tool.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct NodeId {
    pub node_id: i64,
}

/// `{wi_number}` — work items are addressed by their user-facing number.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct WiNumber {
    pub wi_number: i64,
}

/// `{wi_number}` **or** `{node_id}` — the same number either way (WI #966).
///
/// Fable passed `node_id: 965` to `get_work_item` and got
/// `missing field wi_number`. The call was right in every way that matters: the
/// 0009 identity migration made a work item's node id *equal* its `wi_number`,
/// so the agent was holding the number it needed and korg refused it on
/// spelling. Every other node-addressed tool takes `node_id`, so an agent that
/// has just read `related` or `covered` — both of which carry `node_id` — is
/// reaching for the key the rest of the surface taught it.
///
/// Both halves together is `invalid_input`, matching the project/area selectors
/// (#575) and for the same reason: korg does not pick between two ids the
/// caller explicitly passed. That the two *are* equal for a work item is not a
/// licence to guess — an unequal pair is a real contradiction, and a rule with
/// an exception is a rule nobody remembers.
#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
pub struct WorkItemSelector {
    #[serde(default)]
    pub wi_number: Option<i64>,
    /// The alternative spelling of `wi_number`, not a second identity: since
    /// the 0009 migration a work item's node id and `wi_number` are the same
    /// value. Pass one or the other, never both.
    #[serde(default)]
    pub node_id: Option<i64>,
}

impl WorkItemSelector {
    /// The `wi_number` to read, or `invalid_input` naming both spellings.
    pub fn resolve(self) -> anyhow::Result<i64> {
        match (self.wi_number, self.node_id) {
            (Some(_), Some(_)) => Err(crate::error::RepoError::InvalidInput(
                "pass either wi_number or node_id, not both — for a work item they are the \
                 same number (the 0009 identity migration), so either one alone is correct"
                    .into(),
            )
            .into()),
            (Some(n), None) | (None, Some(n)) => Ok(n),
            (None, None) => Err(crate::error::RepoError::InvalidInput(
                "pass wi_number (or node_id — for a work item they are the same number)".into(),
            )
            .into()),
        }
    }
}

/// `{img_id}` **or** `{node_id}` — an attachment, either way an agent is
/// holding it (sprint 056, #1119).
///
/// The two spellings come from two places and both are normal: `node_id` is
/// what a focused read hands you, and `img_id` (`img-c2a`) is what appears in a
/// markdown token in a work item's body — which is precisely the moment an
/// agent wants to look one up. They are the same number in different bases, so
/// refusing one spelling would be refusing on formatting, the mistake #966
/// fixed for work items.
///
/// Both at once is `invalid_input` for the same reason it is there: korg does
/// not pick between two ids the caller explicitly passed, and a rule with an
/// exception is a rule nobody remembers.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct AttachmentSelector {
    /// The display id, e.g. `img-c2a` — what a markdown token carries.
    #[serde(default)]
    #[schemars(schema_with = "schema::non_empty_opt")]
    pub img_id: Option<String>,
    /// The attachment's node id — the same number `img_id` spells in hex.
    #[serde(default)]
    pub node_id: Option<i64>,
}

impl AttachmentSelector {
    /// The node id to read, or `invalid_input` naming both spellings.
    pub fn resolve(&self) -> anyhow::Result<i64> {
        match (&self.img_id, self.node_id) {
            (Some(_), Some(_)) => Err(crate::error::RepoError::InvalidInput(
                "pass either img_id or node_id, not both — an attachment's img_id is its \
                 node_id in hex, so either one alone is correct"
                    .into(),
            )
            .into()),
            (Some(img), None) => Ok(korg_img::ImgId::parse(img)?.node_id()),
            (None, Some(id)) => Ok(id),
            (None, None) => Err(crate::error::RepoError::InvalidInput(
                "pass img_id (e.g. \"img-c2a\", as it appears in a markdown token) or node_id"
                    .into(),
            )
            .into()),
        }
    }
}

/// `{id}` — comments and relationship edges carry plain row ids.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
pub struct Id {
    pub id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vocab;

    /// The enum lists in the generated schemas are the vocabulary, not a copy
    /// of it. This is the drift fence F-22 asked for: change `vocab` and the
    /// tool schema changes with it or this fails.
    #[test]
    fn vocabulary_reaches_the_generated_schema() {
        let schema = schemars::schema_for!(repo::NewWorkItem);
        let v = serde_json::to_value(&schema).unwrap();
        let statuses = v["properties"]["wi_status"]["enum"].as_array().unwrap();
        assert_eq!(statuses.len(), vocab::WI_STATUSES.len());
        for s in vocab::WI_STATUSES {
            assert!(
                statuses.iter().any(|v| v == s),
                "wi_status enum is missing {s}"
            );
        }
        let types = v["properties"]["wi_type"]["enum"].as_array().unwrap();
        assert_eq!(types.len(), vocab::WI_TYPES.len());
    }

    /// `double_option` is what lets a patch say "clear this" instead of only
    /// "leave it alone" — the distinction three separate copies used to encode.
    #[test]
    fn absent_null_and_present_are_three_different_things() {
        let absent: repo::WorkItemPatch = serde_json::from_str("{}").unwrap();
        assert_eq!(absent.details, None);
        let cleared: repo::WorkItemPatch = serde_json::from_str(r#"{"details":null}"#).unwrap();
        assert_eq!(cleared.details, Some(None));
        let set: repo::WorkItemPatch = serde_json::from_str(r#"{"details":"x"}"#).unwrap();
        assert_eq!(set.details, Some(Some("x".into())));
    }

    /// `report_date` used to arrive as a `String` that each transport parsed by
    /// hand; it now deserializes straight to a `Date`. Nothing exercised the
    /// MCP `create_report` path, so this is its fence.
    #[test]
    fn report_date_deserializes_from_the_wire_format() {
        let raw = r#"{"source":"kmon","report_date":"2026-07-22","status":"ok",
                      "summary":"s","body":"b","finding_work_items":[7]}"#;
        let new: repo::NewReport = serde_json::from_str(raw).unwrap();
        assert_eq!(
            new.report_date,
            time::Date::from_calendar_date(2026, time::Month::July, 22).unwrap()
        );
        // `findings` is renamed on the wire; the rename is the contract.
        assert_eq!(new.findings, vec![7]);
        assert!(!new.escalated, "escalated defaults to false");

        let bad = r#"{"source":"k","report_date":"22-07-2026","status":"ok",
                      "summary":"s","body":"b"}"#;
        assert!(serde_json::from_str::<repo::NewReport>(bad).is_err());
    }

    /// `rank` arrives as a JSON number and is kept as an exact `Decimal`, so a
    /// fractional insert between two neighbours does not lose precision. Both
    /// transports used to take an `f64` and convert it themselves.
    #[test]
    fn rank_deserializes_from_a_json_number_without_loss() {
        let patch: repo::CardPatch = serde_json::from_str(r#"{"rank":2.5}"#).unwrap();
        assert_eq!(patch.rank, Some(rust_decimal::Decimal::new(25, 1)));

        let new: repo::NewCard = serde_json::from_str(r#"{"title":"t"}"#).unwrap();
        assert_eq!(new.rank, rust_decimal::Decimal::ZERO, "rank defaults to 0");
        assert_eq!(new.status, "Backlog", "status defaults to Backlog");
    }

    /// The id selector and the body are deserialized from the same object.
    #[test]
    fn id_selector_and_body_share_one_argument_object() {
        let raw = r#"{"node_id":7,"status":"Active","title":"t"}"#;
        let sel: NodeId = serde_json::from_str(raw).unwrap();
        let patch: repo::CardPatch = serde_json::from_str(raw).unwrap();
        assert_eq!(sel.node_id, 7);
        assert_eq!(patch.status.as_deref(), Some("Active"));
        assert_eq!(patch.title.as_deref(), Some("t"));
    }
}
