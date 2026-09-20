//! The project-listing connector (WI 2874, slice 1 of program korg:2916).
//!
//! **A connector is a URL, not a plugin.** kctrldeck's Code tab (WI 2853)
//! wants every live korg project as a closed, launchable row, so that adding a
//! project to korg puts it on the panel and archiving one takes it off. The
//! protocol it consumes is a JSON shape any source can emit; korg is simply
//! the first source, and the deck holds no korg-specific code. That is why the
//! shape below says `connector` and `version` rather than assuming korg.
//!
//! **No new data.** Every field is a projection of what `GET /api/projects`
//! already returns. The endpoint exists because the *consumer* should not have
//! to learn korg's storage conventions to open a folder — which is the whole
//! of what it adds:
//!
//! - `src_path` is `~/`-relative, or a drive root for a Windows clone (0036).
//!   `path` here is ABSOLUTE and native to its host, so a consumer never needs
//!   korg's `~/` convention, a home directory, or the drive-root mapping.
//! - `machines` is a list and `src_path` is singular — documented since #675
//!   as the working copy on the *development* machine. So a location's host is
//!   `machines[0]`, the one host `src_path` is defined against.
//!
//! `locations` is nonetheless a list, because the protocol allows several
//! clones and a consumer written against v1 must not need a shape change when
//! korg learns to describe more than one. korg v1 emits exactly one.
//!
//! **Versioning is the contract mechanism here, deliberately not
//! `contract/read-shapes.json`.** That document is generated from korg's
//! advertised *MCP collection reads* and asserts it describes those and
//! nothing else (`read_shapes::every_advertised_collection_read_is_published`),
//! so a REST-only projection does not belong in it and adding one would fail
//! that suite's `phantom` check. `version` is what a consumer branches on;
//! within a version, changes are additive only.

use axum::extract::State;
use serde::Serialize;

use korg_core::repo::{self, ProjectRow};

use crate::{ApiResult, AppState};

use axum::Json;
use serde_json::{json, Value};

/// Bumped only for a change a v1 consumer could not survive. Additive fields
/// do not bump it — that is the promise the number is making.
pub const VERSION: u32 = 1;

/// Every POSIX host in the fleet runs as `ken`, so `~/` expands the same way
/// on all of them.
///
/// Hardcoded, and worth saying why rather than leaving it to read as an
/// oversight: korg models machines as a `TEXT[]` of names (migration 0011) and
/// has no machine *rows*, so there is nowhere to record a per-host user. A
/// registry is a schema addition and a question about whether korg should
/// model hosts at all — filed rather than guessed at here (korg WI 2919).
/// Until then this constant is the fleet convention it already encodes:
/// `src_path` has been `~/`-relative since 0019 precisely because every
/// development host shares one.
///
/// Verified 2026-09-20 on both hosts this can emit: `$HOME` is `/home/ken` on
/// kai and on kubs0. A wrong value here fails loudly at the consumer — VS Code
/// cannot open the folder — rather than silently.
const POSIX_HOME: &str = "/home/ken";

/// One clone, addressed the way its own host spells it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Location {
    /// A korg `machines` entry — `kai`, `kubs0`, `cleo`.
    pub host: String,
    /// Absolute, and native to `host`: `/home/ken/src/tools/korg`, or
    /// `D:\ClaudeWorks\kctrldeck`.
    pub path: String,
    /// `posix` or `windows` — which spelling `path` is in. A consumer needs it
    /// to build a launch target and cannot reliably infer it from the string.
    pub kind: &'static str,
}

