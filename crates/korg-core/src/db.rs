//! Resolving korg's database connection, and the one connect+migrate path.
//!
//! # Why the password is a variable of its own
//!
//! korg used to read one `DATABASE_URL` with the password embedded in it, which
//! meant `/datastore/korg/korg.env` on kubsdb held a second copy of a password
//! that `/etc/khomelab/secrets.env` already had (korg #2547, program #2440 —
//! "passwords get one copy per host"). Now the per-host file supplies
//! `KORG_DB_PASSWORD` and korg puts it together with a `DATABASE_URL` that
//! carries no credential.
//!
//! **Compose cannot do this for us.** `env_file:` inside a compose file injects
//! the file's own key names into the container and is invisible to compose's
//! `${VAR}` interpolation; only `docker compose --env-file <path>` feeds
//! interpolation, and that puts nothing in the container. So a compose file
//! that wrote `DATABASE_URL: "postgres://korg:${KORG_DB_PASSWORD}@…"` would
//! resolve to an empty password unless every `docker compose` command in
//! `/datastore/korg` remembered the flag — `down`, `logs` and `ps` included.
//! Building the URL here removes that coupling entirely (measured by k-homelab
//! sprint 062, korg:2565).
//!
//! # Why `PgConnectOptions::password` and not string surgery
//!
//! Splicing a value into `postgres://korg:<here>@host/db` makes `@` and `/`
//! corrupt the authority section, which surfaces as a connection failure that
//! reads exactly like a wrong password. `deploy/cold-start.sh` used to *refuse*
//! any password containing either character for that reason — a constraint on
//! what the fleet's rotation tooling may generate, imposed by a string format.
//! Setting the password on the parsed options has no such hazard, so korg
//! accepts any value the age store holds and the refusal is gone.

use std::str::FromStr;

use anyhow::{anyhow, Result};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;

use crate::migrator;

/// Connect a pool to `url` and ensure the schema is migrated.
///
/// The URL-taking entry point, for tests and the importer. The deployed binary
/// goes through [`connect_options_from_env`] instead, but lands in the same
/// [`connect_with`] below — there is one connect+migrate path in the workspace
/// and this is it.
pub async fn connect(url: &str) -> Result<PgPool> {
    connect_with(PgConnectOptions::from_str(url)?).await
}

/// Connect a pool with already-resolved options and ensure the schema is
/// migrated.
pub async fn connect_with(options: PgConnectOptions) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    migrator().run(&pool).await?;
    Ok(pool)
}

/// Resolve the database connection from `DATABASE_URL` and the optional
/// `KORG_DB_PASSWORD`.
pub fn connect_options_from_env() -> Result<PgConnectOptions> {
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| anyhow!("DATABASE_URL is required"))?;
    let password = std::env::var("KORG_DB_PASSWORD").ok();
    resolve_connect_options(&database_url, password.as_deref())
}

/// The rules, separated from the environment so they can be tested without one.
///
/// - No `KORG_DB_PASSWORD` — the URL is used as it stands. This is the
///   local-development shape (`postgres://korg:korg@localhost/korg`) and the
///   importer's, and it is why this change needed no test or dev-loop edits.
/// - `KORG_DB_PASSWORD` set, URL carries no password — the deployed shape. The
///   password is applied to the parsed options.
/// - **Both** — an error, not a precedence rule. See below.
/// - Set but empty — an error.
///
/// Two sources for one credential is refused rather than resolved because the
/// question program #2440 exists to make answerable is *"is this consumer off
/// its own copy?"*, and a fallback makes that unanswerable from config: a
/// deployment that kept a stale password in `DATABASE_URL` would keep working,
/// silently, on whichever copy won. The klams client slices adopted the same
/// rule for identity-plus-token (program #2440 ruling 1); this is that ruling
/// applied to a password.
pub fn resolve_connect_options(
    database_url: &str,
    db_password: Option<&str>,
) -> Result<PgConnectOptions> {
    // The URL is deliberately absent from this error: in the local-development
    // shape it still carries the password inline, and a parse failure is
    // precisely the moment it would otherwise be written to a log.
    let options = PgConnectOptions::from_str(database_url)
        .map_err(|_| anyhow!("DATABASE_URL is not a valid PostgreSQL connection URL"))?;

    let Some(password) = db_password else {
        return Ok(options);
    };

    if password.is_empty() {
        return Err(anyhow!(
            "KORG_DB_PASSWORD is set but empty. korg refuses to start rather than \
             connect as a passwordless role: on kubsdb an empty value means \
             /etc/khomelab/secrets.env rendered wrong, and the connection failure \
             it would cause reads like a Postgres problem instead. Re-render it \
             with `bin/apply kubsdb khomelab-secrets`, or unset the variable to \
             take the password from DATABASE_URL."
        ));
    }

    if url_carries_password(database_url) {
        return Err(anyhow!(
            "DATABASE_URL carries a password and KORG_DB_PASSWORD is also set. These \
             are two sources for one credential and korg will not choose between \
             them: drop the password from DATABASE_URL (the deployed shape, korg \
             #2547), or unset KORG_DB_PASSWORD (the local-development shape)."
        ));
    }

    Ok(options.password(password))
}

