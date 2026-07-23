//! The MCP tool surface, fenced (WI #540).
//!
//! Tool schemas are derived from the shared `korg-core` structs, so drift
//! between "what the handler accepts" and "what the schema advertises" is
//! structurally impossible. What is still possible is drifting *away from what
//! agents have been told* without noticing — a renamed field, a vocabulary
//! entry that quietly disappears, a tool that stops being registered. That is
//! what these tests catch.
//!
//! `tools_schema.json` is the committed snapshot. Regenerate it deliberately:
//!
//! ```text
//! UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p korg-mcp --test schema
//! ```
//!
//! and read the diff before committing it — every line is a change agents see.

use korg_core::vocab;
use serde_json::{json, Value};

fn snapshot_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tools_schema.json")
}

/// The full advertised surface: name, description, and input schema per tool.
fn surface() -> Value {
    let tools: Vec<Value> = korg_mcp::tools::tools()
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        })
        .collect();
    Value::Array(tools)
}

#[test]
fn tool_surface_matches_the_committed_snapshot() {
    let current = serde_json::to_string_pretty(&surface()).expect("serialize surface") + "\n";
    let path = snapshot_path();

    if std::env::var_os("UPDATE_SCHEMA_SNAPSHOT").is_some() {
        std::fs::write(&path, &current).expect("write snapshot");
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nrun UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p korg-mcp --test schema",
            path.display()
        )
    });

    if committed == current {
        return;
    }

    // Report the first differing tool by name rather than dumping 1500 lines.
    let old: Vec<Value> = serde_json::from_str(&committed).expect("snapshot is valid JSON");
    let new: Vec<Value> = serde_json::from_value(surface()).unwrap();
    for (a, b) in old.iter().zip(new.iter()) {
        assert_eq!(
            a,
            b,
            "tool `{}` changed — review the diff, then \
             UPDATE_SCHEMA_SNAPSHOT=1 cargo test -p korg-mcp --test schema",
            b["name"].as_str().unwrap_or("?")
        );
    }
    panic!(
        "the tool list changed length: {} committed, {} now",
        old.len(),
        new.len()
    );
}

/// The parity check F-22 asked for, stated directly: every vocabulary field's
/// advertised `enum` **is** the vocabulary. Derivation already guarantees this;
/// the test says so out loud, and would fail loudly if the derivation were ever
/// swapped back for a literal.
#[test]
fn advertised_enums_are_the_vocabulary() {
    let tools = korg_mcp::tools::tools();
    let field = |tool: &str, field: &str| -> Vec<String> {
        let t = tools
            .iter()
            .find(|t| t.name == tool)
            .unwrap_or_else(|| panic!("no tool named {tool}"));
        t.input_schema["properties"][field]["enum"]
            .as_array()
            .unwrap_or_else(|| panic!("{tool}.{field} has no enum: {:?}", t.input_schema[field]))
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    };
    let expect =
        |values: &[&str]| -> Vec<String> { values.iter().map(|s| (*s).to_string()).collect() };

    assert_eq!(
        field("create_work_item", "wi_status"),
        expect(&vocab::WI_STATUSES)
    );
    assert_eq!(
        field("create_work_item", "wi_type"),
        expect(&vocab::WI_TYPES)
    );
    assert_eq!(
        field("create_work_item", "wi_tshirt"),
        expect(&vocab::WI_TSHIRTS)
    );
    assert_eq!(
        field("update_work_item", "wi_status"),
        expect(&vocab::WI_STATUSES)
    );
    assert_eq!(
        field("create_card", "status"),
        expect(&vocab::CARD_STATUSES)
    );
    assert_eq!(
        field("update_card", "status"),
        expect(&vocab::CARD_STATUSES)
    );
    // Filters carry the same values plus `null`, which means "no filter".
    assert_eq!(field("list_cards", "status"), expect(&vocab::CARD_STATUSES));
    assert_eq!(
        field("update_link", "disposition"),
        expect(&vocab::LINK_DISPOSITIONS)
    );
    assert_eq!(
        field("list_links", "disposition"),
        expect(&vocab::LINK_DISPOSITIONS)
    );
    assert_eq!(
        field("update_proposal", "status"),
        expect(&vocab::PROPOSAL_STATUSES)
    );
    assert_eq!(
        field("list_proposals", "status"),
        expect(&vocab::PROPOSAL_STATUSES)
    );
    assert_eq!(
        field("create_report", "status"),
        expect(&vocab::REPORT_STATUSES)
    );
    assert_eq!(
        field("update_project", "status"),
        expect(&vocab::PROJECT_STATUSES)
    );
}

// `every_advertised_tool_has_a_handler` lived here until sprint 020. It grepped
// `tools.rs` for each tool's string literal, which proved an arm *existed* —
// it would have passed against `todo!()`, and it did pass throughout the period
// when ten tools had never been called by anything. `tests/dispatch.rs` now
// runs every arm instead, so the grep is deleted rather than kept alongside:
// two tests for one property, one of which cannot fail when the other passes,
// is just a slower way to learn the same thing.
