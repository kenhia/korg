//! The published read-shape contract (WI 2041): `contract/read-shapes.json`.
//!
//! korg's shapes are fenced *inside* this repo from three directions already —
//! `dispatch.rs` asserts the documented class against the real wire shape,
//! `docs_drift.rs` holds the docs and the MCP instructions in agreement, and
//! `schema.rs` snapshots the advertised tool surface. Every one of them fires in
//! korg's CI. None of them reaches a consumer.
//!
//! Two consumers copied the same korg sink. When the list reads grew envelopes,
//! kmon's copy crashed daily for 11 days (WI 899) and kyac's silently wrote
//! nothing for two months (kyac sprint 020). Both are loud about their own
//! drift now; neither can learn about korg's. This file is the artifact that
//! lets them: a JSON document of what each collection read actually returns,
//! generated from real dispatched responses, committed, drift-gated, and served
//! at `GET /api/contract/read-shapes` so a consumer that does not clone korg can
//! still assert against it.
//!
//! Regenerate deliberately, and read the diff — every line is a promise a
//! consumer may already be asserting on:
//!
//! ```text
//! UPDATE_READ_SHAPES=1 cargo test -p korg-mcp --test read_shapes
//! ```
//!
//! **Why it is generated and not written.** A hand-written contract is a second
//! source of truth, and this item exists because a hand-copy drifted. The
//! document is read off the wire, never off the structs and never off prose.
//!
//! **Why the seed is deliberately lumpy** (korg+ GP-14, "a sampled enum is not
//! an enum"). A published contract is a claim about korg's value domain, so it
//! must not be read off whatever rows a convenient seed happened to hold. Key
//! *presence* is the register that bites here: serde has exactly one mechanism
//! for a sometimes-absent key (`skip_serializing_if`, live today on
//! `ProjectLeanRow::status`), and one row can never reveal it. So the seed pins
//! the specimens — an archived project beside an active one, a tagged row beside
//! an untagged one — and optionality is *measured* across rows rather than
//! assumed absent. `the_conditional_project_status_key_stays_pinned` is the
//! guard on that, and it fails if a later seed loses the variance.

use korg_core::repo::{self, NewProgram, NewReport};
use korg_test_support::{fresh_korg, new};
use serde_json::{json, Map, Value};
use sqlx::PgPool;
use std::collections::BTreeSet;
use time::macros::date;

mod common;
use common::{args, body, server};

/// The collection reads whose name does not begin `list_`, with the reason each
/// is one. Kept as a named constant rather than a condition so that adding a
/// read here is a decision somebody wrote down.
///
/// - `search` (#1177) is a paginated collection read carrying two fields no
///   other read has.
/// - `neighbors` carries its own capped shape — `truncated`, not `offset` — and
///   `docs/api.md`'s table classifies it there rather than in the MCP
///   instructions, which is why `dispatch.rs` leaves it out and this does not.
///   A consumer calling it meets the shape either way.
const COLLECTION_READS_NOT_NAMED_LIST: [&str; 2] = ["search", "neighbors"];

/// Where the committed document lives. Repo root, not beside a test snapshot:
/// this one is published, so it is meant to be found by somebody browsing the
/// repo, and `korg-api` embeds it from here.
fn contract_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contract/read-shapes.json")
}

// ---------------------------------------------------------------------------
// What we ask of each read
// ---------------------------------------------------------------------------

