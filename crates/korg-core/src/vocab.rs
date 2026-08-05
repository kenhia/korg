//! The domain vocabulary, validated app-side at every write boundary (WI #526).
//!
//! These sets used to live in four places at once: Rust consts, DB CHECK
//! constraints and enum casts, hand-written MCP JSON schemas, and TypeScript
//! const-tuples. The DB copies were doing the enforcing, which meant a typo'd
//! t-shirt size or card status came back as a 500 with raw Postgres text.
//! korg-core is now the authority; the DB constraints are a backstop.
//!
//! `wi_type` was entirely free text before this (D-2). The vocabulary is the
//! union of what the live corpus actually uses plus `chore`, so nothing
//! existing is invalidated.

use crate::error::RepoError;

/// Canonical work-item statuses (WI #285). Lifecycle: `open → resolved`
/// (implemented; may still need a user test / may not be PR'd) `→ done`
/// (agent satisfied — terminal but still visible in default lists)
/// `→ closed` (Ken only; hidden by default).
pub const WI_STATUSES: [&str; 4] = ["open", "resolved", "done", "closed"];

/// The work items a list read means by default (WI #861), i.e. everything that
/// is not terminal. The program plan's §8 answer 1 decided the split: `closed`
/// is 78% of the corpus and is Ken's own "I am finished with this" marker;
/// `resolved` and `done` stay visible, because `done`'s visibility is already a
/// promise `update_work_item`'s schema makes ("terminal but visible in default
/// lists") and `resolved` is the may-still-want-to-see state.
pub const WI_LIVE_STATUSES: [&str; 3] = ["open", "resolved", "done"];

/// The complement of [`WI_LIVE_STATUSES`] — excluded from a lean
/// `list_work_items` unless asked for, and counted in its `omitted`. Kept beside
/// the full vocabulary so a fifth status cannot be added without deciding which
/// side it falls on; `wi_statuses_partition_cleanly` fences that.
pub const WI_TERMINAL_STATUSES: [&str; 1] = ["closed"];

/// Work-item types (D-2). `brainstorm` is deliberate: half-formed ideas get
/// filed as work items rather than lost.
pub const WI_TYPES: [&str; 7] = [
    "task",
    "bug",
    "chore",
    "feature",
    "research",
    "tweak",
    "brainstorm",
];

/// T-shirt sizes; mirrors the `wi_tshirt` CHECK in migration 0001.
pub const WI_TSHIRTS: [&str; 7] = ["XS", "S", "M", "L", "XL", "Huge", "Unknown"];

/// Kanban columns; mirrors the `card_status` enum in migration 0001.
pub const CARD_STATUSES: [&str; 6] = ["Backlog", "Research", "OnDeck", "Active", "Done", "Cut"];

/// Reading-list dispositions; mirrors the `link_disposition` enum (0004).
pub const LINK_DISPOSITIONS: [&str; 5] = ["Unread", "Done", "Revisit", "Summarized", "VaultSaved"];

/// Sprint-proposal lifecycle; mirrors `sprint_proposal_status` (0008).
pub const PROPOSAL_STATUSES: [&str; 4] = ["proposed", "active", "done", "declined"];

/// The proposals a queue read means by default (WI #852): the live ones. The
/// complement — `done` and `declined` — was 71% of `list_proposals`' payload and
/// nothing an agent picking a sprint has ever wanted. Kept beside the full
/// vocabulary so a fifth status cannot be added without deciding which side it
/// falls on; `proposal_statuses_partition_cleanly` fences that.
pub const PROPOSAL_LIVE_STATUSES: [&str; 2] = ["proposed", "active"];

/// The complement of [`PROPOSAL_LIVE_STATUSES`] — excluded from a lean
/// `list_proposals` unless asked for, and counted in its `omitted`.
pub const PROPOSAL_TERMINAL_STATUSES: [&str; 2] = ["done", "declined"];

/// Program lifecycle (#968, sprint 044); mirrors the `program.status` CHECK
/// (0023). A program is the cross-project layer: it `includes` proposals,
/// ordered, and outlives any one of them.
///
/// `holding` is the state a program spends most of its life in and the reason
/// this is not just `active`/`done` — a program whose current slice has shipped
/// and whose next one is not started is neither finished nor in flight, and
/// collapsing that into `active` would make the Operations panel claim work is
/// underway when nobody is on it.
pub const PROGRAM_STATUSES: [&str; 3] = ["active", "holding", "done"];

/// The programs a list read means by default — the ones still going. Same split
/// as proposals and work items, and fenced by the same partition test so a
/// fourth status cannot be added without deciding which side it falls on.
pub const PROGRAM_LIVE_STATUSES: [&str; 2] = ["active", "holding"];