/// How a stored `src_path` is spelled on its host, and the expansion that goes
/// with it.
///
/// Returns `None` for a value in neither form. Unreachable through korg's own
/// writes — `project_src_path_canonical` admits exactly these two — but a
/// direct SQL write is not impossible, and the alternative to skipping is
/// emitting a path that is not absolute, which is the one thing the consumer
/// is promised.
fn locate(src_path: &str) -> Option<(String, &'static str)> {
    if let Some(rest) = src_path.strip_prefix("~/") {
        return Some((format!("{POSIX_HOME}/{rest}"), "posix"));
    }
    // `/d/ClaudeWorks/kctrldeck` -> `D:\ClaudeWorks\kctrldeck`. The inverse of
    // what 0036 accepts, and mechanical in both directions by design — that
    // reversibility is why the drive-root form was chosen over storing the
    // native spelling.
    let bytes = src_path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_lowercase() && bytes[2] == b'/' {
        let drive = (bytes[1] as char).to_ascii_uppercase();
        let rest = src_path[2..].replace('/', "\\");
        return Some((format!("{drive}:{rest}"), "windows"));
    }
    None
}

/// The v1 row for one project, or `None` if korg cannot say where it lives.
///
/// The filter is the shape's own precondition rather than a policy: a project
/// with no `src_path` has no path to emit, and one with no `machines` has no
/// host to attach a path to. Both are silent omissions on purpose — a project
/// korg cannot locate is not an error, it is a project whose metadata nobody
/// has filled in, and failing the whole listing over one would take the panel
/// down for a missing field.
fn project_row(p: &ProjectRow) -> Option<Value> {
    let src_path = p.src_path.as_deref()?;
    let host = p.machines.first()?;
    let (path, kind) = locate(src_path)?;

    let mut row = json!({
        "name": p.name,
        "starred": p.starred,
        "locations": [Location { host: host.clone(), path, kind }],
    });
    // Omitted rather than null when korg does not know it: the protocol marks
    // `category` optional, and a consumer grouping by it wants "no category",
    // not a group called `null`.
    if let Some(category) = &p.category {
        row["category"] = json!(category);
    }
    Some(row)
}

/// `GET /api/connectors/projects` — the v1 listing.
///
/// Read-only and unauthenticated, like every other korg read: korg is no-auth
/// HTTP on the tailnet, and this endpoint exposes nothing `GET /api/projects`
/// does not already.
pub(crate) async fn projects(State(s): State<AppState>) -> ApiResult {
    let rows = repo::list_projects(&s.pool).await?;
    let projects: Vec<Value> = rows
        .iter()
        // Archived projects are the point of the automation: one archived in
        // korg leaves the panel on the next refresh, with nothing hand-kept.
        .filter(|p| p.status == "active")
        .filter_map(project_row)
        .collect();

    Ok(Json(json!({
        "connector": "korg",
        "version": VERSION,
        // Whose staleness the consumer displays when a fetch fails and it
        // keeps its last-good copy.
        "generated": s
            .config
            .now()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        "projects": projects,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tilde_path_expands_to_the_fleet_home() {
        assert_eq!(
            locate("~/src/tools/korg"),
            Some(("/home/ken/src/tools/korg".to_string(), "posix"))
        );
    }

    #[test]
    fn a_drive_root_becomes_the_native_windows_spelling() {
        assert_eq!(
            locate("/d/ClaudeWorks/kctrldeck"),
            Some((r"D:\ClaudeWorks\kctrldeck".to_string(), "windows"))
        );
        assert_eq!(
            locate("/c/Users/kenhi/thing"),
            Some((r"C:\Users\kenhi\thing".to_string(), "windows"))
        );
    }

    /// The near-miss that matters: a bare POSIX absolute path must not be read
    /// as drive `h`. 0036's CHECK refuses to store one, and this is the second
    /// half of that guarantee — if one ever arrives by another route it is
    /// skipped, not mangled into `H:\ome\ken\…`.
    #[test]
    fn an_unrecognised_form_is_skipped_rather_than_guessed() {
        assert_eq!(locate("/home/ken/src/x"), None);
        assert_eq!(locate("src/tools/korg"), None);
        assert_eq!(locate("~"), None);
        assert_eq!(locate("/d"), None);
    }
}
