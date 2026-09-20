//! Sprint 086 — `src_path` learns the drive-root form for Windows clones.
//!
//! WI 2874, slice 1 of program korg:2916. cleo's clones live at
//! `D:\ClaudeWorks\<name>`, which 0019's `~`-relative canonical form could not
//! hold, so four live projects carried NULL for want of a spelling. The form
//! is `/d/ClaudeWorks/kctrldeck` — MSYS/git-bash, mechanical in both
//! directions, and unambiguous against `~/` because a POSIX absolute path was
//! never valid in this field.
//!
//! The widening is narrow on purpose, and the tests that matter most here are
//! the ones asserting what is STILL refused: every rule 0019 fixed is shared
//! by both forms, and `/home/ken/...` — the near-miss that would quietly
//! reintroduce absolute POSIX paths — must keep failing.

use korg_core::error::{ErrorClass, ErrorCode};
use korg_core::repo::{canonical_src_path, update_project_by_name, ProjectPatch};
use korg_test_support::fresh_korg;
use sqlx::PgPool;

fn code(e: &anyhow::Error) -> ErrorCode {
    e.code()
}

async fn make_project(pool: &PgPool, name: &str) {
    sqlx::query("INSERT INTO project (name, status) VALUES ($1, 'active')")
        .bind(name)
        .execute(pool)
        .await
        .expect("seed project");
}

fn patch(value: &str) -> ProjectPatch {
    ProjectPatch {
        src_path: Some(Some(value.into())),
        ..Default::default()
    }
}

/// The four shapes cleo actually needs, end to end through the write path —
/// so the app-side validator and migration 0036's CHECK are both proved, in
/// the order a caller meets them.
#[tokio::test]
async fn a_drive_root_path_stores() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;

    for value in [
        "/d/ClaudeWorks/kctrldeck",
        "/d/ClaudeWorks/kbrickshoot",
        "/d/ClaudeWorks/kpidashclient-win",
        "/d/ClaudeWorks/krcmd",
        // Not only `d:` — the form is the drive letter, whichever it is.
        "/c/Users/kenhi/src/thing",
        // One segment deep is still a path.
        "/e/repo",
    ] {
        let ok = update_project_by_name(&pool, "subject", &patch(value))
            .await
            .unwrap_or_else(|e| panic!("{value} is canonical and must store: {e}"));
        assert_eq!(ok.src_path.as_deref(), Some(value));
    }
}

/// The `~/` form is untouched. A widening that quietly changed the form 46
/// projects already use would be a far worse bug than the one it fixes.
#[tokio::test]
async fn the_tilde_form_is_unchanged() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;

    let ok = update_project_by_name(&pool, "subject", &patch("~/src/tools/korg"))
        .await
        .expect("the canonical POSIX form still stores");
    assert_eq!(ok.src_path.as_deref(), Some("~/src/tools/korg"));
}

/// What stays refused, and why each one is worth an assertion rather than a
/// comment.
#[tokio::test]
async fn the_widening_does_not_admit_the_near_misses() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;

    for (value, why) in [
        // The regex's whole load-bearing detail: `^(~|/[a-z])/` needs a
        // separator immediately after the drive letter, so a POSIX absolute
        // path fails on its second character and 0019's rule still binds.
        ("/home/ken/src/tools/korg", "an absolute POSIX path"),
        ("/usr/local/src/thing", "another absolute POSIX path"),
        // One spelling per clone: an uppercase drive would let two rows
        // describe the same directory.
        ("/D/ClaudeWorks/kctrldeck", "an uppercase drive letter"),
        // The native form is what the CONSUMER is handed, never what is
        // stored — storing it would put a backslash-escaping problem in a
        // field every agent reads.
        (r"D:\ClaudeWorks\kctrldeck", "the native Windows spelling"),
        ("D:/ClaudeWorks/kctrldeck", "a drive-colon path"),
        // Every 0019 rule is shared by the new form, not waived for it.
        (
            "/d/ClaudeWorks/kctrldeck/",
            "a trailing slash on a drive path",
        ),
        ("/d/Claude Works/kctrldeck", "whitespace in a drive path"),
        (
            "/d/ClaudeWorks/kctrldeck (cleo)",
            "prose after a drive path",
        ),
        // A bare drive root names a disk, not a working copy.
        ("/d/", "a bare drive root"),
        // Multi-character roots are not drives.
        ("/dd/ClaudeWorks/x", "a two-letter root"),
    ] {
        let err = update_project_by_name(&pool, "subject", &patch(value))
            .await
            .err()
            .unwrap_or_else(|| panic!("{why} must be refused: {value}"));

        assert_eq!(
            code(&err),
            ErrorCode::InvalidInput,
            "{why}: a correctable input error must not read as korg's fault"
        );
        let message = err.to_string();
        assert!(
            !message.contains("check constraint"),
            "{why}: the app-side validator must catch this before the CHECK does, \
             so raw Postgres text never reaches a caller: {message}"
        );
        assert!(
            message.contains("/d/"),
            "{why}: the remedy must name the drive form a caller should have used, \
             since that is the whole thing they cannot guess: {message}"
        );
    }
}

/// A Windows path gets the remedy it needs, not the one for a POSIX path.
///
/// `D:\ClaudeWorks\kctrldeck` is the value an agent holding a cleo checkout
/// will actually try first. Telling it "write it relative to home" — the
/// answer every other absolute path gets — sends it to a home directory that
/// does not exist on that host.
#[tokio::test]
async fn a_native_windows_path_is_told_the_drive_form() {
    let (_c, pool) = fresh_korg().await;
    make_project(&pool, "subject").await;

    let err = update_project_by_name(&pool, "subject", &patch(r"D:\ClaudeWorks\kctrldeck"))
        .await
        .expect_err("the native spelling is refused");

    let message = err.to_string();
    assert!(
        message.contains("/d/ClaudeWorks"),
        "the message must show the conversion of the value the caller sent, not a \
         generic example: {message}"
    );
    assert!(
        !message.contains("relative to home"),
        "a Windows clone has no `~/` to be relative to — that remedy is for POSIX \
         paths and is actively misleading here: {message}"
    );
    assert_eq!(code(&err), ErrorCode::InvalidInput);
}

/// `canonical_src_path` leaves a drive path alone.
///
/// It is the import path's mechanical fixer (`korg-migrate`), and its rules
/// are ordered so the missing-`~/` rule cannot mistake `/home/ken/…` for a
/// relative path. A drive path is absolute for the same reason, and must fall
/// through the same way rather than becoming `~//d/…`.
#[test]
fn canonicalisation_does_not_mangle_a_drive_path() {
    assert_eq!(
        canonical_src_path("/d/ClaudeWorks/kctrldeck"),
        "/d/ClaudeWorks/kctrldeck"
    );
    // The one rule it does apply to every form.
    assert_eq!(
        canonical_src_path("/d/ClaudeWorks/kctrldeck/"),
        "/d/ClaudeWorks/kctrldeck"
    );
    // And the POSIX rules it already had, unchanged.
    assert_eq!(canonical_src_path("/home/ken/src/x"), "~/src/x");
    assert_eq!(canonical_src_path("src/x"), "~/src/x");
}
