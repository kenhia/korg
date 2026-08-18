//! Sprint 066 — korg-native full-text search (WI #1177, proposal korg:1395).
//!
//! The engine choice is measured elsewhere (see the sprint record). What is
//! pinned here is the *contract*: which documents are reachable, what the
//! defaults hide, and the two failure modes this feature was built out of —
//! a relaxation that reaches only some query paths, and an identifier that
//! parses as an operator.

use korg_core::repo::{
    add_comment, create_project, create_proposal, create_work_item, search, update_work_item,
    SearchQuery, WorkItemPatch,
};
use korg_test_support::{fresh_korg, new};

fn q(text: &str) -> SearchQuery {
    SearchQuery {
        q: text.into(),
        ..Default::default()
    }
}

/// Scope `"all"` — the acceptance suite's setting, and the one every historical
/// question needs.
fn all(text: &str) -> SearchQuery {
    SearchQuery {
        scope: Some("all".into()),
        archived: None,
        ..q(text)
    }
}

fn locators(r: &korg_core::repo::SearchResults) -> Vec<String> {
    r.items.iter().map(|h| h.locator.clone()).collect()
}

/// The Gate A M3 defect, as a test korg cannot regress into.
///
/// khound relaxed a conjunctive query to any-term on its unfiltered path and
/// not on the filtered one, so every multi-word prose query returned zero from
/// each filtered leg while the legs reported healthy. korg has one query
/// builder, and this asserts the relaxation survives every narrowing that
/// builder accepts — because "one code path" is a claim, and this is the
/// evidence for it.
#[tokio::test]
async fn the_relaxation_reaches_every_filtered_path() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();
    let wi = create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            project: Some("korg".into()),
            content: "the worker drop does not increment the writes_failed counter".into(),
            ..new::work_item("Oversized chunks silently dropped")
        },
    )
    .await
    .unwrap();

    // Not every term is in the document, so the strict all-terms parse cannot
    // match. Each of these must still find it, via the any-term relaxation.
    let text = "does the oversized chunk drop show up in the writes_failed metric";
    let cases: Vec<(&str, SearchQuery)> = vec![
        ("unfiltered", all(text)),
        (
            "kind-filtered",
            SearchQuery {
                kind: Some("workitem".into()),
                ..all(text)
            },
        ),
        (
            "project-filtered",
            SearchQuery {
                project: Some("korg".into()),
                ..all(text)
            },
        ),
        (
            "kind+project-filtered",
            SearchQuery {
                kind: Some("workitem".into()),
                project: Some("korg".into()),
                ..all(text)
            },
        ),
    ];
    for (name, query) in cases {
        let r = search(&pool, query).await.unwrap();
        assert!(
            locators(&r).contains(&format!("WI-{}", wi.wi_number)),
            "{name} path did not relax: {:?}",
            locators(&r)
        );
        assert!(r.relaxed, "{name} path answered without reporting relaxed");
    }
}

/// `WI-836` must not parse as "WI, not 836", and a strict match must not be
/// reported as a relaxed one.
#[tokio::test]
async fn an_identifier_is_a_term_not_an_operator() {
    let (_c, pool) = fresh_korg().await;
    let target = create_work_item(
        &pool,
        new::work_item("A record about nothing in particular"),
    )
    .await
    .unwrap();
    create_work_item(&pool, new::work_item("Unrelated prose about widgets"))
        .await
        .unwrap();

    let r = search(&pool, all(&format!("WI-{}", target.wi_number)))
        .await
        .unwrap();
    assert_eq!(r.items[0].locator, format!("WI-{}", target.wi_number));
    assert!(!r.relaxed, "an identifier hit should be a strict match");
}

/// Comments are documents, tied to their parent's locator — the increment over
/// a title scan, and the reason this feature exists at all.
#[tokio::test]
async fn comments_are_documents_reachable_by_the_parents_identifier() {
    let (_c, pool) = fresh_korg().await;
    let wi = create_work_item(&pool, new::work_item("A work item with a plain title"))
        .await
        .unwrap();
    add_comment(
        &pool,
        wi.node_id,
        "the audit log is append-only JSONL under ~/.local/state/kyac/",
    )
    .await
    .unwrap();

    // The body lives only on the comment, so a title scan cannot answer this.
    let r = search(&pool, all("append-only JSONL audit log"))
        .await
        .unwrap();
    let hit = r
        .items
        .iter()
        .find(|h| h.comment_id.is_some())
        .expect("the comment should be a hit");
    assert_eq!(hit.kind, "comment");
    assert_eq!(
        hit.locator,
        format!("korg:{}#comment-{}", wi.node_id, hit.comment_id.unwrap())
    );
    assert_eq!(
        hit.node_id, wi.node_id,
        "a comment hit routes to its parent"
    );

    // And the parent's identifier reaches its comments.
    let by_id = search(&pool, all(&format!("WI-{}", wi.wi_number)))
        .await
        .unwrap();
    assert!(
        locators(&by_id).iter().any(|l| l.contains("#comment-")),
        "the parent identifier should reach its comments: {:?}",
        locators(&by_id)
    );
}