/// The complement of [`PROGRAM_LIVE_STATUSES`] — excluded from a default
/// `list_programs` and counted in its `omitted`.
pub const PROGRAM_TERMINAL_STATUSES: [&str; 1] = ["done"];

/// Daily-report statuses; mirrors the `report.status` CHECK (0010).
pub const REPORT_STATUSES: [&str; 3] = ["ok", "attention", "problem"];

/// Project lifecycle statuses (WI #246, narrowed to two by #828). Default
/// WI-page rail shows only `active` unless "show all" is on.
///
/// `maintenance` and `inactive` were dropped in 0020 on the same argument: both
/// held **zero rows** and neither had semantics distinct from `archived`, which
/// makes an unused vocabulary value a standing invitation for a future writer to
/// invent a meaning nobody agreed. The live corpus was already 27 `active` /
/// 8 `archived` when they were removed.
///
/// The status answers one question — *is this row a legitimate target for new
/// work?* — and that question has two answers. Where `archived` covers two
/// genuinely different situations (superseded: `kwi`/`kcard` → korg; dormant:
/// `trt-llm-explore`), the difference lives in the `description` prose, because
/// both filter identically and a status exists to filter.
pub const PROJECT_STATUSES: [&str; 2] = ["active", "archived"];

/// The one status that is a legitimate target for new work — the question
/// above, named, so the write paths that enforce it (`resolve_project`,
/// `resolve_project_patch`; WI #884) read as the sentence rather than as a bare
/// string comparison, and so a third status could not be added without this
/// line forcing the "which side?" decision.
pub const PROJECT_STATUS_ACTIVE: &str = PROJECT_STATUSES[0];

/// Project categories (WI #678). `project.category` has existed since 0011 but
/// as free text, and it drifted the way free text does — the live corpus held
/// `ai` x8, `AI` x1, `tooling` x3, `infra` x2, `fun` x2 and NULL x15. This
/// closes the set the way LB-2 closed the relationship labels, and the Work
/// Items rail colors from it instead of hashing the project *name* (which put
/// 20 of 31 projects within 12 degrees of another while leaving whole arcs of
/// the wheel empty).
///
/// Adding a category is one edit here plus its hue in `CATEGORY_HUE` in
/// `web/src/lib/domain.ts`, then `just gen`. `EVAL` is the worked example:
/// added 2026-07-26 for harness-evaluation projects that leaked into the real
/// corpus, so that they are findable as a group.
pub const PROJECT_CATEGORIES: [&str; 7] = [
    "AI",
    "Dashboard",
    "EVAL",
    "Fun",
    "Infrastructure",
    "Ops",
    "Other",
];

/// Every vocabulary, as `(const name, TypeScript type name, values)`. Adding a
/// set here is all it takes for the web app to see it: `just gen` writes them
/// into `web/src/lib/generated/vocab.ts`, so the UI's status lists stop being a
/// hand-kept copy that drifts (the old `api.ts` `WI_TYPES` had nine entries,
/// six of which the server rejects).
#[cfg(test)]
const EXPORTED: [(&str, &str, &[&str]); 11] = [
    ("WI_STATUSES", "WiStatus", &WI_STATUSES),
    ("WI_TYPES", "WiType", &WI_TYPES),
    ("WI_TSHIRTS", "WiTshirt", &WI_TSHIRTS),
    ("CARD_STATUSES", "CardStatus", &CARD_STATUSES),
    ("LINK_DISPOSITIONS", "Disposition", &LINK_DISPOSITIONS),
    ("PROPOSAL_STATUSES", "ProposalStatus", &PROPOSAL_STATUSES),
    ("PROGRAM_STATUSES", "ProgramStatus", &PROGRAM_STATUSES),
    ("REPORT_STATUSES", "ReportStatus", &REPORT_STATUSES),
    ("PROJECT_STATUSES", "ProjectStatus", &PROJECT_STATUSES),
    ("PROJECT_CATEGORIES", "ProjectCategory", &PROJECT_CATEGORIES),
    // Not a domain vocabulary but the same kind of fact: a closed set the
    // client must agree with. The web app branches on it to tell "you typed
    // something wrong" from "korg broke" (sprint 019).
    ("ERROR_CODES", "ErrorCode", &crate::error::ERROR_CODES),
];

/// Reject a value outside its vocabulary with the full allowed set in the
/// message — the error doubles as the documentation an agent needs to retry.
pub fn validate(value: &str, allowed: &[&str], what: &str) -> Result<(), RepoError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(RepoError::InvalidInput(format!(
            "invalid {what} '{value}' — expected one of: {}",
            allowed.join(", ")
        )))
    }
}

