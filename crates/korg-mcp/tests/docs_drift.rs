//! Drift checks: the hand-written inventories in `docs/` and `README.md`
//! against the code they describe.
//!
//! The 2026-07 review found eleven documentation-drift instances (F-12), and
//! every one of them was an inventory someone maintained by hand: a tool count,
//! a REST endpoint table, an environment-variable table. Prose about *why*
//! korg does something ages gracefully. A list of *what* it has does not — it is
//! wrong the moment the code changes, and nothing complains.
//!
//! So the rule from the review's documentation map (§5) is: exactly one
//! normative home per fact, and inventories are generated or drift-tested,
//! never hand-counted. These are the drift tests. They read the markdown as
//! data and compare it against the router, the tool list, and the `env::var`
//! call sites.
//!
//! When one of these fails, the doc is the thing to fix — unless the doc was
//! right and the code grew something undocumented, which is the interesting
//! case and the reason this file exists.
//!
//! WI #862 extended the suite past `docs/` with the staleness classes the
//! agent-surface program (816 §6) caught by hand: vocabularies enumerated in
//! prose against `vocab.rs`, migration-header rollback declarations against
//! the SQL under them, and the `## Deployed` convention in sprint records.
//! Same rule throughout: a claim a test can read is a claim a test checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The lines of a `## Heading` section, up to the next heading of the same or
/// higher level.
fn section<'a>(markdown: &'a str, heading: &str) -> Vec<&'a str> {
    let mut lines = markdown.lines().skip_while(|l| l.trim() != heading);
    assert!(lines.next().is_some(), "no `{heading}` section found");
    lines
        .take_while(|l| !l.starts_with("## ") || l.trim() == heading)
        .collect()
}

