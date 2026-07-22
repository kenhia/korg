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
//! [`crate::repo`] and [`crate::topics`]). Both transports deserialize the
//! *same* types, and the MCP input schemas are derived from them via
//! [`schemars`] — including the enum lists, which come from [`crate::vocab`],
//! so a vocabulary change reaches the tool schema with no hand-edit at all.
//!
//! What deliberately does NOT live here: the REST query-string filter structs.
//! A query string cannot carry a JSON `null`, so REST spells the tri-state
//! `archived` filter as `"true" | "false" | "all"` while MCP spells it as a
//! nullable boolean. Those are genuinely different wire shapes; both funnel
//! into the same `repo::*Query` type, which is where the sharing belongs.

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer};

use crate::repo::{self, archived_default, ArchivedFilter, PageQuery};
use crate::topics;

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
pub(crate) fn default_true() -> bool {
    true
}
fn default_survey_limit() -> i64 {
    50
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

    pub fn non_empty(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "minLength": 1 })
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
    pub fn proposal_status_filter(_: &mut SchemaGenerator) -> Schema {
        nullable_enumerated(&vocab::PROPOSAL_STATUSES)
    }
    pub fn report_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::REPORT_STATUSES)
    }
    pub fn project_status(_: &mut SchemaGenerator) -> Schema {
        enumerated(&vocab::PROJECT_STATUSES)
    }

    /// Fractional rank. Arrives as a JSON number and is stored as a `Decimal`.
    pub fn rank(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "number" })
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

    /// A 0-based position within an ordered day.
    pub fn position(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "minimum": 0 })
    }

    pub fn survey_limit(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer", "minimum": 1, "maximum": repo::LIST_LIMIT_MAX,
            "default": super::default_survey_limit()
        })
    }

    pub fn survey_offset(_: &mut SchemaGenerator) -> Schema {
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
}

// --- operations with no core `New*`/`*Patch` counterpart --------------------
//
// Everything below is an operation whose payload never had a core struct to
// live on. The create/update payloads that DO have one (work items, cards,
// links, topics, proposals, projects, reports) are the core types themselves —
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

/// `list_areas` — and the `project` query parameter it shares with REST.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProjectRef {
    #[schemars(schema_with = "schema::non_empty")]
    pub project: String,
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
}

/// `archive_topic` / `POST /api/topics/:node_id/archive`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ArchiveTopic {
    #[serde(default = "default_true")]
    pub archived: bool,
}

/// `create_daily_plan_item` / `POST /api/daily-plan`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateDailyPlanItem {
    pub source_node_id: i64,
    #[schemars(schema_with = "schema::date")]
    pub plan_date: String,
}

/// `set_daily_plan_completion`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SetCompletion {
    pub completed: bool,
}

/// `move_daily_plan_item`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MoveDailyPlanItem {
    #[schemars(schema_with = "schema::date")]
    pub target_date: String,
    #[serde(default)]
    #[schemars(schema_with = "schema::position")]
    pub target_position: i32,
}

/// `reorder_daily_plan` — the complete new order for one open day.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReorderDailyPlan {
    #[schemars(schema_with = "schema::node_ids")]
    pub node_ids: Vec<i64>,
}

/// `list_daily_plan` — an inclusive date range.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DateRange {
    #[schemars(schema_with = "schema::date")]
    pub from: String,
    #[schemars(schema_with = "schema::date")]
    pub to: String,
}

/// `daily_plan_history` — the same range, optionally narrowed to one source.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct HistoryRange {
    #[schemars(schema_with = "schema::date")]
    pub from: String,
    #[schemars(schema_with = "schema::date")]
    pub to: String,
    #[serde(default)]
    pub source_node_id: Option<i64>,
}

// --- collection-read filters (MCP spelling) ---------------------------------
//
// REST spells these as query strings (see the module docs); both spellings
// resolve to the same `repo::*Query`.

/// `list_work_items`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListWorkItems {
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

impl From<ListWorkItems> for repo::WorkItemQuery {
    fn from(a: ListWorkItems) -> Self {
        Self {
            project: a.project,
            archived: a.archived,
            page: PageQuery {
                limit: a.limit,
                offset: a.offset,
            },
        }
    }
}

/// `survey_work_items` — the slim cross-project projection. Note that its
/// `archived` default differs from every other list read on purpose: the survey
/// is a sweep, so omitting the filter means "both".
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SurveyWorkItems {
    /// Project name to filter by
    #[serde(default)]
    pub project: Option<String>,
    /// Status to filter by, e.g. "open"
    #[serde(default)]
    pub wi_status: Option<String>,
    /// Filter by archived flag; OMIT for both archived and unarchived (there is no default).
    #[serde(default)]
    pub archived: Option<bool>,
    #[serde(default = "default_survey_limit")]
    #[schemars(schema_with = "schema::survey_limit")]
    pub limit: i64,
    #[serde(default)]
    #[schemars(schema_with = "schema::survey_offset")]
    pub offset: i64,
}

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

/// `list_topics` / `search_topics`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListTopics {
    /// Match against name and description
    #[serde(default)]
    pub q: Option<String>,
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

impl From<ListTopics> for topics::TopicQuery {
    fn from(a: ListTopics) -> Self {
        Self {
            q: a.q,
            archived: a.archived,
            page: PageQuery {
                limit: a.limit,
                offset: a.offset,
            },
        }
    }
}

/// `list_proposals`.
#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct ListProposals {
    #[serde(default)]
    #[schemars(schema_with = "schema::proposal_status_filter")]
    pub status: Option<String>,
    /// Project name to filter by
    #[serde(default)]
    pub project: Option<String>,
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
