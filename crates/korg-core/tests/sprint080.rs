//! Sprint 080 — the database password arrives as its own variable.
//!
//! The unit tests in `korg_core::db` cover the resolution rules against
//! strings. These cover the part strings cannot: that a `DATABASE_URL` with no
//! credential in it plus a separate `KORG_DB_PASSWORD` actually authenticates
//! against a real Postgres — **and that a wrong one does not**.
//!
//! The control is the whole point. "korg still connects" proves nothing on its
//! own: a server that trusted every connection would pass the positive test and
//! leave a broken password path undetected. So the wrong-password case has to
//! fail here, and if the container is ever configured to trust the host, this
//! suite fails rather than quietly becoming a tautology.

use korg_core::db::{connect_with, resolve_connect_options};
use korg_test_support::start_pg;

/// The testcontainer's own superuser password (`Postgres::default()`).
const REAL: &str = "postgres";

/// The URL korg is deployed with: server, role and database, no credential.
fn passwordless_url(port: u16) -> String {
    format!("postgres://postgres@127.0.0.1:{port}/postgres")
}

#[tokio::test]
async fn a_separated_password_authenticates_and_a_wrong_one_does_not() {
    let pg = start_pg().await;
    let url = passwordless_url(pg.port);

    // The control first, so a trust-authentication container cannot let the
    // positive case stand in for a proof.
    let wrong = resolve_connect_options(&url, Some("not-the-password")).expect("resolve");
    let error = connect_with(wrong).await.expect_err(
        "a wrong KORG_DB_PASSWORD must be refused — if this connected, the \
         container is trusting the host and this suite proves nothing",
    );
    let message = error.to_string().to_lowercase();
    assert!(
        message.contains("password") || message.contains("authentication"),
        "expected an authentication failure, got: {error}"
    );

    // And the real one, through the same path the deployed binary takes.
    let good = resolve_connect_options(&url, Some(REAL)).expect("resolve");
    let pool = connect_with(good)
        .await
        .expect("the real password must authenticate");
    let one: i32 = sqlx::query_scalar("select 1")
        .fetch_one(&pool)
        .await
        .expect("query the connected pool");
    assert_eq!(one, 1);
}

/// The URL-embedded shape still works, because it is what every other test in
/// this workspace, the importer and the kai dev loop use.
#[tokio::test]
async fn the_embedded_shape_still_connects() {
    let pg = start_pg().await;
    let options = resolve_connect_options(&pg.url("postgres"), None).expect("resolve");
    let pool = connect_with(options).await.expect("connect");
    let one: i32 = sqlx::query_scalar("select 1")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(one, 1);
}

/// A password holding every character that would have corrupted the old
/// URL-splicing shape — the constraint `deploy/cold-start.sh` used to enforce by
/// refusing to write such a password at all.
///
/// Set on the role for real and then authenticated with, so this is a statement
/// about Postgres and sqlx rather than about korg's string handling.
#[tokio::test]
async fn a_password_with_url_metacharacters_authenticates() {
    let pg = start_pg().await;
    let awkward = "a@b/c:d#e?f";

    let admin = connect_with(resolve_connect_options(&pg.url("postgres"), None).expect("resolve"))
        .await
        .expect("connect as the superuser");
    sqlx::query("create role awkward login password 'a@b/c:d#e?f'")
        .execute(&admin)
        .await
        .expect("create the role");
    // Its own database, so the role can run korg's migrations and this exercises
    // the whole deployed path rather than stopping at the handshake.
    sqlx::query("create database awkward owner awkward")
        .execute(&admin)
        .await
        .expect("create the database");

    let url = format!("postgres://awkward@127.0.0.1:{}/awkward", pg.port);

    // Control: the same role, a different password.
    let refused =
        connect_with(resolve_connect_options(&url, Some("a@b/c:d#e?g")).expect("resolve")).await;
    assert!(
        refused.is_err(),
        "a near-miss password must still be refused"
    );

    let pool = connect_with(resolve_connect_options(&url, Some(awkward)).expect("resolve"))
        .await
        .expect("a password containing @ / : # ? must authenticate");
    let who: String = sqlx::query_scalar("select current_user")
        .fetch_one(&pool)
        .await
        .expect("query");
    assert_eq!(who, "awkward");
}