/// Every `` `backticked` `` span in a string.
fn backticked(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

// ---------------------------------------------------------------------------
// MCP tools
// ---------------------------------------------------------------------------

/// The category labels used by the tool catalogue in `docs/api.md`. They are
/// also the vocabulary the MCP server instructions must speak, so an agent that
/// only ever sees `initialize` learns the same shape as one reading the docs —
/// F-12 found three copies of this list, each omitting something different.
const CATEGORIES: &[&str] = &[
    "Work items",
    "Cards",
    "Comments",
    "Reading-list links",
    "Relationships",
    "Sprint proposals",
    "Programs",
    "Awaiting Ken",
    "Board",
    "Reports",
    "Schedules",
    "Handoffs",
    "Attachments",
    "Projects and areas",
];

/// Parse the `## Tool catalogue` table into (category, tool names).
fn documented_tools() -> Vec<(String, Vec<String>)> {
    section(&read("docs/api.md"), "## Tool catalogue")
        .into_iter()
        .filter(|l| l.starts_with('|'))
        .filter(|l| !l.contains("---"))
        .filter_map(|l| {
            let mut cells = l.trim_matches('|').split('|');
            let category = cells.next()?.trim().to_string();
            let tools = backticked(cells.next()?);
            (category != "Category").then_some((category, tools))
        })
        .collect()
}

#[test]
fn the_tool_catalogue_lists_every_tool_and_no_others() {
    let documented: BTreeSet<String> = documented_tools()
        .into_iter()
        .flat_map(|(_, tools)| tools)
        .collect();
    let actual: BTreeSet<String> = korg_mcp::tools::tools()
        .iter()
        .map(|t| t.name.to_string())
        .collect();

    let undocumented: Vec<_> = actual.difference(&documented).collect();
    let phantom: Vec<_> = documented.difference(&actual).collect();

    assert!(
        undocumented.is_empty() && phantom.is_empty(),
        "docs/api.md `## Tool catalogue` is out of date.\n  \
         tools missing from the table: {undocumented:?}\n  \
         table entries with no such tool: {phantom:?}",
    );
}

#[test]
fn the_tool_catalogue_uses_the_known_categories() {
    let used: Vec<String> = documented_tools().into_iter().map(|(c, _)| c).collect();
    let known: BTreeSet<&str> = CATEGORIES.iter().copied().collect();
    for category in &used {
        assert!(
            known.contains(category.as_str()),
            "unknown catalogue category {category:?} — add it to CATEGORIES here \
             and to the MCP server instructions, or use an existing one",
        );
    }
    for category in CATEGORIES {
        assert!(
            used.iter().any(|u| u == category),
            "category {category:?} has no row in the docs/api.md catalogue",
        );
    }
}

#[test]
fn the_server_instructions_name_every_category() {
    // The instructions are the only enumeration an MCP client sees before it
    // calls anything, so an omission there is an entire feature the agent does
    // not know exists — which is exactly how proposals and reports went
    // unmentioned for two sprints.
    let instructions = korg_mcp::server_instructions().to_lowercase();
    for category in CATEGORIES {
        assert!(
            instructions.contains(&category.to_lowercase()),
            "the MCP server instructions never mention {category:?}",
        );
    }
}

/// The tool names inside the parenthesised list following `prefix` in the
/// server instructions.
fn named_in_instructions(prefix: &str) -> BTreeSet<String> {
    let text = korg_mcp::server_instructions();
    let at = text
        .find(prefix)
        .unwrap_or_else(|| panic!("server_instructions no longer says {prefix:?}"));
    let rest = &text[at + prefix.len()..];
    let open = rest.find('(').expect("a parenthesised list follows");
    let close = rest.find(')').expect("the list closes");
    rest[open + 1..close]
        .split(',')
        .map(|n| n.trim().to_string())
        .collect()
}

/// `docs/api.md`'s shape table and the MCP instructions must classify the
/// collection reads the same way (WI #579).
///
/// These are the two places korg tells an agent what a list returns, and they
/// are written for different readers — the table for someone reading the docs,
/// the instructions for a client that will never read them. Sprint 020 found
/// the instructions claiming an envelope five reads never return; it fixed both
/// texts, and this keeps them fixed *together*, so correcting one and forgetting
/// the other is a failing test rather than a discrepancy an agent trips over.
///
/// The stronger fence — claim against actual wire shape — lives in
/// `dispatch.rs::collection_reads_return_the_shape_the_instructions_promise`,
/// which needs a database. This one is pure text and runs anywhere.
#[test]
fn the_collection_read_shapes_agree_across_docs_and_instructions() {
    let api = read("docs/api.md");
    let mut documented_paginated = BTreeSet::new();
    let mut documented_bare = BTreeSet::new();
    let mut documented_filtered = BTreeSet::new();
    for line in section(&api, "## Collection reads")
        .into_iter()
        .filter(|l| l.starts_with('|'))
    {
        let mut cells = line.trim_matches('|').split('|');
        let (Some(shape), Some(reads)) = (cells.next(), cells.next()) else {
            continue;
        };
        // The remaining row (neighbors) carries its own shape and is
        // deliberately not summarised in the instructions.
        if shape.contains("items, total, limit, offset") {
            documented_paginated = backticked(reads).into_iter().collect();
        } else if shape.contains("bare array") {
            documented_bare = backticked(reads).into_iter().collect();
        } else if shape.contains("items, omitted") {
            documented_filtered = backticked(reads).into_iter().collect();
        }
    }
    assert!(
        !documented_paginated.is_empty()
            && !documented_bare.is_empty()
            && !documented_filtered.is_empty(),
        "docs/api.md's `## Collection reads` shape table no longer has all three \
         of the rows the instructions summarise: `{{items, total, limit, offset}}`, \
         `bare array`, `{{items, omitted}}`"
    );

    assert_eq!(
        documented_paginated,
        named_in_instructions("Paginated collection reads"),
        "docs/api.md and the MCP server instructions disagree about which \
         collection reads are paginated"
    );
    assert_eq!(
        documented_bare,
        named_in_instructions("the unpaginated ones"),
        "docs/api.md and the MCP server instructions disagree about which \
         collection reads return a bare array"
    );
    assert_eq!(
        documented_filtered,
        named_in_instructions("the filtered ones"),
        "docs/api.md and the MCP server instructions disagree about which \
         collection reads narrow by default and return {{items, omitted}}"
    );
}

/// The `archived` affordance is part of the same contract, and it is the one
/// that was **wrong** rather than merely undocumented (WI #851).
///
/// korg's instructions used to end "All exclude archived rows unless you ask for
/// them", which was false of `survey_work_items` — the tool those same
/// instructions recommended for cross-project sweeps, since deleted (#871) —
/// and of `list_proposals`, which had no `archived` predicate at all. An agent
/// cannot check a claim like that; it can only be burned by it.
///
/// Both are fixed, and the sentence now names the class that genuinely has no
/// archived filter (`list_reports`, `list_areas`, `list_comments`,
/// `list_daily_plan` — the bare-array reads). This asserts that split against
/// the **derived input schemas**, so a read cannot quietly join or leave the
/// class: the one-time correction is worth far less than the fence.
#[test]
fn the_archived_affordance_matches_the_instructions() {
    let without: BTreeSet<String> = named_in_instructions("the unpaginated ones");
    for tool in korg_mcp::tools::tools() {
        let name = tool.name.to_string();
        if !name.starts_with("list_") {
            continue;
        }
        let properties = tool.input_schema.get("properties");
        // `list_projects` is the one read whose subject is not a node. A project
        // has a `status` column, and `archived` is one of its values — so it
        // spells the same default and the same escape hatch through `status`
        // (#828), not through an `archived` flag. Assert it still offers both
        // rather than exempting it outright.
        if name == "list_projects" {
            let variants = properties
                .and_then(|p| p.get("status"))
                .and_then(|s| s.get("enum"))
                .and_then(|e| e.as_array())
                .map(|e| {
                    e.iter()
                        .filter_map(|v| v.as_str())
                        .map(str::to_string)
                        .collect::<BTreeSet<String>>()
                })
                .unwrap_or_default();
            assert!(
                variants.contains("archived") && variants.contains("all"),
                "`list_projects` filters archived projects through `status`, so its \
                 status enum must offer both `archived` and `all` — got {variants:?}"
            );
            continue;
        }
        let has_archived = properties.and_then(|p| p.get("archived")).is_some();
        if without.contains(&name) {
            assert!(
                !has_archived,
                "`{name}` takes an `archived` argument, but the server instructions \
                 list it among the reads that have no archived filter"
            );
        } else {
            assert!(
                has_archived,
                "`{name}` has no `archived` argument, so it cannot honour the \
                 instructions' promise that every read filtering archived rows \
                 excludes them by default — either give it the filter or name it \
                 in the no-filter list"
            );
        }
    }
}

#[test]
fn the_readme_tool_count_is_right() {
    let readme = read("README.md");
    let stated: usize = readme
        .split("exposes ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("README should say `exposes <N> tools`");
    assert_eq!(
        stated,
        korg_mcp::tools::tools().len(),
        "README.md states the wrong tool count",
    );
}

// ---------------------------------------------------------------------------
// REST routes
// ---------------------------------------------------------------------------

/// Scan `korg-api`'s router source for `.route(<path>, <methods>)`.
///
/// Reading the source rather than the built `Router` is a deliberate trade:
/// axum does not expose its route table, and the alternative — an inventory
/// constant the router and the test both consume — is one more thing to keep in
/// sync, which is the failure this test exists to catch.
fn registered_routes() -> BTreeSet<(String, String)> {
    let src = read("crates/korg-api/src/lib.rs");
    let mut routes = BTreeSet::new();

    for start in src
        .match_indices(".route(")
        .map(|(i, _)| i + ".route(".len())
    {
        // Take the balanced argument list, so multi-line calls parse the same
        // as single-line ones.
        let mut depth = 1usize;
        let mut end = start;
        for (offset, ch) in src[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let args = &src[start..end];

        let Some(path) = args.split('"').nth(1) else {
            continue;
        };
        if !path.starts_with("/api") {
            continue;
        }
        for (method, _) in [
            ("GET", "get("),
            ("POST", "post("),
            ("PATCH", "patch("),
            ("PUT", "put("),
            ("DELETE", "delete("),
        ]
        .iter()
        .filter(|(_, needle)| args.contains(*needle))
        {
            routes.insert((method.to_string(), path.to_string()));
        }
    }

    assert!(
        !routes.is_empty(),
        "route scanner found nothing — did the router move?"
    );
    routes
}

/// Parse the `### Endpoints` table in `docs/usage.md`: `| METHOD | /path | … |`,
/// where the method cell may list several (`GET`, `POST`) for one path.
fn documented_routes() -> BTreeSet<(String, String)> {
    section(&read("docs/usage.md"), "## REST API")
        .into_iter()
        .filter(|l| l.starts_with('|'))
        .filter(|l| !l.contains("---"))
        .filter_map(|l| {
            let mut cells = l.trim_matches('|').split('|');
            let methods = backticked(cells.next()?);
            let path = backticked(cells.next()?).into_iter().next()?;
            path.starts_with("/api").then_some((methods, path))
        })
        .flat_map(|(methods, path)| methods.into_iter().map(move |m| (m, path.clone())))
        .collect()
}

#[test]
fn the_rest_table_matches_the_router() {
    let registered = registered_routes();
    let documented = documented_routes();

    let undocumented: Vec<_> = registered.difference(&documented).collect();
    let phantom: Vec<_> = documented.difference(&registered).collect();

    assert!(
        undocumented.is_empty() && phantom.is_empty(),
        "the REST table in docs/usage.md is out of date.\n  \
         routes missing from the table: {undocumented:?}\n  \
         table rows with no such route: {phantom:?}\n  \
         (path parameter names must match the router exactly)",
    );
}

// ---------------------------------------------------------------------------
// Environment variables
// ---------------------------------------------------------------------------

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `KORG_*` / `DATABASE_URL` name passed to `env::var` in a crate's
/// `src/`. Test-only and generator-only reads (`TS_RS_EXPORT_DIR`,
/// `UPDATE_SCHEMA_SNAPSHOT`) live under `tests/`, so scanning `src/` alone
/// keeps this to the variables an operator actually sets.
fn env_vars_read_by(crates: &[&str]) -> BTreeSet<String> {
    let mut files = Vec::new();
    for krate in crates {
        rust_sources(
            &repo_root().join("crates").join(krate).join("src"),
            &mut files,
        );
    }
    let mut vars = BTreeSet::new();
    for file in files {
        let src = std::fs::read_to_string(&file).expect("read source");
        for (idx, _) in src.match_indices("env::var") {
            let after = &src[idx..];
            let Some(open) = after.find('"') else {
                continue;
            };
            let Some(close) = after[open + 1..].find('"') else {
                continue;
            };
            let name = &after[open + 1..open + 1 + close];
            if name.starts_with("KORG_") || name == "DATABASE_URL" {
                vars.insert(name.to_string());
            }
        }
    }
    assert!(
        !vars.is_empty(),
        "env scanner found nothing — did the crates move?"
    );
    vars
}

#[test]
fn the_setup_env_table_covers_every_runtime_variable() {
    // korg-api and korg-core are what the deployed binary is made of; the
    // variables they read are the ones docs/setup.md promises to list.
    // korg-migrate's own variables are documented in docs/migration.md.
    let read_by_code = env_vars_read_by(&["korg-api", "korg-core"]);
    let documented: BTreeSet<String> = section(&read("docs/setup.md"), "## Environment variables")
        .into_iter()
        .filter(|l| l.starts_with('|'))
        .flat_map(backticked)
        .filter(|s| s.starts_with("KORG_") || s == "DATABASE_URL")
        .collect();

    let undocumented: Vec<_> = read_by_code.difference(&documented).collect();
    assert!(
        undocumented.is_empty(),
        "docs/setup.md's environment table is missing {undocumented:?} \
         (read by korg-api/korg-core but never documented)",
    );
}

#[test]
fn the_migration_env_table_covers_every_importer_variable() {
    // migration.md documents its variables in two tables — snapshotting in step
    // 1, importing in step 3 — so this scans every table row in the file rather
    // than one section.
    let read_by_code = env_vars_read_by(&["korg-migrate"]);
    let migration = read("docs/migration.md");
    let documented: BTreeSet<String> = migration
        .lines()
        .filter(|l| l.starts_with('|'))
        .flat_map(backticked)
        .filter(|s| s.starts_with("KORG_") || s == "DATABASE_URL")
        .collect();

    let undocumented: Vec<_> = read_by_code.difference(&documented).collect();
    assert!(
        undocumented.is_empty(),
        "docs/migration.md's environment table is missing {undocumented:?}",
    );
}

// ---------------------------------------------------------------------------
// Vocabularies in prose (WI #862)
// ---------------------------------------------------------------------------
//
// 816 found docs/usage.md still describing a four-value project status after
// 0020 narrowed it to two. A vocabulary enumerated in prose is an inventory
// like any other: wrong the moment `vocab.rs` changes, and nothing complained.

/// Where docs/usage.md's vocabulary bullet list names each set, keyed by the
/// literal bullet label. The values come from `korg_core::vocab::EXPORTED`, so
/// there is one registry and this is a projection of it, not a copy.
const VOCAB_BULLETS: &[(&str, &str)] = &[
    ("node `kind`", "NODE_KINDS"),
    ("`wi_status`", "WI_STATUSES"),
    ("`wi_type`", "WI_TYPES"),
    ("`wi_tshirt`", "WI_TSHIRTS"),
    ("card `status`", "CARD_STATUSES"),
    ("link `disposition`", "LINK_DISPOSITIONS"),
    ("proposal `status`", "PROPOSAL_STATUSES"),
    ("program `status`", "PROGRAM_STATUSES"),
    ("report `status`", "REPORT_STATUSES"),
    ("project `status`", "PROJECT_STATUSES"),
    ("project `category`", "PROJECT_CATEGORIES"),
    ("schedule `cadence`", "SCHEDULE_CADENCES"),
    ("schedule `anchor_mode`", "SCHEDULE_ANCHORS"),
    ("schedule `status`", "SCHEDULE_STATUSES"),
    ("schedule substitutions", "SCHEDULE_SUBSTITUTIONS"),
    ("source `freshness`", "SOURCE_FRESHNESS"),
    ("source `asserts`", "SOURCE_ASSERTIONS"),
];

fn vocabulary(const_name: &str) -> &'static [&'static str] {
    korg_core::vocab::EXPORTED
        .iter()
        .find(|(name, _, _)| *name == const_name)
        .unwrap_or_else(|| panic!("no vocabulary named {const_name} in vocab::EXPORTED"))
        .2
}

#[test]
fn the_usage_vocabulary_list_matches_vocab_rs() {
    let usage = read("docs/usage.md");

    // Every domain vocabulary has a bullet. ERROR_CODES is the one exemption:
    // it is not a write-boundary vocabulary, and it has its own table in the
    // response-contract section.
    for (name, _, _) in korg_core::vocab::EXPORTED {
        assert!(
            name == "ERROR_CODES" || VOCAB_BULLETS.iter().any(|(_, n)| *n == name),
            "vocab::EXPORTED gained {name} but docs/usage.md's vocabulary list \
             has no bullet for it — add the bullet and its VOCAB_BULLETS entry",
        );
    }

    for (label, const_name) in VOCAB_BULLETS {
        let expected = vocabulary(const_name);
        let line = usage
            .lines()
            .find_map(|l| l.strip_prefix("- ")?.strip_prefix(label)?.strip_prefix(':'))
            .unwrap_or_else(|| {
                panic!("docs/usage.md's vocabulary list has no `- {label}: …` bullet")
            });
        let documented = backticked(line);
        assert_eq!(
            documented, expected,
            "docs/usage.md's {label} bullet does not match vocab::{const_name} \
             (order included — the docs list is the canonical spelling)",
        );
    }
}

/// A backticked `a | b | c` span is an enumeration claim: prose asserting
/// "these are the values". If two of its members belong to one vocabulary, the
/// span must BE a vocabulary — the stale four-value project status was exactly
/// a pipe-span whose extra members had been retired under it.
#[test]
fn pipe_spans_in_docs_are_complete_vocabularies() {
    let files: Vec<PathBuf> = std::fs::read_dir(repo_root().join("docs"))
        .expect("read docs/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .chain([repo_root().join("README.md")])
        .collect();

    for file in files {
        let text = std::fs::read_to_string(&file).expect("read doc");
        for span in backticked(&text) {
            if !span.contains('|') {
                continue;
            }
            let tokens: BTreeSet<String> = span.split('|').map(|t| t.trim().to_string()).collect();
            let touches_a_vocabulary = korg_core::vocab::EXPORTED.iter().any(|(_, _, values)| {
                tokens
                    .iter()
                    .filter(|t| values.contains(&t.as_str()))
                    .count()
                    >= 2
            });
            let is_a_vocabulary = korg_core::vocab::EXPORTED.iter().any(|(_, _, values)| {
                tokens
                    == values
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<BTreeSet<_>>()
            });
            assert!(
                !touches_a_vocabulary || is_a_vocabulary,
                "{}: `{span}` reads as a vocabulary enumeration but matches no \
                 vocabulary in vocab.rs — a value was probably added or retired \
                 without updating this prose",
                file.display(),
            );
        }
    }
}

/// The same rule one door over: **inside `tools()`' own descriptions** (#1387).
///
/// F-4 of the 044–059 re-review: sprint 054 added `parked`, the derived schemas
/// picked it up, and the hand-written prose around them did not — three
/// descriptions still enumerated the four-status world, and `relate` claimed
/// eight labels against a registry of nine. The 050 fence reads `docs/`; tool
/// descriptions are outside it, and they are the prose agents actually read.
///
/// The claim being checked: a run joined by `/`, `|` or `,` whose members are
/// *all* values of one vocabulary (at least two of them) is an assertion that
/// these are **the** values, so it must be a set korg actually has — a whole
/// vocabulary, or one of the named partitions `vocab::PARTITIONS` registers.
///
/// "All of them" is what keeps envelope shapes out: `{done, declined,
/// archived}` reads like two proposal statuses, but `archived` is not one, so
/// the run is a field list rather than a vocabulary claim. One member is prose
/// mentioning a value, which is why the floor is two.
#[test]
fn value_runs_in_tool_descriptions_are_complete_sets() {
    for tool in korg_mcp::tools::tools() {
        let description = tool.description.clone().unwrap_or_default();
        for run in value_runs(&description) {
            let is_exactly = |values: &[&str]| {
                run == values
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<BTreeSet<_>>()
            };
            let claims = korg_core::vocab::EXPORTED
                .iter()
                .any(|(_, _, values)| run.iter().all(|t| values.contains(&t.as_str())));
            let complete = korg_core::vocab::EXPORTED
                .iter()
                .any(|(_, _, values)| is_exactly(values))
                || korg_core::vocab::PARTITIONS
                    .iter()
                    .any(|(_, values)| is_exactly(values));
            assert!(
                !claims || complete,
                "{}'s description enumerates {run:?}, which is neither a whole \
                 vocabulary nor a named partition in vocab.rs — a value was \
                 probably added or retired without updating this prose",
                tool.name,
            );
        }
    }
}

/// How the disposal table (`docs/api.md`, WI #855) spells each node kind. The
/// table is written for a reader, so `sprint_proposal` appears as "proposals" —
/// but the mapping being *here* is what lets the coverage test below be a fence
/// rather than a guess.
const DISPOSAL_ROW_NAMES: [(&str, &str); 9] = [
    ("workitem", "work items"),
    ("card", "cards"),
    ("link", "links"),
    ("sprint_proposal", "proposals"),
    ("report", "reports"),
    ("handoff", "handoffs"),
    ("program", "programs"),
    ("schedule", "schedules"),
    ("attachment", "attachments"),
];

/// Every node kind has a disposal, so every node kind is in the table (#1388).
///
/// The table shipped in #855 and three kinds arrived after it — `program`,
/// `schedule`, `attachment` — none of which was added. That is not a cosmetic
/// gap: the doctrine's load-bearing sentence is "a hard delete refuses when the
/// row is referenced", and an agent applying it to an attachment predicts the
/// wrong outcome of `DELETE /api/img/:id`, which deliberately does not refuse.
/// A kind absent from the table is a kind whose disposal an agent will guess.
#[test]
fn the_disposal_table_covers_every_node_kind() {
    let table = section(&read("docs/api.md"), "### Disposal semantics (WI #855)").join("\n");
    for kind in korg_core::vocab::NODE_KINDS {
        let spelling = DISPOSAL_ROW_NAMES
            .iter()
            .find(|(k, _)| *k == kind)
            .unwrap_or_else(|| {
                panic!("NODE_KINDS gained {kind} — give it a DISPOSAL_ROW_NAMES spelling")
            })
            .1;
        assert!(
            table.contains(spelling),
            "docs/api.md's disposal table has no row for {kind} (\"{spelling}\") — \
             every kind has an ending, and one the table omits is one an agent \
             guesses"
        );
    }
}

/// `relate`'s description enumerates the closed label vocabulary, and until
/// #1387 it enumerated eight of the registry's nine — `materializes` (sprint
/// 051) never made it into the prose. The runtime error message is
/// registry-derived and was right the whole time; only the description lied, so
/// an agent trusting it believed it could not write a provenance edge it can.
///
/// The count word is gone from the description on purpose: a hand-maintained
/// numeral is one more copy of the same fact. This test is what keeps the list
/// whole instead.
#[test]
fn relate_names_every_registered_label() {
    let relate = korg_mcp::tools::tools()
        .into_iter()
        .find(|t| t.name == "relate")
        .expect("relate is a tool");
    let description = relate.description.clone().unwrap_or_default();
    let named: BTreeSet<String> = backticked(&description).into_iter().collect();
    for spec in korg_core::relationships::REGISTRY {
        assert!(
            named.contains(spec.label),
            "relate's description never names `{}` — the vocabulary is closed \
             and the description is where an agent reads it",
            spec.label
        );
    }
}

/// A fence that extracts nothing passes for free, so this pins the extractor
/// against the spellings the descriptions actually use — and against the stale
/// one F-4 found, which must come out as a run that no registered set matches.
#[test]
fn the_extractor_reads_the_spellings_descriptions_use() {
    let run = |text: &str| value_runs(text);
    let set =
        |values: &[&str]| -> BTreeSet<String> { values.iter().map(|v| v.to_string()).collect() };

    assert!(
        run("everything not terminal (`open`, `resolved`, `done`, `parked`) and unarchived")
            .contains(&set(&korg_core::vocab::WI_LIVE_STATUSES)),
        "a backticked comma list is an enumeration"
    );
    assert!(
        run("one count per status: open/resolved/done/closed/parked, which sum to it")
            .contains(&set(&korg_core::vocab::WI_STATUSES)),
        "a bare slash run is an enumeration"
    );
    assert!(
        run("a proposal reaching `done`/`declined`")
            .contains(&set(&korg_core::vocab::PROPOSAL_TERMINAL_STATUSES)),
        "backticks around slash-joined values do not hide the run"
    );
    assert!(
        run("`cadence` is once|weekly|monthly|quarterly|yearly")
            .contains(&set(&korg_core::vocab::SCHEDULE_CADENCES)),
        "a pipe run is an enumeration"
    );

    // The sentence F-4 caught. It must survive extraction as a run, and it must
    // match nothing — otherwise the fence above cannot bite.
    let stale = set(&["open", "resolved"]);
    assert!(run("its last materialized work item is not still open/resolved").contains(&stale));
    assert!(
        !korg_core::vocab::EXPORTED
            .iter()
            .any(|(_, _, v)| stale == set(v))
            && !korg_core::vocab::PARTITIONS
                .iter()
                .any(|(_, v)| stale == set(v)),
        "`open/resolved` must match no registered set — it is the stale \
         two-status spelling of WI_UNFINISHED_STATUSES"
    );
}

/// Runs of two or more values joined by `/`, `|` or `,` — with or without
/// backticks, since the descriptions use both spellings.
///
/// Two rules keep prose out. **One separator per run**: `a/b/c, which sums`
/// ends at `c`, rather than swallowing the next word and quietly turning a real
/// enumeration into an unrecognisable set the fence would then wave through.
/// **Comma runs must be fully backticked**: an English sentence is full of
/// commas, and the descriptions only ever comma-list values in code spans.
fn value_runs(desc: &str) -> Vec<BTreeSet<String>> {
    let chars: Vec<char> = desc.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut runs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut current: Vec<String> = Vec::new();
        let mut separator: Option<char> = None;
        let mut all_quoted = true;
        loop {
            // One token: an optionally-backticked word.
            let quoted = chars.get(i) == Some(&'`');
            let start = if quoted { i + 1 } else { i };
            let mut end = start;
            while end < chars.len() && is_word(chars[end]) {
                end += 1;
            }
            if end == start || (quoted && chars.get(end) != Some(&'`')) {
                break;
            }
            current.push(chars[start..end].iter().collect());
            all_quoted &= quoted;
            i = if quoted { end + 1 } else { end };
            // One separator — the same one throughout — optionally followed by
            // a single space.
            let next = match chars.get(i) {
                Some(&c @ ('/' | '|' | ',')) if separator.unwrap_or(c) == c => c,
                _ => break,
            };
            separator = Some(next);
            i += 1;
            if chars.get(i) == Some(&' ') {
                i += 1;
            }
        }
        if current.len() >= 2 && (separator != Some(',') || all_quoted) {
            runs.push(current.into_iter().collect());
        }
        // Skip to the next token boundary so a rejected run cannot loop.
        while i < chars.len() && is_word(chars[i]) {
            i += 1;
        }
        i += 1;
    }
    runs
}

// ---------------------------------------------------------------------------
// Migration headers (WI #862)
// ---------------------------------------------------------------------------
//
// Since 0018 every migration header declares what is under it — `DDL ONLY`,
// `DATA ONLY`, or `DDL + DATA` — because that is the fact the deploy note's
// rollback claim ("clean re-tag" vs "needs a restore") is derived from.
// Migration 0020's rollback prose is where a wrong header would have
// crash-looped an un-hand-fixed database; the declaration is the parseable
// form of that claim, so it is the part a test can hold to the SQL.

/// The first migration whose header carries the DDL/DATA declaration; the
/// convention starts there and 0001–0017 predate it.
const FIRST_DECLARED_MIGRATION: u32 = 18;

fn migrations() -> Vec<(u32, PathBuf)> {
    let dir = repo_root().join("crates/korg-core/migrations");
    let mut files: Vec<(u32, PathBuf)> = std::fs::read_dir(&dir)
        .expect("read migrations/")
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            let number = p.file_name()?.to_str()?.split('_').next()?.parse().ok()?;
            Some((number, p))
        })
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no migrations found — did the directory move?"
    );
    files
}