/// One collection read, the calls that reveal its shape, and the fuller read a
/// consumer must reach for to get the rest of a row.
struct ReadSpec {
    read: &'static str,
    /// The default call first — that is what a consumer writes before it has
    /// read any documentation, and it is the shape that bit both consumers.
    /// Later calls *widen* what the read returns within the same projection, so
    /// a key that only some rows carry is observed rather than assumed absent.
    calls: Vec<Value>,
    /// The typed full read for one of these rows, and arguments naming a row the
    /// list also returns. `None` where korg publishes no fuller shape — which is
    /// itself an answer, and the document says so rather than omitting the key.
    full_read: Option<(&'static str, Value)>,
}

/// Build one database holding the variance the contract has to be honest about,
/// and return the read specs against it.
async fn seed(pool: &PgPool) -> Vec<ReadSpec> {
    // Two projects, and the second one is a pinned specimen: `list_projects`
    // omits `status` on an active row and carries it on every other, so without
    // a non-active project in the corpus the contract would publish `status` as
    // a key that does not exist (korg+ GP-14).
    repo::create_project(pool, "korg").await.expect("project");
    let retired = repo::create_project(pool, "retired-thing")
        .await
        .expect("archived project");
    repo::update_project(
        pool,
        retired,
        &repo::ProjectPatch {
            status: Some("archived".into()),
            ..Default::default()
        },
    )
    .await
    .expect("archive it");

    repo::create_area(pool, "korg", "core", None)
        .await
        .expect("area");

    // A tagged work item beside an untagged one, for the same reason: `tags` is
    // the field whose absence from a lean row cost kyac two months, and a corpus
    // where nothing is tagged cannot show what carries it.
    let tagged = repo::create_work_item(
        pool,
        repo::NewWorkItem {
            project: Some("korg".into()),
            tags: vec!["client-contract".into()],
            details: Some("a details section".into()),
            sprint: Some("083".into()),
            ..new::work_item("a tagged work item")
        },
    )
    .await
    .expect("tagged wi");
    repo::create_work_item(pool, new::work_item("an untagged work item"))
        .await
        .expect("untagged wi");
    // A closed item, so `omitted.closed` is a number a consumer can see move
    // rather than a key that is always zero.
    let closed = repo::create_work_item(pool, new::work_item("a closed work item"))
        .await
        .expect("closed wi");
    repo::update_work_item(
        pool,
        closed.wi_number,
        repo::WorkItemPatch {
            wi_status: Some("closed".into()),
            ..Default::default()
        },
    )
    .await
    .expect("close it");

    repo::create_card(pool, new::card("a card"))
        .await
        .expect("card");
    repo::create_link(pool, new::link("https://example.invalid/one"))
        .await
        .expect("link");
    repo::add_comment(pool, tagged.node_id, "a comment", Some("read_shapes"))
        .await
        .expect("comment");
    repo::relate(
        pool,
        tagged.node_id,
        closed.node_id,
        "related-to",
        None,
        None,
    )
    .await
    .expect("relationship");
    repo::set_awaiting(pool, tagged.node_id, true, Some("awaiting Ken"))
        .await
        .expect("awaiting");

    let proposal = repo::create_proposal(pool, new::proposal("a proposal"))
        .await
        .expect("proposal");
    let program = repo::create_program(
        pool,
        NewProgram {
            slices: vec![proposal.row.node_id],
            ..new::program("a program")
        },
    )
    .await
    .expect("program");
    let schedule = repo::create_schedule(pool, new::schedule("a drill", "quarterly", None))
        .await
        .expect("schedule");
    let report = repo::upsert_report(
        pool,
        NewReport {
            findings: vec![tagged.wi_number],
            ..new::report("kmon", date!(2026 - 07 - 10))
        },
    )
    .await
    .expect("report");
    let attachment = repo::create_attachment(
        pool,
        repo::NewAttachment {
            owner_node_id: Some(tagged.node_id),
            ..new::attachment("screenshot.png")
        },
    )
    .await
    .expect("attachment");
    let mut handoff_in = new::handoff("a handoff");
    handoff_in.related_node_ids = vec![tagged.node_id];
    repo::create_handoff(pool, handoff_in)
        .await
        .expect("handoff");

    vec![
        ReadSpec {
            read: "list_work_items",
            // The widening call is what makes the closed row visible, so the
            // key sets are measured over terminal and live rows alike.
            calls: vec![json!({}), json!({"wi_status": "all", "archived": null})],
            full_read: Some(("get_work_item", json!({"wi_number": tagged.wi_number}))),
        },
        ReadSpec {
            read: "list_cards",
            calls: vec![json!({})],
            full_read: None,
        },
        ReadSpec {
            read: "list_links",
            calls: vec![json!({})],
            full_read: None,
        },
        ReadSpec {
            read: "list_comments",
            calls: vec![json!({"node_id": tagged.node_id})],
            full_read: None,
        },
        ReadSpec {
            read: "list_reports",
            calls: vec![json!({})],
            full_read: Some(("get_report", json!({"node_id": report.node_id}))),
        },
        ReadSpec {
            read: "list_proposals",
            calls: vec![json!({}), json!({"status": "all"})],
            full_read: Some(("get_proposal", json!({"node_id": proposal.row.node_id}))),
        },
        ReadSpec {
            read: "list_programs",
            calls: vec![json!({}), json!({"status": "all"})],
            full_read: Some(("get_program", json!({"node_id": program.row.node_id}))),
        },
        ReadSpec {
            read: "list_projects",
            // The specimen only shows up under `status:"all"` — the default read
            // is active rows only, and an active row omits `status`.
            calls: vec![json!({}), json!({"status": "all"})],
            full_read: Some(("get_project", json!({"name": "korg"}))),
        },
        ReadSpec {
            read: "list_areas",
            calls: vec![json!({"project": "korg"})],
            full_read: None,
        },
        ReadSpec {
            read: "list_schedules",
            calls: vec![json!({}), json!({"status": "all"})],
            full_read: Some(("get_schedule", json!({"node_id": schedule.node_id}))),
        },
        ReadSpec {
            read: "list_report_sources",
            calls: vec![json!({})],
            full_read: None,
        },
        ReadSpec {
            read: "list_awaiting",
            calls: vec![json!({})],
            full_read: None,
        },
        ReadSpec {
            read: "list_attachments",
            calls: vec![json!({"node_id": tagged.node_id})],
            full_read: Some(("get_attachment", json!({"img_id": attachment.img_id}))),
        },
        ReadSpec {
            read: "search",
            calls: vec![json!({"q": "work item", "scope": "all"})],
            full_read: None,
        },
        ReadSpec {
            read: "neighbors",
            calls: vec![json!({"node_id": tagged.node_id})],
            full_read: None,
        },
    ]
}

// ---------------------------------------------------------------------------
// Reading a shape off the wire
// ---------------------------------------------------------------------------

/// Classify a response by what it *is*, never by what it was documented to be —
/// the document must be able to contradict the docs, or it is not a measurement.
fn shape_class(read: &str, value: &Value) -> &'static str {
    let has = |k: &str| value.get(k).is_some();
    match value {
        Value::Array(_) => "bare_array",
        Value::Object(_) if has("items") && has("total") && has("limit") && has("offset") => {
            "paginated"
        }
        Value::Object(_) if has("items") && has("total") && has("limit") && has("truncated") => {
            "capped"
        }
        Value::Object(_) if has("items") && has("omitted") => "filtered",
        _ => panic!(
            "`{read}` returned a shape this contract has no class for, so publishing it would \
             publish a guess: {value}\nAdd the class deliberately — a consumer is going to \
             switch on it."
        ),
    }
}