/// The default scope hides each kind's own terminal state and says how much it
/// hid; `"all"` is how a settled decision is found again.
#[tokio::test]
async fn the_default_scope_hides_terminal_rows_and_reports_the_count() {
    let (_c, pool) = fresh_korg().await;
    let wi = create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            content: "TRUNCATE node, project, area RESTART IDENTITY CASCADE".into(),
            ..new::work_item("What reset truncates")
        },
    )
    .await
    .unwrap();

    let live = search(&pool, q("TRUNCATE RESTART IDENTITY")).await.unwrap();
    assert_eq!(live.total, 1, "an open item is live");
    assert_eq!(live.omitted.terminal, 0);

    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            wi_status: Some("closed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let live = search(&pool, q("TRUNCATE RESTART IDENTITY")).await.unwrap();
    assert_eq!(live.total, 0, "a closed item is not live");
    assert_eq!(
        live.omitted.terminal, 1,
        "and the default must say what it hid, not just return nothing"
    );

    let every = search(&pool, all("TRUNCATE RESTART IDENTITY"))
        .await
        .unwrap();
    assert_eq!(every.total, 1);
    assert_eq!(every.omitted.terminal, 0, "asking for all omits nothing");
}

/// A closed parent takes its comments out of the live view with it — otherwise
/// the default hides a decision and shows the discussion of it.
#[tokio::test]
async fn a_terminal_parent_hides_its_comments_too() {
    let (_c, pool) = fresh_korg().await;
    let wi = create_work_item(&pool, new::work_item("Parent"))
        .await
        .unwrap();
    add_comment(&pool, wi.node_id, "quarantine the flapping heater probe")
        .await
        .unwrap();
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            wi_status: Some("closed".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let live = search(&pool, q("flapping heater probe")).await.unwrap();
    assert_eq!(live.total, 0, "the comment follows its parent out of view");
    assert!(live.omitted.terminal >= 1);
    assert_eq!(
        search(&pool, all("flapping heater probe"))
            .await
            .unwrap()
            .total,
        1
    );
}

/// An explicit exclusion is a precise query. Relaxing `foo -bar` to
/// `foo | !bar` would return every document without `bar`, which is the
/// opposite of what was asked.
#[tokio::test]
async fn an_exclusion_is_never_relaxed() {
    let (_c, pool) = fresh_korg().await;
    create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            content: "the registry deploy path stamps the image".into(),
            ..new::work_item("Deploy notes")
        },
    )
    .await
    .unwrap();

    // `registry` matches; `-deploy` excludes the only document carrying it.
    let r = search(&pool, all("registry nonexistentterm -deploy"))
        .await
        .unwrap();
    assert_eq!(r.total, 0, "an excluded document must stay excluded");
    assert!(!r.relaxed, "a query with an exclusion is never relaxed");
}

/// There is no index to rebuild and nothing to refresh: the tsvector is
/// maintained inside the writing transaction, so the write is searchable the
/// moment it lands. This is the property that let migration 0029 delete the
/// whole staleness story #1177 budgeted for.
#[tokio::test]
async fn a_write_is_searchable_immediately() {
    let (_c, pool) = fresh_korg().await;
    assert_eq!(search(&pool, all("aurorafish")).await.unwrap().total, 0);

    let wi = create_work_item(
        &pool,
        korg_core::repo::NewWorkItem {
            content: "aurorafish is a nonsense token".into(),
            ..new::work_item("Nonsense")
        },
    )
    .await
    .unwrap();
    assert_eq!(search(&pool, all("aurorafish")).await.unwrap().total, 1);

    // Including an edit that removes the term again.
    update_work_item(
        &pool,
        wi.wi_number,
        WorkItemPatch {
            content: Some("the token is gone".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(search(&pool, all("aurorafish")).await.unwrap().total, 0);
}

/// Proposals are searchable through `notes`, the unbounded field the 500-char
/// summary cannot hold — which is exactly where a sprint's analysis lives.
#[tokio::test]
async fn a_proposals_notes_are_searchable() {
    let (_c, pool) = fresh_korg().await;
    create_project(&pool, "korg").await.unwrap();
    let p = create_proposal(
        &pool,
        korg_core::repo::NewProposal {
            notes: Some("measured before assuming: the sweep rejected ts_rank_cd".into()),
            ..new::proposal_in("korg", "A proposal with a short summary")
        },
    )
    .await
    .unwrap();

    let r = search(&pool, all("the sweep rejected ts_rank_cd"))
        .await
        .unwrap();
    assert_eq!(r.items[0].locator, format!("korg:{}", p.row.node_id));
    assert_eq!(r.items[0].kind, "sprint_proposal");
}

/// `total` is the whole filtered corpus on every page, including one whose
/// offset overshoots the last row (WI #883) — counted in its own statement,
/// never with a window function riding the paged rows.
#[tokio::test]
async fn total_survives_an_overshooting_offset() {
    let (_c, pool) = fresh_korg().await;
    for i in 0..3 {
        create_work_item(
            &pool,
            korg_core::repo::NewWorkItem {
                content: "shared marker token".into(),
                ..new::work_item(&format!("Item {i}"))
            },
        )
        .await
        .unwrap();
    }
    let overshoot = search(
        &pool,
        SearchQuery {
            page: korg_core::repo::PageQuery {
                limit: Some(2),
                offset: Some(99),
            },
            ..all("shared marker token")
        },
    )
    .await
    .unwrap();
    assert!(overshoot.items.is_empty());
    assert_eq!(overshoot.total, 3, "an empty page still reports the corpus");
}
