//! Sprint 010 — status vocabulary (WI #285), editable comments (WI #232),
//! project metadata (WI #246).

use korg_core::repo::{
    add_comment, create_project, create_work_item, list_comments, list_projects, update_comment,
    update_project_by_name, update_work_item, NewWorkItem, ProjectPatch, WorkItemPatch,
    PROJECT_CATEGORIES, WI_STATUSES,
};
use korg_test_support::{fresh_korg, new};

fn wi(title: &str, project_id: i64, status: &str) -> NewWorkItem {
    NewWorkItem {
        project_id: Some(project_id),
        wi_status: status.into(),
        content: "body".into(),
        ..new::work_item(title)
    }
}

#[tokio::test]
async fn wi_status_vocabulary_is_enforced() {
    let (_c, pool) = fresh_korg().await;
    let pid = create_project(&pool, "p").await.unwrap();

    // Every canonical status is accepted at creation.
    for s in WI_STATUSES {
        create_work_item(&pool, wi(&format!("as {s}"), pid, s))
            .await
            .unwrap_or_else(|e| panic!("status '{s}' should be valid: {e}"));
    }

    // Dead vocabulary ("active"/"draft" once lived in the web constant) and
    // typos are rejected at creation…
    for s in ["active", "draft", "Done", "bogus"] {
        assert!(
            create_work_item(&pool, wi(&format!("as {s}"), pid, s))
                .await
                .is_err(),
            "status '{s}' should be rejected"
        );
    }

    // …and on update.
    let r = create_work_item(&pool, wi("patch me", pid, "open"))
        .await
        .unwrap();
    let ok = WorkItemPatch {
        wi_status: Some("done".into()),
        ..Default::default()
    };
    update_work_item(&pool, r.wi_number, ok).await.unwrap();
    let bad = WorkItemPatch {
        wi_status: Some("finished".into()),
        ..Default::default()
    };
    assert!(update_work_item(&pool, r.wi_number, bad).await.is_err());
}

#[tokio::test]
async fn comments_are_editable() {
    let (_c, pool) = fresh_korg().await;
    let pid = create_project(&pool, "p").await.unwrap();
    let r = create_work_item(&pool, wi("holder", pid, "open"))
        .await
        .unwrap();

    let c = add_comment(&pool, r.node_id, "forgot the WI #")
        .await
        .unwrap();
    let edited = update_comment(&pool, c.id, "refers to WI #42")
        .await
        .unwrap();

    assert_eq!(edited.id, c.id);
    assert_eq!(edited.body, "refers to WI #42");
    assert_eq!(edited.created, c.created, "created must be preserved");

    let listed = list_comments(&pool, r.node_id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].body, "refers to WI #42");

    assert!(update_comment(&pool, 999_999, "nope").await.is_err());
}

#[tokio::test]
async fn project_metadata_roundtrip() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "meta").await.unwrap();

    // Migration defaults.
    let p = list_projects(&pool).await.unwrap().remove(0);
    assert_eq!(p.status, "active");
    assert!(p.machines.is_empty() && p.deploy_to.is_empty());
    assert_eq!(p.category, None);

    update_project_by_name(
        &pool,
        "meta",
        &ProjectPatch {
            status: Some("archived".into()),
            machines: Some(vec!["kai".into(), "kubs0".into()]),
            deploy_to: Some(vec!["kubsdb".into()]),
            // Was "tooling" — free text until WI #678 closed the vocabulary.
            category: Some(Some("Infrastructure".into())),
            description: Some(Some("desc".into())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let p = list_projects(&pool).await.unwrap().remove(0);
    assert_eq!(p.status, "archived");
    assert_eq!(p.machines, vec!["kai", "kubs0"]);
    assert_eq!(p.deploy_to, vec!["kubsdb"]);
    assert_eq!(p.category.as_deref(), Some("Infrastructure"));

    // Invalid project status rejected; unknown project errors; name immutable
    // by construction (no field for it).
    let bad = ProjectPatch {
        status: Some("paused".into()),
        ..Default::default()
    };
    assert!(update_project_by_name(&pool, "meta", &bad).await.is_err());
    let ok = ProjectPatch {
        status: Some("active".into()),
        ..Default::default()
    };
    assert!(update_project_by_name(&pool, "nope", &ok).await.is_err());
}

/// WI #678 — `project.category` is a closed vocabulary, not free text.
///
/// The pre-#678 corpus held `ai`, `AI`, `tooling`, `infra`, `fun` and NULL for
/// what were five categories' worth of meaning, because nothing validated it.
/// Migration 0018 trues those up; this is the enforcement that keeps them true.
#[tokio::test]
async fn project_category_vocabulary_is_enforced() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "cat").await.unwrap();

    let set = |c: Option<&str>| ProjectPatch {
        category: Some(c.map(Into::into)),
        ..Default::default()
    };

    for c in PROJECT_CATEGORIES {
        update_project_by_name(&pool, "cat", &set(Some(c)))
            .await
            .unwrap_or_else(|e| panic!("category '{c}' should be valid: {e}"));
    }

    // The retired free-text values are rejected — including the case variants,
    // which is half of why the corpus drifted in the first place.
    for c in ["tooling", "infra", "ai", "eval", "ops", "", "Nonsense"] {
        let err = update_project_by_name(&pool, "cat", &set(Some(c)))
            .await
            .expect_err("off-vocabulary category should be rejected");
        // The message carries the whole allowed set, so a caller can retry.
        assert!(
            err.to_string().contains("Infrastructure"),
            "error should list the vocabulary, got: {err}"
        );
    }

    // A rejected write leaves the previous value alone.
    let last = PROJECT_CATEGORIES[PROJECT_CATEGORIES.len() - 1];
    let p = list_projects(&pool).await.unwrap().remove(0);
    assert_eq!(p.category.as_deref(), Some(last));

    // Clearing stays legal: create_project takes only a name, so uncategorised
    // is a state a project can be in and be returned to.
    update_project_by_name(&pool, "cat", &set(None))
        .await
        .unwrap();
    let p = list_projects(&pool).await.unwrap().remove(0);
    assert_eq!(p.category, None);
}