/// The rows of a collection response, whichever shape carries them.
fn rows(value: &Value) -> Vec<&Map<String, Value>> {
    let items = match value {
        Value::Array(a) => a,
        Value::Object(o) => o
            .get("items")
            .and_then(Value::as_array)
            .expect("an enveloped collection read has `items`"),
        _ => panic!("not a collection response: {value}"),
    };
    items.iter().filter_map(Value::as_object).collect()
}

/// Keys every row carries, and keys only some rows carry.
///
/// The second set is the one worth having: a consumer reading a key that is
/// sometimes absent gets `None` rather than an error, which is the silent half
/// of this whole failure class.
fn key_sets(rows: &[&Map<String, Value>]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut required: Option<BTreeSet<String>> = None;
    let mut union: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        let keys: BTreeSet<String> = row.keys().cloned().collect();
        union.extend(keys.iter().cloned());
        required = Some(match required {
            None => keys,
            Some(acc) => acc.intersection(&keys).cloned().collect(),
        });
    }
    let required = required.unwrap_or_default();
    let optional = union.difference(&required).cloned().collect();
    (required, optional)
}

fn sorted(keys: &Map<String, Value>) -> Vec<String> {
    keys.keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The reads korg advertises that return a collection.
fn advertised_collection_reads() -> BTreeSet<String> {
    korg_mcp::tools::tools()
        .iter()
        .map(|t| t.name.to_string())
        .filter(|n| n.starts_with("list_") || COLLECTION_READS_NOT_NAMED_LIST.contains(&n.as_str()))
        .collect()
}

/// Whether a read takes a `detail` argument — a second projection of the same
/// rows. Derived from the advertised input schema rather than declared here, so
/// it cannot drift from what the tool accepts.
fn projection_switch(read: &str) -> Option<String> {
    let tools = korg_mcp::tools::tools();
    let tool = tools.iter().find(|t| t.name == read)?;
    let props = tool.input_schema.get("properties")?.as_object()?;
    props.contains_key("detail").then(|| "detail".to_string())
}

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

async fn document(pool: &PgPool) -> Value {
    let specs = seed(pool).await;
    let server = server(pool.clone());

    let mut reads = Vec::new();
    for spec in &specs {
        let mut envelope: Option<Vec<String>> = None;
        let mut omitted: Option<Vec<String>> = None;
        let mut class: Option<&'static str> = None;
        let mut all_rows: Vec<Map<String, Value>> = Vec::new();

        for call in &spec.calls {
            let value = body(
                &server
                    .call(spec.read, args(call.clone()))
                    .await
                    .unwrap_or_else(|e| panic!("`{}` with {call}: {e}", spec.read)),
            );
            let observed = shape_class(spec.read, &value);
            if let Some(first) = class {
                assert_eq!(
                    first, observed,
                    "`{}` changed shape class between calls ({first} then {observed}); the \
                     contract publishes one class per read, so this needs a decision rather \
                     than a regenerate",
                    spec.read
                );
            }
            class = Some(observed);

            if let Value::Object(o) = &value {
                let keys: Vec<String> = sorted(o);
                if let Some(first) = &envelope {
                    assert_eq!(
                        first, &keys,
                        "`{}` returned different envelope keys for different arguments",
                        spec.read
                    );
                }
                envelope = Some(keys);
                if let Some(Value::Object(om)) = o.get("omitted") {
                    omitted = Some(sorted(om));
                }
            }
            all_rows.extend(rows(&value).into_iter().cloned());
        }

        assert!(
            !all_rows.is_empty(),
            "`{}` returned no rows against the seed, so the contract would publish no row \
             shape for it at all — a hole that reads exactly like a read with no fields. \
             Seed a row for it.",
            spec.read
        );

        let row_refs: Vec<&Map<String, Value>> = all_rows.iter().collect();
        let (required, optional) = key_sets(&row_refs);

        // The projection difference, measured both ways. `only_on_full_read` is
        // the register that cost kyac two months — a lean row carries no `tags`,
        // and reaching for them yields `None` rather than an error. The reverse
        // direction is just as real: a consumer that "upgrades" to the full read
        // can silently lose a signal only the list computes.
        let (full_read, only_on_full, only_on_list) = match &spec.full_read {
            None => (Value::Null, Vec::new(), Vec::new()),
            Some((name, call)) => {
                let full = body(
                    &server
                        .call(name, args(call.clone()))
                        .await
                        .unwrap_or_else(|e| panic!("`{name}` with {call}: {e}")),
                );
                let full_keys: BTreeSet<String> = full
                    .as_object()
                    .expect("a typed full read returns an object")
                    .keys()
                    .cloned()
                    .collect();
                let list_keys: BTreeSet<String> = required.union(&optional).cloned().collect();
                (
                    json!(name),
                    full_keys.difference(&list_keys).cloned().collect(),
                    list_keys.difference(&full_keys).cloned().collect(),
                )
            }
        };

        let projection = match (&spec.full_read, only_on_full.is_empty()) {
            (None, _) => "unpaired",
            (Some(_), true) => "full",
            (Some(_), false) => "lean",
        };

        reads.push(json!({
            "read": spec.read,
            "shape": class.expect("classified"),
            "envelope_keys": envelope.unwrap_or_default(),
            "omitted_keys": omitted.unwrap_or_default(),
            "row_keys": required.iter().cloned().collect::<Vec<_>>(),
            "row_optional_keys": optional.iter().cloned().collect::<Vec<_>>(),
            "projection": projection,
            "full_read": full_read,
            "only_on_full_read": only_on_full,
            "only_on_list_read": only_on_list,
            "projection_switch": projection_switch(spec.read),
        }));
    }
    reads.sort_by(|a, b| a["read"].as_str().cmp(&b["read"].as_str()));

    json!({
        "about": "korg's read shapes, as its own test suite measures them off the wire. \
                  Assert against this in your own CI and a korg shape change fails in your \
                  build instead of in production. Generated by korg's `read_shapes` suite \
                  (WI 2041) — never hand-edited. Served live at GET /api/contract/read-shapes.",
        "shape_classes": {
            "paginated": "{items, total, limit, offset} — `total` is the whole filtered corpus on EVERY page, including one whose offset overshot the last row.",
            "filtered": "{items, omitted} — no paging; these narrow rows by default, and `omitted` is what that hid, so a narrowed view can never be mistaken for the whole corpus.",
            "bare_array": "a bare JSON array, no envelope. Iterating one of the enveloped reads the same way yields its KEYS — the failure that crashed kmon for 11 days.",
            "capped": "{items, total, limit, truncated} — caps rather than pages, so there is no `offset` to advance."
        },
        "projections": {
            "lean": "the list row carries less than the full read; `only_on_full_read` names what you must fetch the item for.",
            "full": "the list row carries everything the full read does.",
            "unpaired": "korg publishes no fuller read for these rows — the list row is the whole shape."
        },
        "field_notes": {
            "row_keys": "present on every row.",
            "row_optional_keys": "present on SOME rows only. Reading one of these gets null rather than an error, which is the silent half of this failure class — korg omits a key whose value would be meaningless, it does not send null.",
            "only_on_full_read": "keys the full read adds. Reaching for one of these on a list row yields nothing and raises nothing.",
            "only_on_list_read": "keys only the list computes — a consumer that switches to the full read loses these.",
            "projection_switch": "an argument that returns a different row shape from the same read; the keys here describe the default projection."
        },
        "reads": reads
    })
}

// ---------------------------------------------------------------------------
// The gates
// ---------------------------------------------------------------------------

/// The one test that needs a database: regenerate from the wire, compare with
/// what is committed.
#[tokio::test]
async fn the_published_contract_matches_the_wire() {
    let (_pg, pool) = fresh_korg().await;
    let current = serde_json::to_string_pretty(&document(&pool).await).expect("serialize") + "\n";
    let path = contract_path();

    if std::env::var_os("UPDATE_READ_SHAPES").is_some() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir contract/");
        std::fs::write(&path, &current).expect("write contract");
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun UPDATE_READ_SHAPES=1 cargo test -p korg-mcp --test read_shapes",
            path.display()
        )
    });

    assert_eq!(
        committed, current,
        "contract/read-shapes.json no longer describes what the reads return.\n\
         This file is PUBLISHED — consumers assert against it, and korg-api serves it at \
         /api/contract/read-shapes — so regenerating is announcing a contract change:\n  \
         UPDATE_READ_SHAPES=1 cargo test -p korg-mcp --test read_shapes\n\
         Read the diff before committing it."
    );
}