/// Statement-initial keywords over the non-comment lines, with the two shapes
/// that are neither schema nor data excluded: `CREATE TEMP TABLE` scaffolding
/// (and the `DROP` that cleans it up — 0018), and transient
/// `DISABLE TRIGGER`/`ENABLE TRIGGER` toggles around a guarded UPDATE.
fn classify_sql(sql: &str) -> (bool, bool) {
    let lines: Vec<&str> = sql
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("--"))
        .collect();

    let temp_tables: Vec<String> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("CREATE TEMP TABLE "))
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .expect("a table name follows CREATE TEMP TABLE")
                .trim_end_matches('(')
                .to_string()
        })
        .collect();

    let mut has_ddl = false;
    let mut has_dml = false;
    for line in &lines {
        if line.starts_with("CREATE TEMP TABLE")
            || line.contains(" DISABLE TRIGGER ")
            || line.contains(" ENABLE TRIGGER ")
        {
            continue;
        }
        if line.starts_with("DROP TABLE") && temp_tables.iter().any(|t| line.contains(t.as_str())) {
            continue;
        }
        if ["CREATE ", "ALTER ", "DROP "]
            .iter()
            .any(|k| line.starts_with(k))
        {
            has_ddl = true;
        }
        if ["UPDATE ", "INSERT ", "DELETE "]
            .iter()
            .any(|k| line.starts_with(k))
        {
            has_dml = true;
        }
    }
    (has_ddl, has_dml)
}

