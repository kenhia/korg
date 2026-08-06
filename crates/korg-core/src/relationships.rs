//! The relationship-label registry (WI #530).
//!
//! Sprint 008 made `relate()` directed and said so in the tool description —
//! but nothing declared *which* labels have a meaningful direction or what the
//! two ends mean, and two writers (`create_proposal`, `upsert_report`) stored
//! their edges id-canonicalized, so `covers` and `finding` orientation was a
//! function of node-id ordering rather than semantics. This module is the
//! single place that answers "what does this label mean, and does its
//! direction carry information?" — consulted by readers, quoted by the tool
//! descriptions, and published in `docs/api.md`.
//!
//! Per D-1, undirected labels keep whatever orientation they were stored with
//! and readers treat it as meaningless; there is no canonicalization migration.
//! Reverse duplicates of a *directed* label stay distinct and meaningful
//! (`A depends_on B` and `B depends_on A` is a cycle, not a duplicate).

/// What one known label means, and what its two endpoints are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LabelSpec {
    pub label: &'static str,
    /// True when the stored orientation carries meaning. False means readers
    /// must treat the edge as symmetric (D-1).
    pub directed: bool,
    /// Node kind expected on the left, or `None` where any kind is legitimate.
    pub left_kind: Option<&'static str>,
    /// Node kind expected on the right, or `None` for any.
    pub right_kind: Option<&'static str>,
    /// True when both endpoints must be in the same project (sprint 043,
    /// #967). Only `covers` sets it: a sprint proposal is a single-project
    /// bundle, and the cross-project layer is the program, not the proposal.
    /// `depends_on` deliberately does not — the homelab-ai plan is built out
    /// of cross-project dependencies.
    ///
    /// An endpoint with *no* project is unfiled rather than filed elsewhere
    /// and is not a violation; see [`crate::repo::relate`].
    pub same_project: bool,
    /// How the label reads left-to-right, for humans and tool descriptions.
    pub reads: &'static str,
}

/// Labels korg itself writes or interprets. Free-form labels stay legal —
/// [`spec`] returns `None` for them and their direction is caller-defined.
pub const REGISTRY: [LabelSpec; 7] = [
    LabelSpec {
        label: "covers",
        directed: true,
        left_kind: Some("sprint_proposal"),
        right_kind: Some("workitem"),
        same_project: true,
        reads: "proposal covers work item",
    },
    LabelSpec {
        label: "finding",
        directed: true,
        left_kind: Some("report"),
        right_kind: Some("workitem"),
        same_project: false,
        reads: "report reported work item as a finding",
    },
    LabelSpec {
        label: "depends_on",
        directed: true,
        left_kind: None,
        right_kind: None,
        same_project: false,
        reads: "dependent depends on dependency",
    },
    LabelSpec {
        label: "related-to",
        directed: false,
        left_kind: None,
        right_kind: None,
        same_project: false,
        reads: "the two nodes are related (no direction)",
    },
    LabelSpec {
        // Sprint 025: subject on the left, handoff on the right. Any node kind
        // may own a handoff (work item, proposal, report, another handoff), so
        // only the right endpoint is pinned. create_handoff writes the edge
        // directly (orientation correct by construction); relate() enforces this
        // for the generic MCP path (e.g. attaching an existing handoff later).
        label: "has_handoff",
        directed: true,
        left_kind: None,
        right_kind: Some("handoff"),
        same_project: false,
        reads: "node has handoff",
    },
    LabelSpec {
        // Sprint 044 (#968): the multi-project layer. A NEW label rather than a
        // widened `covers`, because `covers` is declared proposal -> workitem
        // *and* single-project; teaching it a second endpoint pair would mean
        // one registry entry with two meanings and a `same_project` that depends
        // on which pair matched — exactly what this registry exists to prevent.
        //
        // `same_project` is false and that is the entire point: a program is
        // where cross-project work became legal once 0022 made it illegal on a
        // proposal.
        //
        // The right end is pinned to `sprint_proposal` in v1. #968 also wants
        // "occasionally standalone WIs", which needs `right_kind` to become a
        // kind *set*; that is a mechanical change to six entries and is cheaper
        // later than shipping an unvalidated `None` now and never tightening it.
        label: "includes",
        directed: true,
        left_kind: Some("program"),
        right_kind: Some("sprint_proposal"),
        same_project: false,
        reads: "program includes proposal as a slice",
    },
    LabelSpec {
        // korg #1002 (kfdc sprint 003): the kfdc curator records mined
        // collisions between proposals — "same contract", "folds with
        // whichever lands second" — and `related-to` is too vacuous to drive a
        // Deconfliction card: a collision must be distinguishable from generic
        // relatedness without reading prose. A collision has no inherent
        // orientation, so undirected like `related-to`; any→any because the
        // colliding surfaces are not always two proposals (a proposal can
        // collide with a standalone WI), and cross-project because two repos
        // rewriting one published contract is exactly the case the panel
        // exists to surface.
        label: "collides-with",
        directed: false,
        left_kind: None,
        right_kind: None,
        same_project: false,
        reads: "the two nodes collide (same contract / fold on landing; no direction)",
    },
];