// --- assertions about the PUBLISHED document -------------------------------
//
// Deliberately read from disk rather than rebuilt from a fresh database. The
// subject of these four is the artifact a consumer receives, which is the file,
// not a document this process could build. They also need no Postgres, so the
// facts below stay checkable in a context where nothing else here is.

/// The committed document, parsed.
fn published() -> Value {
    let path = contract_path();
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&text).expect("the committed contract is json")
}

fn published_reads() -> Vec<Value> {
    published()["reads"]
        .as_array()
        .expect("a `reads` array")
        .clone()
}

fn published_read(name: &str) -> Value {
    published_reads()
        .into_iter()
        .find(|r| r["read"] == name)
        .unwrap_or_else(|| panic!("`{name}` is not in the published contract"))
}

fn string_list(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap_or_else(|| panic!("`{key}` is a list"))
        .iter()
        .map(|k| k.as_str().expect("a string").to_string())
        .collect()
}

/// Every collection read korg advertises is in the published document, and
/// nothing else is. Asserted against the live tool list rather than a list
/// written here: a hand-copy is the thing this whole item is about.
#[test]
fn every_advertised_collection_read_is_published() {
    let published: BTreeSet<String> = published_reads()
        .iter()
        .map(|r| r["read"].as_str().expect("read name").to_string())
        .collect();
    let advertised = advertised_collection_reads();

    let missing: Vec<&String> = advertised.difference(&published).collect();
    assert!(
        missing.is_empty(),
        "korg advertises collection reads the published contract says nothing about: \
         {missing:?} — a consumer calling one of these is back to guessing, which is the \
         failure WI 2041 exists to close. Regenerate: UPDATE_READ_SHAPES=1 cargo test \
         -p korg-mcp --test read_shapes (add a ReadSpec for it first)"
    );
    let phantom: Vec<&String> = published.difference(&advertised).collect();
    assert!(
        phantom.is_empty(),
        "the published contract describes reads korg does not advertise: {phantom:?}"
    );
}

