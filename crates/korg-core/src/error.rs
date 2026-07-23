//! One error taxonomy for korg-core (WI #524).
//!
//! Before this, the crate had three regimes — `RepoError` (mapped to 4xx),
//! `PlanningError` (mapped precisely), and bare `anyhow::bail!` (always 500) —
//! so invalid dates, unknown reports, bad t-shirt sizes and FK violations all
//! surfaced to agents as 500s with raw DB text. Everything now funnels into
//! `RepoError`, and every transport asks the same question of an error: what
//! is its [`ErrorCode`]?
//!
//! `PlanningError` keeps its precise variants (they carry planning-specific
//! context the daily-plan surfaces use) but maps into the same codes.

use crate::daily_plan::PlanningError;

/// Domain errors every surface translates to 4xx rather than 500. Carried
/// through `anyhow` and recovered by `downcast_ref` at the transport edge.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    /// Caller passed a bad value (unknown status, area not in project, …) → 400.
    #[error("{0}")]
    InvalidInput(String),
    /// Named/keyed entity doesn't exist (no project X, no card N, …) → 404.
    #[error("{0}")]
    NotFound(String),
    /// The request is well-formed but conflicts with server-enforced state
    /// (frozen past, stale reorder) → 409.
    #[error("{0}")]
    Conflict(String),
}

impl RepoError {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidInput(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
}

/// The stable, machine-readable classification carried on every error
/// response: `code` in REST bodies, `code` in MCP error content (D-5). Agents
/// branch on this instead of pattern-matching prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidInput,
    NotFound,
    Conflict,
    Internal,
}

impl ErrorCode {
    /// Every code, for exhaustive iteration. Kept next to the enum so adding a
    /// variant without listing it here fails `error_codes_are_exhaustive`.
    pub const ALL: [Self; 4] = [
        Self::InvalidInput,
        Self::NotFound,
        Self::Conflict,
        Self::Internal,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Internal => "internal",
        }
    }
}

/// The codes as strings, exported to TypeScript alongside the vocabularies so
/// the web app can branch on `code` without hand-mirroring the list (sprint
/// 019). `invalid_input` is the user's problem and reads as a correction;
/// `internal` is korg's and reads as an apology — same HTTP failure, different
/// UI, and only this field tells them apart.
pub const ERROR_CODES: [&str; 4] = ["invalid_input", "not_found", "conflict", "internal"];

/// Anything a transport can classify. Implemented for the two typed errors and
/// for `anyhow::Error` (which downcasts to them, defaulting to `Internal`).
pub trait ErrorClass {
    fn code(&self) -> ErrorCode;
}

impl ErrorClass for RepoError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidInput(_) => ErrorCode::InvalidInput,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Conflict(_) => ErrorCode::Conflict,
        }
    }
}

impl ErrorClass for PlanningError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::SourceNotFound(_) | Self::ItemNotFound(_) => ErrorCode::NotFound,
            Self::WrongSource { .. } | Self::TargetPast | Self::InvalidRange(_) => {
                ErrorCode::InvalidInput
            }
            Self::FrozenPast | Self::InvalidReorder => ErrorCode::Conflict,
            Self::Database(_) => ErrorCode::Internal,
        }
    }
}

impl ErrorClass for anyhow::Error {
    fn code(&self) -> ErrorCode {
        if let Some(e) = self.downcast_ref::<RepoError>() {
            return e.code();
        }
        if let Some(e) = self.downcast_ref::<PlanningError>() {
            return e.code();
        }
        ErrorCode::Internal
    }
}

#[cfg(test)]
mod code_tests {
    use super::{ErrorCode, ERROR_CODES};

    #[test]
    fn error_codes_are_exhaustive() {
        let from_enum: Vec<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            from_enum, ERROR_CODES,
            "ERROR_CODES must list exactly what ErrorCode::ALL renders — the \
             TypeScript union is generated from it",
        );
    }
}