/// WI #678 — migration 0018's true-up, re-applied over legacy values.
///
/// The *mapping* is Ken's taxonomy and is verified by rehearsal against a
/// restored dump (docs/operations.md, "Rehearsing a data migration") — no test
/// can second-guess which category a project belongs to. What is worth pinning
/// here is the mechanism, which has two failure modes a diff would not show.
///
/// First, `project_touch_updated` (0013) fires BEFORE UPDATE, so a bulk
/// category set stamps `updated` on every project it touches. Migration 0016
/// hit exactly this and disabled the trigger for the same reason: a fix-up is
/// not a content edit. Second, the postcondition has to actually fire — an
/// assertion that cannot fail is not an assertion.
///
/// Re-running the file is safe (the UPDATE is guarded by IS DISTINCT FROM), so
/// this seeds the pre-#678 free-text values on an already-migrated database and
/// applies the real file again.
#[tokio::test]
async fn migration_0018_trues_up_categories_without_touching_updated() {
    const SQL: &str = include_str!("../migrations/0018_project_category_vocabulary.sql");

    let (_c, pool) = fresh_korg().await;
    for name in ["korg", "klams", "hv-simulator", "brand-new"] {
        create_project(&pool, name).await.unwrap();
    }

    // The values the live corpus actually held, written straight to the column:
    // update_project would (correctly) refuse them now. The trigger is off
    // across the seed so that `updated` reflects "these were always here",
    // which is the state production is really in.
    sqlx::raw_sql(
        "ALTER TABLE project DISABLE TRIGGER project_touch_updated; \
         UPDATE project SET category = 'tooling' WHERE name = 'korg'; \
         UPDATE project SET category = 'ai'      WHERE name = 'klams'; \
         UPDATE project SET category = 'fun'     WHERE name = 'hv-simulator'; \
         ALTER TABLE project ENABLE TRIGGER project_touch_updated; \
         CREATE TABLE updated_snapshot AS SELECT name, updated FROM project;",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(SQL).execute(&pool).await.unwrap();

    let category = |name: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>("SELECT category FROM project WHERE name = $1")
                .bind(name)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(category("korg").await.as_deref(), Some("Infrastructure"));
    assert_eq!(category("klams").await.as_deref(), Some("AI"));
    assert_eq!(category("hv-simulator").await.as_deref(), Some("Fun"));
    // Not in the mapping — a project created after it was written stays
    // uncategorised, which is legal and reported rather than failed.
    assert_eq!(category("brand-new").await, None);

    let touched: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM project p JOIN updated_snapshot s ON s.name = p.name \
         WHERE p.updated IS DISTINCT FROM s.updated",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(touched, 0, "the true-up must not bump project.updated");

    // The postcondition fires: an off-vocabulary value the mapping does not
    // cover cannot be left behind for enforcement to trip over later.
    sqlx::query("UPDATE project SET category = 'tooling' WHERE name = 'brand-new'")
        .execute(&pool)
        .await
        .unwrap();
    let err = sqlx::raw_sql(SQL)
        .execute(&pool)
        .await
        .expect_err("postcondition should reject a leftover off-vocabulary category");
    assert!(
        err.to_string().contains("off_vocab"),
        "expected the postcondition's message, got: {err}"
    );
}
