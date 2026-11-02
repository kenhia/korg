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
//!   korg's `~/` convention, a home directory, or the drive-root mapping. The
//!   home comes from the host's `machine` row (0037, WI 2919); sprint 086 used
//!   a `/home/ken` constant, which was wrong for kimac (`/Users/ken`).
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

use std::collections::HashMap;

use axum::extract::State;
use serde::Serialize;

use korg_core::repo::{self, ProjectRow};

use crate::{ApiResult, AppState};

use axum::Json;
use serde_json::{json, Value};

/// Bumped only for a change a v1 consumer could not survive. Additive fields
/// do not bump it — that is the promise the number is making.
pub const VERSION: u32 = 1;

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

/// Why a project with a `src_path` and a host still has no `locations` row.
/// Reported in the listing's `unlocated` array rather than guessed at: the
/// consumer is promised an absolute path, and a guessed one is worse than none.
#[derive(Debug, PartialEq, Eq)]
pub enum Unlocated {
    /// `~/`-relative, and the host has no `machine` row, so its home is unknown.
    NoMachineRow,
    /// `~/`-relative, and the host's row says it does not expand `~` (NULL home).
    NoHome,
    /// Neither 0036 form. Unreachable through korg's own writes —
    /// `project_src_path_canonical` admits exactly the two — but a direct SQL
    /// write is not impossible.
    UnrecognisedForm,
}

impl Unlocated {
    fn reason(&self) -> &'static str {
        match self {
            Self::NoMachineRow => "no_machine_row",
            Self::NoHome => "host_does_not_expand_tilde",
            Self::UnrecognisedForm => "unrecognised_src_path",
        }
    }
}

/// How a stored `src_path` is spelled on its host, and the expansion that goes
/// with it. `home` is the host's `machine` row: `None` for no row,
/// `Some(None)` for a row whose home is NULL.
fn locate(
    src_path: &str,
    home: Option<&Option<String>>,
) -> Result<(String, &'static str), Unlocated> {
    if let Some(rest) = src_path.strip_prefix("~/") {
        return match home {
            None => Err(Unlocated::NoMachineRow),
            Some(None) => Err(Unlocated::NoHome),
            Some(Some(home)) => Ok((format!("{home}/{rest}"), "posix")),
        };
    }
    // `/d/ClaudeWorks/kctrldeck` -> `D:\ClaudeWorks\kctrldeck`. The inverse of
    // what 0036 accepts, and mechanical in both directions by design — that
    // reversibility is why the drive-root form was chosen over storing the
    // native spelling. Needs no home, so it does not consult the row.
    let bytes = src_path.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_lowercase() && bytes[2] == b'/' {
        let drive = (bytes[1] as char).to_ascii_uppercase();
        let rest = src_path[2..].replace('/', "\\");
        return Ok((format!("{drive}:{rest}"), "windows"));
    }
    Err(Unlocated::UnrecognisedForm)
}

/// The v1 row for one project, `None` if korg has nothing to place, or an
/// `unlocated` entry if it has a path and a host but cannot make the path
/// absolute.
///
/// The filter is the shape's own precondition rather than a policy: a project
/// with no `src_path` has no path to emit, and one with no `machines` has no
/// host to attach a path to. Both are silent omissions on purpose — a project
/// korg cannot locate is not an error, it is a project whose metadata nobody
/// has filled in, and failing the whole listing over one would take the panel
/// down for a missing field.
///
/// A path korg *has* but cannot expand is different: that is a missing
/// `machine` row (or a host that does not expand `~`), which somebody can fix,
/// so it is named in `unlocated` instead of vanishing (WI 2919).
fn project_row(
    p: &ProjectRow,
    homes: &HashMap<String, Option<String>>,
) -> Option<Result<Value, Value>> {
    let src_path = p.src_path.as_deref()?;
    let host = p.machines.first()?;
    let (path, kind) = match locate(src_path, homes.get(host)) {
        Ok(found) => found,
        Err(why) => {
            return Some(Err(json!({
                "name": p.name,
                "host": host,
                "src_path": src_path,
                "reason": why.reason(),
            })))
        }
    };

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
    Some(Ok(row))
}

/// `GET /api/connectors/projects` — the v1 listing.
///
/// Read-only and unauthenticated, like every other korg read: korg is no-auth
/// HTTP on the tailnet, and this endpoint exposes nothing `GET /api/projects`
/// and `GET /api/machines` do not already.
///
/// `unlocated` is additive within v1: a consumer that ignores it sees exactly
/// the listing it always did.
pub(crate) async fn projects(State(s): State<AppState>) -> ApiResult {
    let rows = repo::list_projects(&s.pool).await?;
    let homes = repo::machine_homes(&s.pool).await?;
    let (mut projects, mut unlocated) = (Vec::new(), Vec::new());
    // Archived projects are the point of the automation: one archived in korg
    // leaves the panel on the next refresh, with nothing hand-kept.
    for row in rows.iter().filter(|p| p.status == "active") {
        match project_row(row, &homes) {
            Some(Ok(v)) => projects.push(v),
            Some(Err(v)) => unlocated.push(v),
            None => {}
        }
    }

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
        "unlocated": unlocated,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(h: &str) -> Option<String> {
        Some(h.to_string())
    }

    #[test]
    fn a_tilde_path_expands_to_its_hosts_home() {
        assert_eq!(
            locate("~/src/tools/korg", Some(&home("/home/ken"))),
            Ok(("/home/ken/src/tools/korg".to_string(), "posix"))
        );
        // kimac: the host the old `/home/ken` constant was wrong for.
        assert_eq!(
            locate("~/src/x", Some(&home("/Users/ken"))),
            Ok(("/Users/ken/src/x".to_string(), "posix"))
        );
    }

    #[test]
    fn a_tilde_path_with_no_home_is_reported_not_guessed() {
        assert_eq!(locate("~/src/x", None), Err(Unlocated::NoMachineRow));
        assert_eq!(locate("~/src/x", Some(&None)), Err(Unlocated::NoHome));
    }

    #[test]
    fn a_drive_root_becomes_the_native_windows_spelling() {
        // A drive root needs no home, so neither a missing row nor a NULL home
        // stops it — cleo's row is NULL by design.
        for h in [None, Some(&None)] {
            assert_eq!(
                locate("/d/ClaudeWorks/kctrldeck", h),
                Ok((r"D:\ClaudeWorks\kctrldeck".to_string(), "windows"))
            );
        }
        assert_eq!(
            locate("/c/Users/kenhi/thing", None),
            Ok((r"C:\Users\kenhi\thing".to_string(), "windows"))
        );
    }

    /// The near-miss that matters: a bare POSIX absolute path must not be read
    /// as drive `h`. 0036's CHECK refuses to store one, and this is the second
    /// half of that guarantee — if one ever arrives by another route it is
    /// reported, not mangled into `H:\ome\ken\…`.
    #[test]
    fn an_unrecognised_form_is_reported_rather_than_guessed() {
        let h = Some(&home("/home/ken"));
        for p in ["/home/ken/src/x", "src/tools/korg", "~", "/d"] {
            assert_eq!(locate(p, h), Err(Unlocated::UnrecognisedForm), "{p}");
        }
    }
}