/// Emit the vocabularies — and the relationship-label registry — as
/// TypeScript, next to the `ts-rs` output (WI #541).
///
/// This runs as a test because that is where `ts-rs` generates too, so one
/// `just gen` invocation produces the whole `web/src/lib/generated/` directory
/// and CI's `git diff --exit-code` covers all of it. Naming it
/// `export_bindings_*` puts it in the same `cargo test export_bindings` filter
/// `ts-rs` uses.
#[cfg(test)]
mod partition {
    use super::{
        PROGRAM_LIVE_STATUSES, PROGRAM_STATUSES, PROGRAM_TERMINAL_STATUSES, PROPOSAL_LIVE_STATUSES,
        PROPOSAL_STATUSES, PROPOSAL_TERMINAL_STATUSES, WI_LIVE_STATUSES, WI_STATUSES,
        WI_TERMINAL_STATUSES,
    };
    use std::collections::BTreeSet;

    /// A lean `list_proposals` shows the live statuses and counts the terminal
    /// ones in `omitted` (WI #852). A fifth proposal status added to the enum
    /// and to `PROPOSAL_STATUSES` but to neither half would be silently
    /// invisible *and* uncounted — the exact failure `omitted` exists to stop.
    #[test]
    fn proposal_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = PROPOSAL_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = PROPOSAL_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = PROPOSAL_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a proposal status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every proposal status must be classified live or terminal — \
             list_proposals' default and its `omitted` counts both depend on it"
        );
    }

    /// The same fence for work items (WI #861), and it matters more here: a
    /// fifth work-item status in neither half would drop out of the default
    /// `list_work_items` **and** out of `omitted.closed`, which is precisely the
    /// silent under-count the envelope exists to prevent.
    #[test]
    fn wi_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = WI_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = WI_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = WI_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a work-item status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every work-item status must be classified live or terminal — \
             list_work_items' default and its `omitted` counts both depend on it"
        );
    }

    /// And for programs (#968, sprint 044). `holding` is the value this fence
    /// exists for: it is neither in flight nor finished, and a fourth status
    /// with the same ambiguity must not be able to slip in unclassified.
    #[test]
    fn program_statuses_partition_cleanly() {
        let all: BTreeSet<&str> = PROGRAM_STATUSES.into_iter().collect();
        let live: BTreeSet<&str> = PROGRAM_LIVE_STATUSES.into_iter().collect();
        let terminal: BTreeSet<&str> = PROGRAM_TERMINAL_STATUSES.into_iter().collect();
        assert!(
            live.is_disjoint(&terminal),
            "a program status is both live and terminal: {:?}",
            live.intersection(&terminal).collect::<Vec<_>>()
        );
        assert_eq!(
            all,
            live.union(&terminal).copied().collect::<BTreeSet<&str>>(),
            "every program status must be classified live or terminal — \
             list_programs' default and its `omitted` counts both depend on it"
        );
    }
}

#[cfg(test)]
mod generate {
    use super::EXPORTED;
    use crate::relationships::REGISTRY;

    fn quoted(values: &[&str]) -> String {
        values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn render() -> String {
        let mut out = String::from(
            "// This file is generated by `just gen` from korg-core. Do not edit this file manually.\n",
        );
        for (name, ty, values) in EXPORTED {
            out.push_str(&format!(
                "\nexport const {name} = [{}] as const;\nexport type {ty} = (typeof {name})[number];\n",
                quoted(values)
            ));
        }
        out.push_str(
            "\n/** The complete, closed set of relationship labels korg accepts (LB-2):\n \
             *  relate() rejects anything else. Extend it by adding a registry entry in\n \
             *  korg-core and running `just gen`. */\nexport const \
             RELATIONSHIP_LABELS = [\n",
        );
        for spec in REGISTRY {
            out.push_str(&format!(
                "  {{ label: \"{}\", directed: {}, reads: \"{}\" }},\n",
                spec.label, spec.directed, spec.reads
            ));
        }
        out.push_str("] as const;\nexport type RelationshipLabel = (typeof RELATIONSHIP_LABELS)[number][\"label\"];\n");
        out
    }

    #[test]
    fn export_bindings_vocab() {
        let dir = std::env::var("TS_RS_EXPORT_DIR").unwrap_or_else(|_| "./bindings".into());
        std::fs::create_dir_all(&dir).expect("create export dir");
        std::fs::write(std::path::Path::new(&dir).join("vocab.ts"), render())
            .expect("write vocab.ts");
    }
}
