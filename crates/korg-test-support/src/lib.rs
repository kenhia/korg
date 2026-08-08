//! Shared test scaffolding for the korg workspace (WI #550).
//!
//! Before this crate, seventeen test files each carried their own copy of "start
//! a Postgres container, connect, migrate", and thirty `NewWorkItem { .. }`
//! literals spelled out thirteen fields apiece to set two of them. The copies
//! had drifted — some connected via `korg_core::connect`, some hand-rolled a
//! pool with a different `max_connections` — which meant a test could pass or
//! fail depending on which harness its file happened to inherit.
//!
//! What lives here:
//!
//! - [`start_pg`], the one place in the workspace that starts a container, and
//!   the two bootstraps over it — [`fresh_korg`] (migrated, what almost every
//!   suite wants) and [`raw_postgres`] (unmigrated, for suites whose subject
//!   *is* the migrator);
//! - [`count`], for the migrate suites; and
//! - [`new`], builders for the `New*` structs, which default every optional
//!   field so a test names only what it is actually asserting on.
//!
//! Surface-specific scaffolding (the MCP `server()`/`call()` wrappers, the
//! korg-api `req()` helper) deliberately does *not* live here: this crate would
//! then depend on `korg-mcp`/`korg-api` while being their dev-dependency. That
//! scaffolding sits in each crate's own `tests/common/` instead.

use rust_decimal::Decimal;
use sqlx::PgPool;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::{ContainerAsync, ImageExt};

/// A running Postgres container and its mapped port.
///
/// Holds the [`ContainerAsync`] because dropping it stops the container — the
/// reason every helper here returns something the caller must bind.
pub struct Pg {
    pub container: ContainerAsync<Postgres>,
    pub port: u16,
}

impl Pg {
    pub fn url(&self, db: &str) -> String {
        format!(
            "postgres://postgres:postgres@127.0.0.1:{}/{}",
            self.port, db
        )
    }
}

/// The one place in the workspace that starts a Postgres container.
///
/// Pinned to 18-alpine: the migrate suites restore pg_dump-18 (kwi) and
/// pg_dump-16 (kcard) archives, which a server older than 18 cannot read.
/// Everything else inherits the pin so that "works in tests" means the same
/// server version everywhere.
pub async fn start_pg() -> Pg {
    let container = Postgres::default()
        .with_tag("18-alpine")
        .start()
        .await
        .expect("start postgres container");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("mapped port");
    Pg { container, port }
}

/// A fresh, migrated korg database in a throwaway container.
///
/// The returned [`Pg`] must be held for as long as the pool is used. Bind it,
/// do not `let _ = `:
///
/// ```ignore
/// let (_pg, pool) = fresh_korg().await;
/// ```
///
/// Migration runs through `korg_core::connect`, the same path production takes,
/// so a migration that only works under the test harness cannot exist.
pub async fn fresh_korg() -> (Pg, PgPool) {
    let pg = start_pg().await;
    let pool = korg_core::connect(&pg.url("postgres"))
        .await
        .expect("connect+migrate");
    (pg, pool)
}

/// A container and an *unmigrated* pool, for tests whose subject is the
/// migrator itself (the schema, identity, and fresh-install-sequence suites).
pub async fn raw_postgres() -> (Pg, PgPool) {
    let pg = start_pg().await;
    let pool = connect(&pg.url("postgres")).await;
    (pg, pool)
}

/// Connect a pool without migrating. The migrate suites use this against the
/// restored snapshot databases, which have their own (legacy) schemas.
pub async fn connect(url: &str) -> PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(url)
        .await
        .expect("connect to postgres")
}

/// The project a suite files things under when it does not care which one.
///
/// Sprint 043 made a project mandatory on every proposal, so `new::proposal`
/// names this one and the suite creates it — one line — rather than every
/// proposal test growing a routing story it has no opinion about. Suites that
/// *are* about routing name their own projects instead.
pub const TEST_PROJECT: &str = "korg";

/// Create [`TEST_PROJECT`], returning its id. Idempotent, like
/// `create_project` itself, so calling it twice in one suite is fine.
pub async fn test_project(pool: &PgPool) -> i64 {
    korg_core::repo::create_project(pool, TEST_PROJECT)
        .await
        .expect("create the default test project")
}

/// Row count of `table`. Only ever called with literal table names from the
/// migrate suites, so the format-string interpolation is not a lever an input
/// can reach.
pub async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("count {table}: {e}"))
}

/// Builders for the `New*` argument structs.
///
/// Each returns a fully-defaulted value; callers override the two or three
/// fields the test is about with struct-update syntax:
///
/// ```ignore
/// create_work_item(&pool, NewWorkItem { wi_status: "done".into(), ..new::work_item("ship it") })
/// ```
///
/// The defaults match the `#[serde(default)]` values the MCP and REST surfaces
/// apply, so a builder-built struct is what an agent sending a minimal request
/// would produce.
pub mod new {
    use super::Decimal;
    use korg_core::repo::{
        NewAttachment, NewAttachmentVariant, NewCard, NewHandoff, NewLink, NewProgram, NewProposal,
        NewReport, NewSchedule, NewWorkItem,
    };

    pub fn work_item(title: &str) -> NewWorkItem {
        NewWorkItem {
            project_id: None,
            project: None,
            area_id: None,
            area: None,
            wi_type: "task".into(),
            wi_status: "open".into(),
            wi_tshirt: "Unknown".into(),
            sprint: None,
            title: title.into(),
            content: String::new(),
            details: None,
            category: None,
            tags: Vec::new(),
        }
    }

    pub fn card(title: &str) -> NewCard {
        NewCard {
            project_id: None,
            project: None,
            category: None,
            tags: Vec::new(),
            status: "Backlog".into(),
            title: title.into(),
            description: String::new(),
            rank: Decimal::ZERO,
        }
    }