#[test]
fn migration_headers_declare_what_the_sql_actually_is() {
    for (number, path) in migrations() {
        if number < FIRST_DECLARED_MIGRATION {
            continue;
        }
        let sql = std::fs::read_to_string(&path).expect("read migration");
        let header: String = sql
            .lines()
            .take_while(|l| l.trim().is_empty() || l.starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();

        let lower = header.to_lowercase();
        assert!(
            lower.contains("rollback") || lower.contains("re-tag") || lower.contains("restore"),
            "{name}: the header never states its rollback semantics — say whether \
             rolling the image back across this deploy is a clean re-tag or needs \
             a restore, the way every migration since 0017 does",
        );

        let claims_ddl = header.contains("DDL ONLY")
            || header.contains("DDL + DATA")
            || header.contains("DATA + DDL");
        let claims_data = header.contains("DATA ONLY")
            || header.contains("DDL + DATA")
            || header.contains("DATA + DDL");
        assert!(
            claims_ddl || claims_data,
            "{name}: the header carries no `DDL ONLY` / `DATA ONLY` / `DDL + DATA` \
             declaration — every migration since 0018 states one, because it is \
             the fact the rollback claim rests on",
        );

        let (has_ddl, has_dml) = classify_sql(&sql);
        assert_eq!(
            claims_ddl,
            has_ddl,
            "{name}: the header {} DDL but the SQL {} — the rollback claim \
             derived from it cannot be trusted",
            if claims_ddl {
                "declares"
            } else {
                "does not declare"
            },
            if has_ddl {
                "contains some"
            } else {
                "contains none"
            },
        );
        assert_eq!(
            claims_data,
            has_dml,
            "{name}: the header {} DATA but the SQL {} — the rollback claim \
             derived from it cannot be trusted",
            if claims_data {
                "declares"
            } else {
                "does not declare"
            },
            if has_dml {
                "contains DML"
            } else {
                "contains no DML"
            },
        );
    }
}

// ---------------------------------------------------------------------------
// Sprint deploy records (WI #862)
// ---------------------------------------------------------------------------

/// Sprint 047 is where `.sprint-deploy` landed: from there on, shipping a
/// sprint deploys it (sprint-ship Phase 7), and Phase 7.2 / deploy-kubsdb
/// append a `## Deployed` section to the record. Earlier records predate the
/// declaration and are exempt.
const FIRST_DECLARED_DEPLOY_SPRINT: u32 = 47;

#[test]
fn every_shipped_sprint_since_047_records_its_deploy() {
    if !repo_root().join(".sprint-deploy").exists() {
        // The repo stopped declaring deploys; the convention this asserts
        // ended with it.
        return;
    }

    // A record is `NNN-name.md` or a `NNN-name/` directory of files.
    let mut records: Vec<(u32, String, String)> = std::fs::read_dir(repo_root().join("sprints"))
        .expect("read sprints/")
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            let number: u32 = name.split('-').next()?.parse().ok()?;
            let content = if p.is_dir() {
                std::fs::read_dir(&p)
                    .ok()?
                    .flatten()
                    .map(|f| f.path())
                    .filter(|f| f.extension().is_some_and(|e| e == "md"))
                    .filter_map(|f| std::fs::read_to_string(f).ok())
                    .collect()
            } else {
                std::fs::read_to_string(&p).ok()?
            };
            Some((number, name, content))
        })
        .collect();
    records.sort_by_key(|(n, _, _)| *n);

    // The highest-numbered record is the sprint in flight: its deploy happens
    // after merge, so its block cannot exist at PR time. Everything below it
    // has shipped, and a shipped sprint either deployed or must say why not.
    let Some(newest) = records.last().map(|(n, _, _)| *n) else {
        panic!("no sprint records found — did sprints/ move?");
    };
    for (number, name, content) in &records {
        if *number < FIRST_DECLARED_DEPLOY_SPRINT || *number == newest {
            continue;
        }
        assert!(
            content
                .lines()
                .any(|l| l.starts_with("## Deployed") || l.starts_with("## Not deployed")),
            "sprints/{name} has no `## Deployed` section, but this repo declares \
             deploys (`.sprint-deploy`) — sprint-ship Phase 7.2 appends one after \
             the deploy runs. If this sprint deliberately did not deploy, record \
             that under a `## Not deployed` heading instead",
        );
    }
}