/// Whether `url`'s userinfo section carries a password.
///
/// Answered from the raw string because `PgConnectOptions` exposes no password
/// getter — and it only has to be right about *presence*, never about the value.
/// The authority runs from `://` to the first `/`, `?` or `#`; the userinfo is
/// what precedes its **last** `@`, so a raw `@` inside a password is attributed
/// to the password rather than treated as the host separator.
fn url_carries_password(url: &str) -> bool {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = after_scheme
        .find(['/', '?', '#'])
        .map_or(after_scheme, |end| &after_scheme[..end]);
    authority
        .rsplit_once('@')
        .is_some_and(|(userinfo, _)| userinfo.contains(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEPLOYED: &str = "postgres://korg@postgresql:5432/korg";
    const WITH_PASSWORD: &str = "postgres://korg:sekrit@postgresql:5432/korg";

    #[test]
    fn a_url_without_a_password_is_recognised_as_such() {
        assert!(!url_carries_password(DEPLOYED));
        assert!(!url_carries_password("postgres://postgresql:5432/korg"));
        // No userinfo at all, and a socket URL whose authority is empty.
        assert!(!url_carries_password(
            "postgres:///korg?host=/var/run/postgresql"
        ));
        // A colon in the path or query is not a userinfo colon.
        assert!(!url_carries_password("postgres://korg@host/db?opt=a:b"));
    }

    #[test]
    fn a_url_with_a_password_is_recognised_as_such() {
        assert!(url_carries_password(WITH_PASSWORD));
        // The host separator is the LAST `@`, so a raw `@` in the password does
        // not hide the credential from this check.
        assert!(url_carries_password(
            "postgres://korg:pw@word@postgresql:5432/korg"
        ));
    }

    #[test]
    fn no_password_variable_leaves_the_url_alone() {
        let options = resolve_connect_options(WITH_PASSWORD, None).expect("resolve");
        assert_eq!(options.get_username(), "korg");
        assert_eq!(options.get_host(), "postgresql");
    }

    #[test]
    fn the_deployed_shape_resolves() {
        let options = resolve_connect_options(DEPLOYED, Some("sekrit")).expect("resolve");
        assert_eq!(options.get_username(), "korg");
        assert_eq!(options.get_host(), "postgresql");
        assert_eq!(options.get_port(), 5432);
        assert_eq!(options.get_database(), Some("korg"));
    }

    /// A password containing the two characters `deploy/cold-start.sh` used to
    /// refuse. Nothing here has to escape them, which is the point.
    #[test]
    fn a_password_with_url_metacharacters_is_accepted() {
        resolve_connect_options(DEPLOYED, Some("a@b/c:d#e?f")).expect("resolve");
    }

    #[test]
    fn two_sources_for_one_password_is_an_error() {
        let err = resolve_connect_options(WITH_PASSWORD, Some("other"))
            .expect_err("two sources must be refused");
        let message = err.to_string();
        assert!(message.contains("two sources"), "{message}");
        // The refusal must not quote either credential back.
        assert!(!message.contains("sekrit"), "{message}");
        assert!(!message.contains("other"), "{message}");
    }

    #[test]
    fn an_empty_password_variable_is_an_error() {
        let err = resolve_connect_options(DEPLOYED, Some("")).expect_err("empty must be refused");
        assert!(err.to_string().contains("set but empty"), "{err}");
    }

    #[test]
    fn an_unparseable_url_does_not_echo_itself() {
        let err =
            resolve_connect_options("not a url:sekrit@nowhere", None).expect_err("must be refused");
        assert!(!err.to_string().contains("sekrit"), "{err}");
    }
}