    pub fn link(url: &str) -> NewLink {
        NewLink {
            project_id: None,
            project: None,
            category: None,
            tags: Vec::new(),
            url: url.into(),
            title: None,
        }
    }

    /// A proposal in [`TEST_PROJECT`](super::TEST_PROJECT).
    ///
    /// Unlike the other builders this one is *not* all-defaults: sprint 043
    /// made `project` mandatory, so the honest default is the project the
    /// suite will have created with [`test_project`](super::test_project).
    /// Use [`proposal_in`] when the project is what the test is about.
    /// A schedule in [`TEST_PROJECT`], anchored `now` unless the caller moves it.
    /// `cadence` and `anchor_at` are the two fields every schedule test is
    /// actually about, so they are the arguments; everything else defaults.
    pub fn schedule(
        title: &str,
        cadence: &str,
        anchor_at: Option<time::OffsetDateTime>,
    ) -> NewSchedule {
        NewSchedule {
            title: title.into(),
            template: None,
            notes: None,
            cadence: cadence.into(),
            anchor_mode: "completed".into(),
            anchor_at,
            wi_type: "maintenance".into(),
            wi_tshirt: "Unknown".into(),
            project_id: None,
            project: Some(super::TEST_PROJECT.into()),
            category: None,
            tags: Vec::new(),
        }
    }

    pub fn proposal(title: &str) -> NewProposal {
        proposal_in(super::TEST_PROJECT, title)
    }

    /// A proposal in a named project — for suites where the routing matters.
    pub fn proposal_in(project: &str, title: &str) -> NewProposal {
        NewProposal {
            project_id: None,
            project: Some(project.into()),
            category: None,
            tags: Vec::new(),
            title: title.into(),
            summary: String::new(),
            notes: None,
            rank: Decimal::ZERO,
            pinned: false,
            covers: Vec::new(),
        }
    }

    /// A program (#968). Note the absence of any project selector in the
    /// defaults: a program carries none by construction (D-6), and the two
    /// fields that exist on `NewProgram` are there only so passing one can be
    /// *refused*. `slices` are proposal node_ids, in order.
    pub fn program(title: &str) -> NewProgram {
        NewProgram {
            title: title.into(),
            aim: format!("{title} aim"),
            notes: None,
            rank: Decimal::ZERO,
            pinned: false,
            category: None,
            tags: Vec::new(),
            slices: Vec::new(),
            project: None,
            project_id: None,
        }
    }

    /// A handoff with one owner is the realistic case; tests that want more,
    /// or the standalone opt-in, set `related_node_ids`/`allow_standalone`.
    pub fn handoff(title: &str) -> NewHandoff {
        NewHandoff {
            project_id: None,
            project: None,
            category: None,
            tags: Vec::new(),
            title: title.into(),
            summary: format!("{title} summary"),
            body: String::new(),
            related_node_ids: Vec::new(),
            allow_standalone: false,
        }
    }

    /// An image attachment, as korg-img would have measured it off a 1200×800
    /// PNG screenshot (sprint 056).
    ///
    /// Metadata only: `create_attachment` records an upload, and the *bytes*
    /// are korg-api's half. A suite that needs real pixels on disk wants
    /// `korg-api/tests/images.rs`, which uploads through the router.
    ///
    /// `owner_node_id` is `None` — the paste-before-save state — because that
    /// is the one an attachment test has to opt out of deliberately: an
    /// unclaimed attachment is what the sweeper collects.
    pub fn attachment(filename: &str) -> NewAttachment {
        NewAttachment {
            filename: filename.into(),
            mime: "image/png".into(),
            byte_size: 40_000,
            width: 1200,
            height: 800,
            content_hash: "a".repeat(64),
            variants: vec![
                NewAttachmentVariant {
                    variant: "thumb".into(),
                    mime: "image/png".into(),
                    width: 400,
                    height: 267,
                    byte_size: 3_000,
                },
                NewAttachmentVariant {
                    variant: "agent".into(),
                    mime: "image/png".into(),
                    width: 1200,
                    height: 800,
                    byte_size: 30_000,
                },
            ],
            owner_node_id: None,
            tags: Vec::new(),
        }
    }

    /// `report_date` has no sensible default — a report is identified by
    /// `(source, report_date)`, so both are arguments.
    pub fn report(source: &str, report_date: time::Date) -> NewReport {
        NewReport {
            source: source.into(),
            report_date,
            status: "ok".into(),
            summary: format!("{source} report"),
            body: String::new(),
            model: None,
            escalated: false,
            findings: Vec::new(),
        }
    }

    /// The builders hard-code the same vocabulary values the surfaces' serde
    /// defaults produce. Hard-coding is the point — a builder that read
    /// `vocab::CARD_STATUSES[0]` would silently follow a reordering of the
    /// vocabulary instead of failing — but a value that leaves the vocabulary
    /// entirely must not go unnoticed, which is what this fences.
    #[cfg(test)]
    mod tests {
        use korg_core::vocab;

        #[test]
        fn builder_defaults_are_valid_vocabulary_members() {
            let wi = super::work_item("t");
            assert!(vocab::WI_TYPES.contains(&wi.wi_type.as_str()));
            assert!(vocab::WI_STATUSES.contains(&wi.wi_status.as_str()));
            assert!(vocab::WI_TSHIRTS.contains(&wi.wi_tshirt.as_str()));
            assert!(vocab::CARD_STATUSES.contains(&super::card("t").status.as_str()));
            assert!(vocab::REPORT_STATUSES.contains(
                &super::report("s", time::macros::date!(2026 - 07 - 23))
                    .status
                    .as_str()
            ));
        }
    }
}