/// Every class the document explains is one some read actually returns, and
/// every class a read returns is explained.
///
/// This is GP-14's mirror image in the small: a legend entry for a shape korg
/// does not emit is a hypothetical with an expiry date, and nothing fails when it
/// quietly stops being true.
#[test]
fn every_documented_shape_class_is_one_a_read_returns() {
    let doc = published();
    let explained: BTreeSet<String> = doc["shape_classes"]
        .as_object()
        .expect("shape_classes")
        .keys()
        .cloned()
        .collect();
    let observed: BTreeSet<String> = published_reads()
        .iter()
        .map(|r| r["shape"].as_str().expect("shape").to_string())
        .collect();
    assert_eq!(
        explained, observed,
        "the shape-class legend and the shapes the reads actually return disagree"
    );
    let projections_explained: BTreeSet<String> = doc["projections"]
        .as_object()
        .expect("projections")
        .keys()
        .cloned()
        .collect();
    let projections_used: BTreeSet<String> = published_reads()
        .iter()
        .map(|r| r["projection"].as_str().expect("projection").to_string())
        .collect();
    assert_eq!(
        projections_explained, projections_used,
        "the projection legend and the projections the reads report disagree"
    );
}

/// The pinned specimen (korg+ GP-14).
///
/// `ProjectLeanRow::status` is korg's only `skip_serializing_if` field: it is
/// omitted on an active row and present on every other, and the default read
/// returns active rows only. So the corpus needs a non-active project or the
/// contract publishes `status` as a key that does not exist — and the seed
/// losing that project is a silent regression in the *published* document, not
/// in a test.
#[test]
fn the_conditional_project_status_key_stays_pinned() {
    let projects = published_read("list_projects");
    let optional = string_list(&projects, "row_optional_keys");
    assert!(
        optional.contains(&"status".to_string()),
        "`list_projects` should report `status` as a SOMETIMES-present key, and reports \
         {optional:?}. Either the seed lost its non-active project — in which case the \
         published contract is now claiming `status` never appears — or the field stopped \
         being conditional, which is a contract change to make deliberately."
    );
}