/// The registry entry for `label`, or `None` if it is a free-form label.
pub fn spec(label: &str) -> Option<&'static LabelSpec> {
    REGISTRY.iter().find(|s| s.label == label)
}

/// Whether `direction` on a neighbor entry carries information.
///
/// Registry-undirected labels are `false`. **Unknown labels are `true`**: korg
/// stores the caller's order faithfully, so the orientation is exactly what the
/// caller meant by it — korg simply doesn't know what that meaning is.
pub fn direction_is_meaningful(label: &str) -> bool {
    spec(label).is_none_or(|s| s.directed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_declares_the_labels_korg_writes() {
        assert!(spec("covers").unwrap().directed);
        assert_eq!(spec("covers").unwrap().left_kind, Some("sprint_proposal"));
        assert!(spec("finding").unwrap().directed);
        assert!(spec("depends_on").unwrap().directed);
        assert!(!spec("related-to").unwrap().directed);
        // has_handoff (sprint 025): any subject -> handoff, directed.
        assert!(spec("has_handoff").unwrap().directed);
        assert_eq!(spec("has_handoff").unwrap().left_kind, None);
        assert_eq!(spec("has_handoff").unwrap().right_kind, Some("handoff"));
        // includes (sprint 044): program -> proposal, directed, both ends pinned.
        assert!(spec("includes").unwrap().directed);
        assert_eq!(spec("includes").unwrap().left_kind, Some("program"));
        assert_eq!(
            spec("includes").unwrap().right_kind,
            Some("sprint_proposal")
        );
        // collides-with (korg #1002): undirected like related-to, any→any.
        assert!(!spec("collides-with").unwrap().directed);
        assert_eq!(spec("collides-with").unwrap().left_kind, None);
        assert_eq!(spec("collides-with").unwrap().right_kind, None);
    }

    /// The one that would quietly undo sprint 043 if it were ever "tidied up".
    /// `includes` is the layer where cross-project work became legal again; a
    /// well-meaning edit giving it `same_project: true` for symmetry with
    /// `covers` would leave the corpus with no way to express a multi-repo
    /// program at all, which is the entire reason #968 exists.
    #[test]
    fn includes_is_deliberately_cross_project() {
        assert!(!spec("includes").unwrap().same_project);
    }

    #[test]
    fn covers_is_the_only_single_project_label() {
        // Sprint 043 (#967). depends_on in particular must stay cross-project:
        // the homelab-ai plan's whole structure is dependencies between repos.
        assert!(spec("covers").unwrap().same_project);
        for s in REGISTRY.iter().filter(|s| s.label != "covers") {
            assert!(
                !s.same_project,
                "{} must not require a shared project",
                s.label
            );
        }
    }

    #[test]
    fn unknown_labels_are_caller_defined_and_keep_their_direction() {
        assert!(spec("part_of").is_none());
        assert!(direction_is_meaningful("part_of"));
        assert!(!direction_is_meaningful("related-to"));
    }
}
