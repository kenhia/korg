//! Collection reads: the envelope every list returns.

use serde::Serialize;
use ts_rs::TS;

// --- collection reads: the envelope every list returns ----------------------

/// The shape every collection read returns (WI #534, D-3). `total` is the full
/// filtered count *before* `limit`/`offset`, so a caller can page without
/// guessing and can tell a complete answer from a clipped one.
///
/// That holds on **every** page, including one whose `offset` overshoots the
/// last row: `items` is empty there and `total` still reports the corpus, so
/// `remaining = total - offset` and "trust the last page's total" both stay
/// sound (WI #883). Count in a statement of your own, never with a
/// `count(*) OVER()` riding on the paged rows — that one returns zero exactly
/// when the page is empty.
///
/// Unbounded list reads were the review's context bomb: `list_work_items`
/// returned every row with full content, which is why `survey_work_items` had
/// to exist at all. #861 made the lean projection *the* MCP list read, leaving
/// the survey a deprecated alias of it, and #871 deleted that alias once its
/// last caller moved. This envelope carries `omitted` alongside on the reads
/// that also narrow by default.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[ts(export, export_to = "korg.ts")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

impl<T> Page<T> {
    /// Assemble a page from an already-executed query.
    pub fn from_parts(items: Vec<T>, total: i64, limit: i64, offset: i64) -> Self {
        Self::new(items, total, limit, offset)
    }

    pub(super) fn new(items: Vec<T>, total: i64, limit: i64, offset: i64) -> Self {
        Self {
            items,
            total,
            limit,
            offset,
        }
    }
}

/// Default page size for collection reads. Generous enough that one project's
/// work items stay a single call (D-10), finite enough to bound the payload.
pub const LIST_LIMIT_DEFAULT: i64 = 200;
/// Hard ceiling a caller may request.
pub const LIST_LIMIT_MAX: i64 = 500;

/// Pagination knobs shared by every collection read. Defaults are applied in
/// [`PageQuery::resolve`], not here, so `None` means "use the documented
/// default" rather than "no limit".
#[derive(Debug, Clone, Copy, Default)]
pub struct PageQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl PageQuery {
    /// Clamped (limit, offset), for callers outside this module.
    pub fn resolve_public(&self) -> (i64, i64) {
        self.resolve()
    }

    /// Clamped (limit, offset) — callers can't escape the ceiling or go negative.
    pub(super) fn resolve(&self) -> (i64, i64) {
        (
            self.limit
                .unwrap_or(LIST_LIMIT_DEFAULT)
                .clamp(1, LIST_LIMIT_MAX),
            self.offset.unwrap_or(0).max(0),
        )
    }
}

/// `archived` filter shared by every collection read: `Some(false)` hides
/// archived rows, `Some(true)` shows only them, `None` means both.
///
/// The default is `Some(false)` (D-3) and it is declared **once**, here, so
/// core and both transports cannot drift apart on it. Ask for `None`
/// explicitly to see everything.
pub type ArchivedFilter = Option<bool>;

/// The archived default every collection read starts from.
pub fn archived_default() -> ArchivedFilter {
    Some(false)
}