/// The lean-vs-full projection difference, named rather than merely generated.
///
/// This is the half of WI 2041 that failed *silently*: dedup built on a lean
/// row's `tags` reads `None` every run, forever, and nothing errors. If korg ever
/// puts `tags` on the lean row this fails, and the sentence above stops being
/// true — which is the right moment to notice.
#[test]
fn the_lean_work_item_row_is_published_as_missing_tags() {
    let wi = published_read("list_work_items");
    assert_eq!(wi["shape"], "paginated");
    assert_eq!(wi["projection"], "lean");

    let only_on_full = string_list(&wi, "only_on_full_read");
    for field in ["tags", "content", "details", "sprint"] {
        assert!(
            only_on_full.contains(&field.to_string()),
            "the lean work-item row carries no `{field}`, so the contract must say so; \
             it publishes {only_on_full:?}"
        );
    }
    assert_eq!(
        wi["full_read"], "get_work_item",
        "a consumer told a row is lean must also be told where the rest is"
    );
}

/// Every read the document describes says all of it.
///
/// A published entry missing a key is worse than a missing entry: a consumer
/// asserting on `only_on_full_read` gets an empty list from `null` and concludes
/// the row is complete.
#[test]
fn every_published_entry_is_complete() {
    for read in published_reads() {
        let name = read["read"].as_str().expect("read name");
        for key in [
            "shape",
            "envelope_keys",
            "omitted_keys",
            "row_keys",
            "row_optional_keys",
            "projection",
            "only_on_full_read",
            "only_on_list_read",
        ] {
            assert!(
                !read[key].is_null() && read.get(key).is_some(),
                "`{name}` publishes no `{key}`"
            );
        }
        assert!(
            !string_list(&read, "row_keys").is_empty(),
            "`{name}` publishes no row keys at all, which reads exactly like a read whose \
             rows have no fields"
        );
        // `full_read` and `projection_switch` are nullable *by design* — null is
        // the answer "korg publishes no fuller read for this" — so they are
        // checked for presence, not for content.
        assert!(
            read.as_object()
                .expect("an object")
                .contains_key("full_read"),
            "`{name}` omits `full_read`; null is the answer, absence is not"
        );
        assert!(
            read.as_object()
                .expect("an object")
                .contains_key("projection_switch"),
            "`{name}` omits `projection_switch`; null is the answer, absence is not"
        );
    }
}
