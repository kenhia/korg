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
    "Topics",
    "Daily planning",
    "Sprint proposals",
    "Reports",
    "Handoffs",
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
