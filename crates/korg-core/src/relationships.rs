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
    /// How the label reads left-to-right, for humans and tool descriptions.
    pub reads: &'static str,
}

/// Labels korg itself writes or interprets. Free-form labels stay legal —
/// [`spec`] returns `None` for them and their direction is caller-defined.
pub const REGISTRY: [LabelSpec; 4] = [
    LabelSpec {
        label: "covers",
        directed: true,
        left_kind: Some("sprint_proposal"),
        right_kind: Some("workitem"),
        reads: "proposal covers work item",
    },
    LabelSpec {
        label: "finding",
        directed: true,
        left_kind: Some("report"),
        right_kind: Some("workitem"),
        reads: "report reported work item as a finding",
    },
    LabelSpec {
        label: "depends_on",
        directed: true,
        left_kind: None,
        right_kind: None,
        reads: "dependent depends on dependency",
    },
    LabelSpec {
        label: "related-to",
        directed: false,
        left_kind: None,
        right_kind: None,
        reads: "the two nodes are related (no direction)",
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
    }

    #[test]
    fn unknown_labels_are_caller_defined_and_keep_their_direction() {
        assert!(spec("part_of").is_none());
        assert!(direction_is_meaningful("part_of"));
        assert!(!direction_is_meaningful("related-to"));
    }
}
