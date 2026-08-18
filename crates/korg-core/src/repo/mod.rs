//! korg-core repository layer: typed creation of nodes (work items, cards,
//! reading-list links) and generalized cross-kind relationships.
//!
//! Every entity is a `node`; kind-specific data lives in a detail table; any
//! two nodes can be linked through a single `relationship` edge regardless of
//! kind. This is the API the MCP/CLI/web surfaces (M4/M5) build on.
//!
//! # Layout
//!
//! One module per domain, split (#1345) out of the single 8,151-line
//! `repo.rs` this used to be, along the `// --- section ---` markers that file
//! had already grown. [`common`] and [`selectors`] are the plumbing the rest
//! reach for — existence and kind checks, the name-or-id resolvers; every
//! other module is one node kind or one read surface.
//!
//! The split is organisational only. Every public item is re-exported here,
//! so `repo::` stays the flat namespace it has always been and nothing
//! outside this crate names a submodule.
//!
//! Those re-exports are globs on purpose. An explicit list is a second place
//! to forget a new item — and the guarantee worth having is that moving a
//! function between these modules cannot change what a caller sees. `rustc`
//! still refuses an ambiguous glob, so no two modules can export one name.

mod areas;
mod attachments;
mod awaiting;
mod board;
mod cards;
mod comments;
mod common;
mod flow;
mod handoffs;
mod links;
mod page;
mod planning;
mod preview;
mod programs;
mod projects;
mod proposals;
mod relationships;
mod reports;
mod schedules;
mod search;
mod selectors;
mod work_items;

pub use crate::error::RepoError;
pub use crate::vocab::{PROJECT_CATEGORIES, PROJECT_STATUSES, WI_STATUSES};

pub use areas::*;
pub use attachments::*;
pub use awaiting::*;
pub use board::*;
pub use cards::*;
pub use comments::*;
pub use flow::*;
pub use handoffs::*;
pub use links::*;
pub use page::*;
pub use planning::*;
pub use preview::*;
pub use programs::*;
pub use projects::*;
pub use proposals::*;
pub use relationships::*;
pub use reports::*;
pub use schedules::*;
pub use search::*;
pub use work_items::*;
